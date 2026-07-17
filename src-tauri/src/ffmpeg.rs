use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandEvent;

// ── VP9 encoding ──────────────────────────────────────────────────────────────

/// Re-encode input to a VP9 WebM conforming to ISDCF Doc 13.
/// `vf_filter` is the FFmpeg -vf argument (scale or crop+scale).
/// `target_kbps` is the video bitrate target in kbit/s; it also drives the VBV
/// so no single 2-second window can blow past the PCM block budget.
/// Calls `on_progress(percent)` with 0.0–100.0 as encoding proceeds.
pub async fn encode_vp9(
    app: &AppHandle,
    input: &Path,
    output: &Path,
    vf_filter: &str,
    target_kbps: u32,
    duration_secs: f64,
    cancel: &Arc<AtomicBool>,
    on_progress: impl Fn(f64),
) -> Result<()> {
    // Constrained VBR: cap the peak rate and keep the VBV buffer short (~1s) so
    // that any given 2-second GOP stays close to `target_kbps * 2` bits.
    let rate = format!("{}k", target_kbps);

    let args = [
        "-y",
        "-i", &input.to_string_lossy(),
        "-c:v", "libvpx-vp9",
        "-b:v", &rate,
        "-maxrate", &rate,
        "-bufsize", &rate,
        "-vf", vf_filter,
        "-r", "24",
        "-pix_fmt", "yuv420p",
        "-an",
        "-g", "48",
        "-keyint_min", "48",
        // Force a keyframe exactly every 2s (0s, 2s, 4s…) so the segment muxer
        // cuts on clean 2-second boundaries — essential for the chunk model.
        "-force_key_frames", "expr:gte(t,n_forced*2)",
        "-deadline", "good",
        "-cpu-used", "2",
        &output.to_string_lossy(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();

    let (mut rx, child) = app
        .shell()
        .sidecar("ffmpeg")?
        .args(args)
        .spawn()?;

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            return Err(anyhow!("Cancelado pelo usuário."));
        }

        match rx.recv().await {
            Some(CommandEvent::Stderr(bytes)) => {
                let line = String::from_utf8_lossy(&bytes).to_string();
                if let Some(pct) = parse_ffmpeg_time(&line, duration_secs) {
                    on_progress(pct);
                }
            }
            Some(CommandEvent::Terminated(p)) => {
                if p.code != Some(0) {
                    return Err(anyhow!(
                        "A codificação falhou (código {:?}). Verifique se o vídeo está íntegro.",
                        p.code
                    ));
                }
                break;
            }
            None => break,
            _ => {}
        }
    }

    Ok(())
}

// ── WebM segmentation ─────────────────────────────────────────────────────────

/// Cut a VP9 WebM into 2-second chunks using FFmpeg segment muxer.
/// Returns sorted list of chunk file paths.
pub async fn segment_webm(
    app: &AppHandle,
    input: &Path,
    chunk_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let pattern = chunk_dir.join("chunk_%05d.webm");

    let args = [
        "-y",
        "-i", &input.to_string_lossy(),
        "-c", "copy",
        "-f", "segment",
        "-segment_time", "2",
        "-reset_timestamps", "1",
        &pattern.to_string_lossy(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();

    let output = app.shell().sidecar("ffmpeg")?.args(args).output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Segmentação falhou: {}", stderr.trim()));
    }

    collect_chunks(chunk_dir)
}

// ── WebM concatenation ────────────────────────────────────────────────────────

/// Concatenate WebM chunks into a single output file.
/// `chunk_paths` must be in playback order.
pub async fn concat_webm(
    app: &AppHandle,
    chunk_paths: &[PathBuf],
    output: &Path,
) -> Result<()> {
    // Write FFmpeg concat list
    let list_path = output.with_extension("concat.txt");
    let list_content: String = chunk_paths
        .iter()
        .map(|p| format!("file '{}'\n", p.to_string_lossy()))
        .collect();
    std::fs::write(&list_path, &list_content)?;

    let args = [
        "-y",
        "-f", "concat",
        "-safe", "0",
        "-i", &list_path.to_string_lossy(),
        "-c", "copy",
        &output.to_string_lossy(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();

    let result = app.shell().sidecar("ffmpeg")?.args(args).output().await?;
    let _ = std::fs::remove_file(&list_path);

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow!("Concatenação falhou: {}", stderr.trim()));
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn collect_chunks(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut chunks: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|x| x == "webm").unwrap_or(false)
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("chunk_"))
                    .unwrap_or(false)
        })
        .collect();

    if chunks.is_empty() {
        return Err(anyhow!("Nenhum chunk gerado. O vídeo pode ser muito curto."));
    }

    chunks.sort();
    Ok(chunks)
}

/// Parse `time=HH:MM:SS.ss` from FFmpeg stderr and return progress 0–100.
fn parse_ffmpeg_time(line: &str, total: f64) -> Option<f64> {
    if total <= 0.0 {
        return None;
    }
    let pos = line.find("time=")?;
    let rest = &line[pos + 5..];
    let end = rest.find(|c: char| c == ' ' || c == '\n').unwrap_or(rest.len());
    let t = &rest[..end];

    let parts: Vec<&str> = t.splitn(3, ':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: f64 = parts[0].parse().ok()?;
    let m: f64 = parts[1].parse().ok()?;
    let s: f64 = parts[2].parse().ok()?;

    let elapsed = h * 3600.0 + m * 60.0 + s;
    Some((elapsed / total * 100.0).clamp(0.0, 100.0))
}

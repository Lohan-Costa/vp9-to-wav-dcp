use anyhow::{anyhow, Result};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;

use tauri::Emitter;

use crate::ffmpeg;
use crate::wav;
use crate::ProgressEvent;

// PCM block constants (ISDCF Doc 13 §3.3)
const BLOCK_SIZE: usize = 288_000; // Lb = 48000 * 3 * 2
const HEADER_SIZE: usize = 20;
const MAGIC: u32 = 0xFFFF_FFFF;
const SEGMENT_ID: [u8; 4] = [0x18, 0x53, 0x80, 0x67];

// ── Public API ────────────────────────────────────────────────────────────────

/// Convert a video file to an ISDCF Doc 13-compliant WAV PCM file.
pub async fn pack_to_wav(
    input: &Path,
    output: &Path,
    crop: bool,
    app: &AppHandle,
    cancel: &Arc<AtomicBool>,
) -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let encoded_webm = temp_dir.path().join("encoded.webm");
    let chunk_dir = temp_dir.path().join("chunks");
    std::fs::create_dir_all(&chunk_dir)?;

    // ── Stage 1: VP9 encoding ─────────────────────────────────────────────────
    emit_progress(app, "encoding", 0.0, "Iniciando codificação VP9…");

    // Get video duration from a quick probe
    let duration = probe_duration(app, input).await?;

    let vf_filter = if crop {
        // The crop_filter is already computed by analyzer; packager receives crop=true
        // and uses a default landscape crop. For the exact filter the UI passes it
        // via the `crop` flag — we build the conservative safe crop here.
        "crop=ih*3/4:ih,scale=480:640".to_string()
    } else {
        "scale=480:640".to_string()
    };

    let app_clone = app.clone();
    ffmpeg::encode_vp9(
        app,
        input,
        &encoded_webm,
        &vf_filter,
        duration,
        cancel,
        move |pct| {
            emit_progress(&app_clone, "encoding", pct, &format!("Codificando… {:.0}%", pct));
        },
    )
    .await?;

    if cancel.load(Ordering::SeqCst) {
        return Err(anyhow!("Cancelado pelo usuário."));
    }

    // ── Stage 2: Segmentation ─────────────────────────────────────────────────
    emit_progress(app, "packaging", 0.0, "Segmentando em chunks de 2s…");
    let chunks = ffmpeg::segment_webm(app, &encoded_webm, &chunk_dir).await?;
    let total = chunks.len();

    if total == 0 {
        return Err(anyhow!("Nenhum chunk de vídeo foi gerado. O vídeo pode ser muito curto."));
    }

    // ── Stage 3: PCM assembly ─────────────────────────────────────────────────
    let chunks_ref = &chunks;
    let app_ref = app;
    let cancel_ref = cancel;

    let mut chunk_idx: usize = 0;

    wav::write_wav(output, std::iter::from_fn(move || {
        if chunk_idx >= chunks_ref.len() {
            return None;
        }
        if cancel_ref.load(Ordering::SeqCst) {
            return Some(Err(anyhow!("Cancelado pelo usuário.")));
        }

        let path = &chunks_ref[chunk_idx];
        let result = build_pcm_block(path, chunk_idx);

        let pct = (chunk_idx + 1) as f64 / total as f64 * 100.0;
        emit_progress(
            app_ref,
            "packaging",
            pct,
            &format!("Empacotando chunk {}/{}", chunk_idx + 1, total),
        );

        chunk_idx += 1;
        Some(result)
    }))?;

    Ok(())
}

// ── PCM block builder ─────────────────────────────────────────────────────────

/// Read one WebM chunk and pack it into a 288,000-byte PCM block.
fn build_pcm_block(chunk_path: &Path, idx: usize) -> Result<Vec<u8>> {
    let chunk_bytes = std::fs::read(chunk_path)?;

    // Find Segment boundary: the first occurrence of 0x18 0x53 0x80 0x67
    let seg_offset = find_segment_id(&chunk_bytes).ok_or_else(|| {
        anyhow!(
            "Chunk {} não contém o ID de Segment WebM (0x18538067). \
             O arquivo pode estar corrompido.",
            idx
        )
    })?;

    let ebml_header = &chunk_bytes[..seg_offset];
    let vp9_segment = &chunk_bytes[seg_offset..];

    let le = ebml_header.len();
    let lv = vp9_segment.len();

    // Validate that data fits in one block
    if HEADER_SIZE + le + lv > BLOCK_SIZE {
        return Err(anyhow!(
            "Chunk {} é grande demais para um bloco PCM \
             (header {} + EBML {} + Segment {} = {} > {}). \
             O bitrate ficou acima de 1 Mbps.",
            idx,
            HEADER_SIZE,
            le,
            lv,
            HEADER_SIZE + le + lv,
            BLOCK_SIZE
        ));
    }

    let padding_len = BLOCK_SIZE - HEADER_SIZE - le - lv;

    let mut block = Vec::with_capacity(BLOCK_SIZE);

    // 20-byte header (big-endian uint32 fields)
    block.extend_from_slice(&MAGIC.to_be_bytes());           // H1
    block.extend_from_slice(&(lv as u32).to_be_bytes());     // Lv
    block.extend_from_slice(&(BLOCK_SIZE as u32).to_be_bytes()); // Lb
    block.extend_from_slice(&(le as u32).to_be_bytes());     // Le
    block.extend_from_slice(&MAGIC.to_be_bytes());           // H2

    block.extend_from_slice(ebml_header);                    // E
    block.extend_from_slice(vp9_segment);                    // VP9 Segment
    block.extend(std::iter::repeat(0u8).take(padding_len));  // P

    debug_assert_eq!(block.len(), BLOCK_SIZE);
    Ok(block)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn find_segment_id(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == SEGMENT_ID)
}

async fn probe_duration(app: &AppHandle, input: &Path) -> Result<f64> {
    use tauri_plugin_shell::ShellExt;

    let args = vec![
        "-v".to_string(), "quiet".to_string(),
        "-print_format".to_string(), "json".to_string(),
        "-show_format".to_string(),
        input.to_string_lossy().to_string(),
    ];

    let out = app.shell().sidecar("ffprobe")?.args(args).output().await?;
    if !out.status.success() {
        return Ok(0.0); // fall back gracefully; progress won't be accurate
    }

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null);
    Ok(v["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0))
}

fn emit_progress(app: &AppHandle, stage: &str, percent: f64, message: &str) {
    let _ = app.emit(
        "conversion-progress",
        ProgressEvent {
            stage: stage.to_string(),
            percent,
            message: message.to_string(),
        },
    );
}

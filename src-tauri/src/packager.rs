use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;

use tauri::Emitter;

use crate::analyzer::{self, AttributeStatus};
use crate::ffmpeg;
use crate::wav;
use crate::ProgressEvent;

// PCM block constants (ISDCF Doc 13 §3.3)
const BLOCK_SIZE: usize = 288_000; // Lb = 48000 * 3 * 2
const HEADER_SIZE: usize = 20;
const MAGIC: u32 = 0xFFFF_FFFF;
const SEGMENT_ID: [u8; 4] = [0x18, 0x53, 0x80, 0x67];

// Maximum bytes a single 2-second chunk (EBML header + VP9 Segment) may occupy
// so that `HEADER_SIZE + Le + Lv <= BLOCK_SIZE`. Since a chunk file *is*
// `EBML header + Segment`, its on-disk size equals `Le + Lv`.
const MAX_CHUNK_BYTES: u64 = (BLOCK_SIZE - HEADER_SIZE) as u64; // 287_980

// Verify-retry encoding bounds.
const INITIAL_TARGET_KBPS: u32 = 900; // stays within the ISDCF 1 Mbps ceiling
const MIN_TARGET_KBPS: u32 = 300;     // floor before we give up

// ── Public API ────────────────────────────────────────────────────────────────

/// Convert a video file to an ISDCF Doc 13-compliant WAV PCM file.
///
/// Two paths guarantee that every 2-second chunk fits in a 288,000-byte block:
///   1. **Reuse** — if the input is already a fully conforming VP9 (480×640,
///      24fps, yuv420p) and, once segmented, every chunk fits, we package it
///      as-is with no recompression (maximum quality).
///   2. **Encode with verify-retry** — otherwise we re-encode to VP9 with a
///      tight VBV and exact 2s keyframes, segment, and check the largest chunk.
///      If it overflows, we lower the bitrate and re-encode until it fits.
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

    let duration = probe_duration(app, input).await?;
    let expected_chunks = (duration / 2.0).ceil() as usize;

    let info = analyzer::analyze_video(&input.to_string_lossy(), app).await?;

    // ── Path 1: try to reuse an already-conforming VP9 input ──────────────────
    if !crop && is_reuse_eligible(&info) {
        emit_progress(app, "encoding", 100.0, "Vídeo já é VP9 conforme — verificando…");
        reset_dir(&chunk_dir)?;
        let chunks = ffmpeg::segment_webm(app, input, &chunk_dir).await?;
        let max = max_chunk_size(&chunks)?;

        if chunks.len() == expected_chunks && max <= MAX_CHUNK_BYTES {
            emit_progress(app, "encoding", 100.0, "VP9 de entrada aproveitado sem recompressão.");
            return assemble_wav(output, &chunks, app, cancel);
        }
        // Not usable as-is (misaligned keyframes or an oversized chunk) → encode.
    }

    // ── Path 2: encode to VP9 with verify-retry ───────────────────────────────
    let vf_filter = info
        .crop_filter
        .clone()
        .unwrap_or_else(|| "scale=480:640".to_string());

    let mut target_kbps = INITIAL_TARGET_KBPS;

    let chunks = loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(anyhow!("Cancelado pelo usuário."));
        }

        // Stage 1: encoding
        emit_progress(app, "encoding", 0.0, &format!("Iniciando codificação VP9 ({}k)…", target_kbps));
        reset_dir(&chunk_dir)?;

        let app_clone = app.clone();
        let bitrate = target_kbps;
        ffmpeg::encode_vp9(
            app,
            input,
            &encoded_webm,
            &vf_filter,
            target_kbps,
            duration,
            cancel,
            move |pct| {
                emit_progress(
                    &app_clone,
                    "encoding",
                    pct,
                    &format!("Codificando ({}k)… {:.0}%", bitrate, pct),
                );
            },
        )
        .await?;

        if cancel.load(Ordering::SeqCst) {
            return Err(anyhow!("Cancelado pelo usuário."));
        }

        // Stage 2: segmentation + budget check
        emit_progress(app, "packaging", 0.0, "Segmentando em chunks de 2s…");
        let chunks = ffmpeg::segment_webm(app, &encoded_webm, &chunk_dir).await?;
        if chunks.is_empty() {
            return Err(anyhow!("Nenhum chunk de vídeo foi gerado. O vídeo pode ser muito curto."));
        }

        let max = max_chunk_size(&chunks)?;
        if max <= MAX_CHUNK_BYTES {
            break chunks;
        }

        // Overflow → drop the bitrate ~15% and try again.
        let next = target_kbps * 85 / 100;
        if next < MIN_TARGET_KBPS {
            return Err(anyhow!(
                "Mesmo reduzindo o bitrate ao mínimo, um trecho de 2s do vídeo não coube \
                 no bloco PCM (maior chunk: {} bytes, limite: {} bytes). O vídeo tem \
                 movimento ou detalhe demais. Tente reduzir a resolução ou simplificar o fundo.",
                max, MAX_CHUNK_BYTES
            ));
        }

        emit_progress(
            app,
            "packaging",
            0.0,
            &format!(
                "Um chunk excedeu {} bytes (limite {}). Recodificando a {}k…",
                max, MAX_CHUNK_BYTES, next
            ),
        );
        target_kbps = next;
    };

    // ── Stage 3: PCM assembly ─────────────────────────────────────────────────
    assemble_wav(output, &chunks, app, cancel)
}

// ── WAV assembly ────────────────────────────────────────────────────────────

/// Build the WAV by streaming one 288,000-byte PCM block per chunk.
fn assemble_wav(
    output: &Path,
    chunks: &[PathBuf],
    app: &AppHandle,
    cancel: &Arc<AtomicBool>,
) -> Result<()> {
    let total = chunks.len();
    let mut chunk_idx: usize = 0;

    wav::write_wav(output, std::iter::from_fn(|| {
        if chunk_idx >= chunks.len() {
            return None;
        }
        if cancel.load(Ordering::SeqCst) {
            return Some(Err(anyhow!("Cancelado pelo usuário.")));
        }

        let path = &chunks[chunk_idx];
        let result = build_pcm_block(path, chunk_idx);

        let pct = (chunk_idx + 1) as f64 / total as f64 * 100.0;
        emit_progress(
            app,
            "packaging",
            pct,
            &format!("Empacotando chunk {}/{}", chunk_idx + 1, total),
        );

        chunk_idx += 1;
        Some(result)
    }))?;

    Ok(())
}

// ── Reuse eligibility & chunk sizing ────────────────────────────────────────

/// True when the input is already a fully conforming VP9 that we may package
/// without re-encoding (subject to the per-chunk size test done by the caller).
fn is_reuse_eligible(info: &analyzer::VideoInfo) -> bool {
    matches!(info.codec.status, AttributeStatus::Conformant)
        && matches!(info.resolution.status, AttributeStatus::Conformant)
        && matches!(info.frame_rate.status, AttributeStatus::Conformant)
        && matches!(info.pixel_format.status, AttributeStatus::Conformant)
        && !info.needs_crop
}

/// Largest chunk file size in bytes.
fn max_chunk_size(chunks: &[PathBuf]) -> Result<u64> {
    let mut max = 0u64;
    for p in chunks {
        let len = std::fs::metadata(p)?.len();
        if len > max {
            max = len;
        }
    }
    Ok(max)
}

/// Empty and recreate a directory so stale chunk files never leak between
/// encoding attempts.
fn reset_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
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

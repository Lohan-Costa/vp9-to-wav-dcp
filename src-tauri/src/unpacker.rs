use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

use crate::ffmpeg;
use crate::wav;

// ISDCF Doc 13 constants
const BLOCK_SIZE: usize = 288_000;
const HEADER_SIZE: usize = 20;
const MAGIC: u32 = 0xFFFF_FFFF;
const SEGMENT_ID: [u8; 4] = [0x18, 0x53, 0x80, 0x67];

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CheckItem {
    pub label:    String,
    pub ok:       bool,
    pub detail:   String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ValidationResult {
    pub checks:       Vec<CheckItem>,
    pub total_chunks: usize,
    pub good_chunks:  usize,
    pub duration_secs: f64,
    pub video_path:   Option<PathBuf>,
    pub video_info:   Option<String>, // "480×640 · 24 fps · VP9 · Xs"
    pub all_ok:       bool,
}

// ── Public API ────────────────────────────────────────────────────────────────

pub async fn unpack_wav(
    wav_path: &Path,
    work_dir: &Path,
    app: &AppHandle,
) -> Result<ValidationResult> {
    let mut checks: Vec<CheckItem> = Vec::new();

    // ── Check 1: WAV header ───────────────────────────────────────────────────
    let wav_meta = match wav::read_wav_header(wav_path) {
        Ok(m) => {
            checks.push(ok(
                "Cabeçalho WAV",
                &format!("{} Hz, {}-bit, {} canal(is), PCM", m.sample_rate, m.bits_per_sample, m.channels),
            ));
            m
        }
        Err(e) => {
            checks.push(fail("Cabeçalho WAV", &e.to_string()));
            return Ok(ValidationResult {
                checks,
                total_chunks: 0,
                good_chunks: 0,
                duration_secs: 0.0,
                video_path: None,
                video_info: None,
                all_ok: false,
            });
        }
    };

    // ── Check 2: WAV format (48kHz / 24-bit / mono) ───────────────────────────
    if let Some(msg) = wav::validate_wav_format(&wav_meta) {
        checks.push(fail("Formato WAV", &msg));
        return Ok(ValidationResult {
            checks,
            total_chunks: 0,
            good_chunks: 0,
            duration_secs: 0.0,
            video_path: None,
            video_info: None,
            all_ok: false,
        });
    } else {
        checks.push(ok(
            "Formato WAV",
            "48.000 Hz, 24-bit, mono — conforme ISDCF Doc 13",
        ));
    }

    // ── Load data section ─────────────────────────────────────────────────────
    let (_, data) = wav::read_wav_data(wav_path)?;

    let expected_blocks = data.len() / BLOCK_SIZE;
    let remainder = data.len() % BLOCK_SIZE;

    // ── Check 3: Block structure ──────────────────────────────────────────────
    if expected_blocks == 0 {
        checks.push(fail(
            "Estrutura de blocos",
            "Nenhum bloco de 288.000 bytes encontrado. \
             Este WAV não parece conter vídeo encapsulado no formato ISDCF Doc 13.",
        ));
        return Ok(no_video(checks));
    }

    if remainder == 0 {
        checks.push(ok(
            "Estrutura de blocos",
            &format!("{} bloco(s) completo(s) de 288.000 bytes", expected_blocks),
        ));
    } else {
        checks.push(fail(
            "Estrutura de blocos",
            &format!(
                "Encontrado(s) {} bloco(s) completo(s), mas o último fragmento tem {} bytes em vez de 288.000.",
                expected_blocks, remainder
            ),
        ));
    }

    // ── Parse each block ──────────────────────────────────────────────────────
    let mut magic_ok = true;
    let mut lb_ok = true;
    let mut ebml_ok = true;
    let mut seg_ok = true;
    let mut size_ok = true;

    let mut good_chunks: usize = 0;
    let chunk_dir = work_dir.join("chunks");
    std::fs::create_dir_all(&chunk_dir)?;
    let mut chunk_paths: Vec<PathBuf> = Vec::new();

    for i in 0..expected_blocks {
        let start = i * BLOCK_SIZE;
        let block = &data[start..start + BLOCK_SIZE];

        // H1
        let h1 = u32::from_be_bytes(block[0..4].try_into().unwrap());
        if h1 != MAGIC {
            magic_ok = false;
            continue;
        }

        // Lv, Lb, Le
        let lv = u32::from_be_bytes(block[4..8].try_into().unwrap()) as usize;
        let lb = u32::from_be_bytes(block[8..12].try_into().unwrap()) as usize;
        let le = u32::from_be_bytes(block[12..16].try_into().unwrap()) as usize;

        // H2
        let h2 = u32::from_be_bytes(block[16..20].try_into().unwrap());
        if h2 != MAGIC {
            magic_ok = false;
            continue;
        }

        if lb != BLOCK_SIZE {
            lb_ok = false;
        }

        // Bounds check
        if HEADER_SIZE + le + lv > BLOCK_SIZE {
            size_ok = false;
            continue;
        }

        let ebml_bytes = &block[HEADER_SIZE..HEADER_SIZE + le];
        let seg_bytes  = &block[HEADER_SIZE + le..HEADER_SIZE + le + lv];

        // EBML header must start with 0x1A 0x45 0xDF 0xA3
        if le < 4 || ebml_bytes[..4] != [0x1A, 0x45, 0xDF, 0xA3] {
            ebml_ok = false;
        }

        // VP9 Segment must start with 0x18 0x53 0x80 0x67
        if lv < 4 || seg_bytes[..4] != SEGMENT_ID {
            seg_ok = false;
            continue;
        }

        // Write chunk as mini-WebM (EBML Header + VP9 Segment)
        let chunk_path = chunk_dir.join(format!("chunk_{:05}.webm", i));
        let mut mini = Vec::with_capacity(le + lv);
        mini.extend_from_slice(ebml_bytes);
        mini.extend_from_slice(seg_bytes);
        std::fs::write(&chunk_path, &mini)?;
        chunk_paths.push(chunk_path);
        good_chunks += 1;
    }

    // ── Checks 4–8 ────────────────────────────────────────────────────────────
    checks.push(if magic_ok {
        ok("Magic numbers", "0xFFFFFFFF encontrado em todos os blocos")
    } else {
        fail("Magic numbers", "Não foi possível encontrar os marcadores 0xFFFFFFFF em todos os blocos. Este WAV pode não conter vídeo encapsulado.")
    });

    checks.push(if lb_ok {
        ok("Consistência de Lb", "Todos os blocos declaram Lb = 288.000")
    } else {
        fail("Consistência de Lb", "Alguns blocos declaram um valor de Lb diferente de 288.000.")
    });

    checks.push(if ebml_ok {
        ok("EBML Headers", "Cabeçalhos EBML válidos em todos os chunks")
    } else {
        fail("EBML Headers", "O cabeçalho EBML não foi encontrado ou está corrompido em um ou mais chunks.")
    });

    checks.push(if seg_ok {
        ok("VP9 Segments", &format!("ID de Segment (0x18538067) presente em {} chunk(s)", good_chunks))
    } else {
        fail("VP9 Segments", "O ID de Segment 0x18538067 não foi encontrado em um ou mais chunks. Os dados VP9 podem estar corrompidos.")
    });

    checks.push(if size_ok {
        ok("Tamanhos consistentes", "HEADER_SIZE + Le + Lv ≤ Lb em todos os blocos")
    } else {
        fail("Tamanhos consistentes", "Em um ou mais blocos, Lv + Le + 20 excede Lb = 288.000. O empacotamento original pode ter tido bitrate excessivo.")
    });

    // ── Reconstruct video ─────────────────────────────────────────────────────
    let duration_secs = good_chunks as f64 * 2.0;
    checks.push(ok(
        "Duração reconstruída",
        &format!("{:.1} s ({} chunk(s) × 2 s)", duration_secs, good_chunks),
    ));

    if good_chunks == 0 {
        checks.push(fail("Reconstrução", "Nenhum chunk válido encontrado para reconstruir o vídeo."));
        return Ok(no_video(checks));
    }

    let output_path = work_dir.join("recovered.webm");
    let video_path = match ffmpeg::concat_webm(app, &chunk_paths, &output_path).await {
        Ok(()) => {
            checks.push(ok("Reconstrução", &format!("Vídeo reconstruído com {} chunk(s)", good_chunks)));
            Some(output_path)
        }
        Err(e) => {
            checks.push(fail("Reconstrução", &format!("Falha ao concatenar chunks: {}", e)));
            None
        }
    };

    let all_ok = checks.iter().all(|c| c.ok);
    let video_info = video_path.as_ref().map(|_| {
        format!("480×640 · 24 fps · VP9 · {} frames · duração {:.1} s",
            good_chunks * 48,
            duration_secs)
    });

    Ok(ValidationResult {
        checks,
        total_chunks: expected_blocks,
        good_chunks,
        duration_secs,
        video_path,
        video_info,
        all_ok,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ok(label: &str, detail: &str) -> CheckItem {
    CheckItem { label: label.to_string(), ok: true, detail: detail.to_string() }
}

fn fail(label: &str, detail: &str) -> CheckItem {
    CheckItem { label: label.to_string(), ok: false, detail: detail.to_string() }
}

fn no_video(checks: Vec<CheckItem>) -> ValidationResult {
    ValidationResult {
        checks,
        total_chunks: 0,
        good_chunks: 0,
        duration_secs: 0.0,
        video_path: None,
        video_info: None,
        all_ok: false,
    }
}

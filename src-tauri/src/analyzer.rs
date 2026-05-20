use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AttributeStatus {
    Conformant,   // ✓ já está conforme
    WillAdjust,   // ↻ será ajustado silenciosamente
    NeedsConfirm, // ⚠ requer confirmação do usuário
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Attribute {
    pub current: String,
    pub target: Option<String>,
    pub status: AttributeStatus,
    pub label: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VideoInfo {
    pub codec:        Attribute,
    pub frame_rate:   Attribute,
    pub bitrate:      Attribute,
    pub pixel_format: Attribute,
    pub resolution:   Attribute,
    pub duration_secs: f64,
    pub has_video:    bool,
    pub needs_crop:   bool,
    pub crop_description: Option<String>,
    // Computed crop filter string used by FFmpeg
    pub crop_filter:  Option<String>,
}

// ── ffprobe JSON types ─────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format:  FfprobeFormat,
}

#[derive(Deserialize, Debug)]
struct FfprobeStream {
    codec_type:   Option<String>,
    codec_name:   Option<String>,
    width:        Option<u32>,
    height:       Option<u32>,
    r_frame_rate: Option<String>,
    pix_fmt:      Option<String>,
    bit_rate:     Option<String>,
}

#[derive(Deserialize, Debug)]
struct FfprobeFormat {
    duration: Option<String>,
    bit_rate: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────────

pub async fn analyze_video(path: &str, app: &AppHandle) -> Result<VideoInfo> {
    let args = vec![
        "-v".to_string(), "quiet".to_string(),
        "-print_format".to_string(), "json".to_string(),
        "-show_streams".to_string(),
        "-show_format".to_string(),
        path.to_string(),
    ];

    let output = app
        .shell()
        .sidecar("ffprobe")?
        .args(args)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "Não foi possível ler o vídeo. O arquivo pode estar corrompido ou em um formato não suportado. Detalhes: {}",
            stderr.trim()
        ));
    }

    let probe: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow!("Erro ao interpretar metadados do vídeo: {}", e))?;

    let video_stream = probe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| anyhow!("O arquivo não contém nenhuma trilha de vídeo."))?;

    let duration = probe
        .format
        .duration
        .as_deref()
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(0.0);

    if duration < 2.0 {
        return Err(anyhow!(
            "O vídeo é muito curto (menos de 2 segundos). É necessário pelo menos um chunk de 2s."
        ));
    }

    build_video_info(video_stream, &probe.format, duration)
}

// ── Internal builders ─────────────────────────────────────────────────────────

fn build_video_info(
    stream: &FfprobeStream,
    format: &FfprobeFormat,
    duration: f64,
) -> Result<VideoInfo> {
    // Codec
    let codec_name = stream.codec_name.as_deref().unwrap_or("desconhecido");
    let codec = if codec_name == "vp9" {
        Attribute {
            current: "VP9".into(),
            target:  None,
            status:  AttributeStatus::Conformant,
            label:   "Codec".into(),
        }
    } else {
        Attribute {
            current: codec_name.to_uppercase(),
            target:  Some("VP9".into()),
            status:  AttributeStatus::WillAdjust,
            label:   "Codec".into(),
        }
    };

    // Frame rate
    let fps = parse_rational(stream.r_frame_rate.as_deref().unwrap_or("0/1"));
    let fps_display = format!("{:.3} fps", fps);
    let frame_rate = if (fps - 24.0).abs() < 0.01 {
        Attribute {
            current: fps_display,
            target:  None,
            status:  AttributeStatus::Conformant,
            label:   "Frame rate".into(),
        }
    } else {
        Attribute {
            current: fps_display,
            target:  Some("24.0 fps".into()),
            status:  AttributeStatus::WillAdjust,
            label:   "Frame rate".into(),
        }
    };

    // Bitrate (use stream bitrate first, fall back to format bitrate)
    let bitrate_bps = stream
        .bit_rate
        .as_deref()
        .and_then(|b| b.parse::<u64>().ok())
        .or_else(|| {
            format
                .bit_rate
                .as_deref()
                .and_then(|b| b.parse::<u64>().ok())
        })
        .unwrap_or(0);
    let bitrate_mbps = bitrate_bps as f64 / 1_000_000.0;
    let bitrate = if bitrate_mbps <= 1.0 {
        Attribute {
            current: format!("{:.1} Mbps", bitrate_mbps),
            target:  None,
            status:  AttributeStatus::Conformant,
            label:   "Bitrate".into(),
        }
    } else {
        Attribute {
            current: format!("{:.1} Mbps", bitrate_mbps),
            target:  Some("1.0 Mbps".into()),
            status:  AttributeStatus::WillAdjust,
            label:   "Bitrate".into(),
        }
    };

    // Pixel format
    let pix = stream.pix_fmt.as_deref().unwrap_or("desconhecido");
    let pixel_format = if pix == "yuv420p" {
        Attribute {
            current: "yuv420p".into(),
            target:  None,
            status:  AttributeStatus::Conformant,
            label:   "Pixel format".into(),
        }
    } else {
        Attribute {
            current: pix.to_string(),
            target:  Some("yuv420p".into()),
            status:  AttributeStatus::WillAdjust,
            label:   "Pixel format".into(),
        }
    };

    // Resolution
    let w = stream.width.unwrap_or(0);
    let h = stream.height.unwrap_or(0);
    let (resolution, needs_crop, crop_description, crop_filter) =
        build_resolution_attribute(w, h);

    Ok(VideoInfo {
        codec,
        frame_rate,
        bitrate,
        pixel_format,
        resolution,
        duration_secs: duration,
        has_video: true,
        needs_crop,
        crop_description,
        crop_filter,
    })
}

fn build_resolution_attribute(
    w: u32,
    h: u32,
) -> (Attribute, bool, Option<String>, Option<String>) {
    let current = format!("{}×{}", w, h);

    if w == 480 && h == 640 {
        return (
            Attribute {
                current,
                target: None,
                status: AttributeStatus::Conformant,
                label: "Resolução".into(),
            },
            false,
            None,
            None,
        );
    }

    // Check if proportion is already 3:4 (portrait 480×640 ratio)
    let is_portrait_34 = h > 0 && w > 0 && {
        let ratio = w as f64 / h as f64;
        (ratio - 3.0 / 4.0).abs() < 0.02
    };

    if is_portrait_34 {
        // Same proportion but wrong size → just scale
        return (
            Attribute {
                current,
                target: Some("480×640".into()),
                status: AttributeStatus::WillAdjust,
                label: "Resolução".into(),
            },
            false,
            None,
            Some("scale=480:640".to_string()),
        );
    }

    // Different proportion → needs crop
    let (crop_filter, description) = if w > h {
        // Landscape → crop width to get 3:4
        (
            "crop=ih*3/4:ih,scale=480:640".to_string(),
            format!(
                "O vídeo é no formato paisagem ({}×{}). Será feito um crop centralizado para a proporção 3:4 (portrait), mantendo a altura completa e cortando as laterais.",
                w, h
            ),
        )
    } else {
        // Portrait but wrong ratio → crop height
        (
            "crop=iw:iw*4/3,scale=480:640".to_string(),
            format!(
                "O vídeo é portrait ({}×{}) mas não está na proporção 3:4. Será feito um crop centralizado cortando topo e base para encaixar em 480×640.",
                w, h
            ),
        )
    };

    (
        Attribute {
            current,
            target: Some("480×640 (com crop)".into()),
            status: AttributeStatus::NeedsConfirm,
            label: "Resolução".into(),
        },
        true,
        Some(description),
        Some(crop_filter),
    )
}

fn parse_rational(r: &str) -> f64 {
    let parts: Vec<&str> = r.split('/').collect();
    if parts.len() == 2 {
        let num: f64 = parts[0].parse().unwrap_or(0.0);
        let den: f64 = parts[1].parse().unwrap_or(1.0);
        if den != 0.0 {
            return num / den;
        }
    }
    r.parse().unwrap_or(0.0)
}

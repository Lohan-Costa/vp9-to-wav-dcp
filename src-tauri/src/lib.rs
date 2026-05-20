mod analyzer;
mod ffmpeg;
mod packager;
mod unpacker;
mod wav;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::Emitter;

// ── Shared application state ──────────────────────────────────────────────────

pub struct ConversionArtifact {
    _temp_dir: tempfile::TempDir,
    pub wav_path: PathBuf,
}

pub struct ValidationArtifact {
    _temp_dir: tempfile::TempDir,
    pub video_path: PathBuf,
}

pub struct AppState {
    pub cancel_flag: Arc<AtomicBool>,
    pub log_buffer: Arc<Mutex<Vec<String>>>,
    pub conversion: Arc<Mutex<Option<ConversionArtifact>>>,
    pub validation: Arc<Mutex<Option<ValidationArtifact>>>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            cancel_flag: Arc::new(AtomicBool::new(false)),
            log_buffer: Arc::new(Mutex::new(Vec::new())),
            conversion: Arc::new(Mutex::new(None)),
            validation: Arc::new(Mutex::new(None)),
        }
    }
}

// ── Progress / log event payloads ─────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct ProgressEvent {
    pub stage: String,
    pub percent: f64,
    pub message: String,
}

#[derive(Serialize, Clone)]
pub struct LogEvent {
    pub level: String,
    pub message: String,
}

// ── Helper: append to in-memory log and emit to UI ───────────────────────────

fn log_info(app: &tauri::AppHandle, state: &AppState, msg: &str) {
    let entry = format!("[INFO] {}", msg);
    state.log_buffer.lock().unwrap().push(entry.clone());
    let _ = app.emit("log-line", LogEvent { level: "info".into(), message: msg.to_string() });
    log::info!("{}", msg);
}

#[allow(dead_code)]
fn log_warn(app: &tauri::AppHandle, state: &AppState, msg: &str) {
    let entry = format!("[WARN] {}", msg);
    state.log_buffer.lock().unwrap().push(entry.clone());
    let _ = app.emit("log-line", LogEvent { level: "warn".into(), message: msg.to_string() });
    log::warn!("{}", msg);
}

fn log_error(app: &tauri::AppHandle, state: &AppState, msg: &str) {
    let entry = format!("[ERRO] {}", msg);
    state.log_buffer.lock().unwrap().push(entry.clone());
    let _ = app.emit("log-line", LogEvent { level: "error".into(), message: msg.to_string() });
    log::error!("{}", msg);
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
async fn analyze_video(
    path: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<analyzer::VideoInfo, String> {
    log_info(&app, &state, &format!("Analisando arquivo: {}", path));
    analyzer::analyze_video(&path, &app)
        .await
        .map_err(|e| {
            log_error(&app, &state, &format!("Análise falhou: {}", e));
            e.to_string()
        })
}

#[tauri::command]
async fn start_conversion(
    path: String,
    crop: bool,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.cancel_flag.store(false, Ordering::SeqCst);
    // Clear previous conversion artifact (drops old temp dir)
    *state.conversion.lock().unwrap() = None;

    log_info(&app, &state, &format!("Iniciando conversão: {} (crop={})", path, crop));

    let temp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let wav_path = temp_dir.path().join("output.wav");

    let app_clone = app.clone();
    let cancel = Arc::clone(&state.cancel_flag);

    packager::pack_to_wav(
        std::path::Path::new(&path),
        &wav_path,
        crop,
        &app_clone,
        &cancel,
    )
    .await
    .map_err(|e| {
        log_error(&app, &state, &format!("Conversão falhou: {}", e));
        e.to_string()
    })?;

    *state.conversion.lock().unwrap() = Some(ConversionArtifact {
        _temp_dir: temp_dir,
        wav_path,
    });

    log_info(&app, &state, "Conversão concluída com sucesso.");
    Ok(())
}

#[tauri::command]
async fn cancel_conversion(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.cancel_flag.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
async fn save_wav(
    dest_path: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let guard = state.conversion.lock().unwrap();
    let artifact = guard
        .as_ref()
        .ok_or("Nenhum WAV disponível para salvar")?;

    std::fs::copy(&artifact.wav_path, &dest_path)
        .map_err(|e| format!("Erro ao salvar o arquivo: {}", e))?;

    log_info(&app, &state, &format!("WAV salvo em: {}", dest_path));
    Ok(())
}

#[tauri::command]
async fn start_validation(
    path: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<unpacker::ValidationResult, String> {
    *state.validation.lock().unwrap() = None;

    log_info(&app, &state, &format!("Iniciando validação: {}", path));

    let temp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;

    let result = unpacker::unpack_wav(
        std::path::Path::new(&path),
        temp_dir.path(),
        &app,
    )
    .await
    .map_err(|e| {
        log_error(&app, &state, &format!("Validação falhou: {}", e));
        e.to_string()
    })?;

    if let Some(ref vp) = result.video_path {
        *state.validation.lock().unwrap() = Some(ValidationArtifact {
            _temp_dir: temp_dir,
            video_path: vp.clone(),
        });
    }

    log_info(&app, &state, "Validação concluída.");
    Ok(result)
}

#[tauri::command]
async fn get_video_temp_path(
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let guard = state.validation.lock().unwrap();
    guard
        .as_ref()
        .map(|a| a.video_path.to_string_lossy().to_string())
        .ok_or_else(|| "Nenhum vídeo recuperado disponível".to_string())
}

#[tauri::command]
async fn save_recovered_video(
    dest_path: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let guard = state.validation.lock().unwrap();
    let artifact = guard
        .as_ref()
        .ok_or("Nenhum vídeo disponível para salvar")?;

    std::fs::copy(&artifact.video_path, &dest_path)
        .map_err(|e| format!("Erro ao salvar o vídeo: {}", e))?;

    log_info(&app, &state, &format!("Vídeo salvo em: {}", dest_path));
    Ok(())
}

#[tauri::command]
async fn get_log(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let buf = state.log_buffer.lock().unwrap();
    Ok(buf.join("\n"))
}

#[tauri::command]
async fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(&url).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd").args(["/c", "start", &url]).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            analyze_video,
            start_conversion,
            cancel_conversion,
            save_wav,
            start_validation,
            get_video_temp_path,
            save_recovered_video,
            get_log,
            open_url,
        ])
        .run(tauri::generate_context!())
        .expect("Erro ao iniciar o app");
}

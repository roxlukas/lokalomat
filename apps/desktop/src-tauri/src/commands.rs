//! IPC commands. Every command logs its call at DEBUG, so verbose mode shows
//! the full IPC traffic (with secrets redacted).

use std::path::PathBuf;

use lokalsi_core::diagnostics::{self, ExportSummary};
use lokalsi_core::logging::{self, LogMode};
use lokalsi_core::redact::redact;
use lokalsi_core::settings::{self, AppSettings};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;

type CmdResult<T> = Result<T, String>;

const MAX_UI_MESSAGE: usize = 4000;
const MAX_UI_CONTEXT: usize = 1000;
const MAX_TAIL_LINES: usize = 2000;

#[derive(Serialize)]
pub struct SettingsView {
    settings: AppSettings,
    effective_log_mode: LogMode,
    log_mode_overridden: bool,
}

pub fn settings_view(state: &AppState) -> SettingsView {
    SettingsView {
        settings: state.settings().clone(),
        effective_log_mode: state.log.mode(),
        log_mode_overridden: state.log_mode_overridden,
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> SettingsView {
    tracing::debug!(command = "get_settings");
    settings_view(&state)
}

#[tauri::command]
pub fn update_settings(
    settings: AppSettings,
    state: State<'_, AppState>,
) -> CmdResult<SettingsView> {
    tracing::debug!(command = "update_settings", ?settings);
    settings::save(&state.settings_path, &settings).map_err(|e| {
        tracing::error!(error = %e, path = %state.settings_path.display(), "cannot save settings");
        e.to_string()
    })?;
    if state.log_mode_overridden {
        tracing::debug!("log mode is forced for this run; saved value applies after restart");
    } else {
        state.log.set_mode(settings.log_mode).map_err(|e| {
            tracing::error!(error = %e, "cannot change log mode");
            e.to_string()
        })?;
    }
    logging::set_log_prompts(settings.log_prompts);
    state.scheduler.set_keep_llm(settings.keep_llm_loaded);
    *state.settings() = settings;
    crate::models::apply_download_settings(&state);
    crate::models::apply_hardware(&state);
    Ok(settings_view(&state))
}

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    os: &'static str,
    arch: &'static str,
    cpu_logical_cores: usize,
    home: PathBuf,
    log_dir: PathBuf,
}

fn app_info(state: &AppState) -> AppInfo {
    let sys = lokalsi_hwprobe::basic_info();
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        os: sys.os,
        arch: sys.arch,
        cpu_logical_cores: sys.cpu_logical_cores,
        home: state.home.clone(),
        log_dir: state.log.dir().to_path_buf(),
    }
}

#[tauri::command]
pub fn get_edition(state: State<'_, AppState>) -> lokalsi_core::edition::Edition {
    state.ext.edition()
}

#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> AppInfo {
    tracing::debug!(command = "get_app_info");
    app_info(&state)
}

fn truncate(mut s: String, max_chars: usize) -> String {
    if let Some((idx, _)) = s.char_indices().nth(max_chars) {
        s.truncate(idx);
        s.push('…');
    }
    s
}

/// Forwards frontend errors and diagnostics into the application log.
#[tauri::command]
pub fn log_from_ui(level: String, message: String, context: Option<String>) {
    let message = truncate(redact(&message), MAX_UI_MESSAGE);
    let context = truncate(redact(context.as_deref().unwrap_or("")), MAX_UI_CONTEXT);
    match level.as_str() {
        "error" => tracing::error!(target: "ui", context = %context, "{message}"),
        "warn" => tracing::warn!(target: "ui", context = %context, "{message}"),
        "info" => tracing::info!(target: "ui", context = %context, "{message}"),
        _ => tracing::debug!(target: "ui", context = %context, "{message}"),
    }
}

#[tauri::command]
pub fn get_log_tail(lines: usize, state: State<'_, AppState>) -> CmdResult<String> {
    tracing::trace!(command = "get_log_tail", lines);
    logging::tail(state.log.dir(), lines.clamp(1, MAX_TAIL_LINES)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_log_dir(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let dir = state.log.dir().to_string_lossy().into_owned();
    tracing::debug!(command = "open_log_dir", %dir);
    app.opener().open_path(dir, None::<&str>).map_err(|e| {
        tracing::warn!(error = %e, "cannot open log directory");
        e.to_string()
    })
}

#[tauri::command]
pub async fn export_diagnostics(
    dest: String,
    state: State<'_, AppState>,
) -> CmdResult<ExportSummary> {
    tracing::debug!(command = "export_diagnostics", %dest);
    let system = serde_json::to_string_pretty(&app_info(&state)).map_err(|e| e.to_string())?;
    let settings =
        serde_json::to_string_pretty(&settings_view(&state)).map_err(|e| e.to_string())?;
    let out = PathBuf::from(&dest);
    let extra = [("system.json", system), ("settings.json", settings)];
    match diagnostics::export_bundle(state.log.dir(), &extra, &out) {
        Ok(summary) => {
            tracing::info!(path = %summary.path.display(), files = summary.files, bytes = summary.bytes, "diagnostics bundle exported");
            Ok(summary)
        }
        Err(e) => {
            tracing::error!(error = %e, path = %out.display(), "diagnostics export failed");
            Err(e.to_string())
        }
    }
}

//! IPC for M1: hardware, data folder, model library, downloads, HF token.

use std::path::{Path, PathBuf};

use lokalsi_core::benchmark::{Benchmark, BenchmarkMeta};
use lokalsi_core::downloads::{DownloadError, DownloadItem, EnqueueOutcome};
use lokalsi_core::library::{self, Library, LibraryError, ModelView, StarterPick};
use lokalsi_core::settings::{self, Language};
use lokalsi_core::{paths, secrets};
use lokalsi_hwprobe::{HardwareProfile, MemoryBudget};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;
use crate::commands::SettingsView;
use crate::events::{BENCHMARK_UPDATED, HARDWARE_UPDATED};

/// Structured error for the UI: `code` selects the translated message.
#[derive(Debug, Serialize)]
pub struct CmdError {
    code: std::borrow::Cow<'static, str>,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    needed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available: Option<u64>,
}

impl CmdError {
    pub fn new(
        code: impl Into<std::borrow::Cow<'static, str>>,
        message: impl Into<String>,
    ) -> Self {
        CmdError {
            code: code.into(),
            message: message.into(),
            needed: None,
            available: None,
        }
    }
}

impl CmdError {
    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<lokalsi_engines::modes::ModeError> for CmdError {
    fn from(e: lokalsi_engines::modes::ModeError) -> Self {
        CmdError::new(e.code, e.detail)
    }
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<DownloadError> for CmdError {
    fn from(e: DownloadError) -> Self {
        match e {
            DownloadError::InsufficientSpace { needed, available } => CmdError {
                code: "insufficient_space".into(),
                message: e.to_string(),
                needed: Some(needed),
                available: Some(available),
            },
            DownloadError::NotFound(_) => CmdError::new("not_found", e.to_string()),
            other => CmdError::new("error", other.to_string()),
        }
    }
}

impl From<LibraryError> for CmdError {
    fn from(e: LibraryError) -> Self {
        match e {
            LibraryError::Download(d) => d.into(),
            LibraryError::NoDataDir => CmdError::new("no_data_dir", e.to_string()),
            LibraryError::NotGguf(_) => CmdError::new("not_gguf", e.to_string()),
            LibraryError::Hf(ref h) => match h {
                lokalsi_core::hf::HfError::NotFound { .. }
                | lokalsi_core::hf::HfError::Status { status: 404, .. } => {
                    CmdError::new("hf_not_found", e.to_string())
                }
                lokalsi_core::hf::HfError::Status {
                    status: 401 | 403, ..
                } => CmdError::new("hf_auth", e.to_string()),
                lokalsi_core::hf::HfError::InvalidName => {
                    CmdError::new("hf_invalid", e.to_string())
                }
                _ => CmdError::new("network", e.to_string()),
            },
            other => CmdError::new("error", other.to_string()),
        }
    }
}

type Res<T> = Result<T, CmdError>;

fn log_err<E: std::fmt::Display>(command: &'static str) -> impl Fn(E) -> E {
    move |e| {
        tracing::warn!(command, error = %e, "command failed");
        e
    }
}

// --- hardware ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct HardwareView {
    profile: Option<HardwareProfile>,
    budget: Option<MemoryBudget>,
}

/// Hands the hardware profile and memory budget to the engines and the scheduler.
pub fn apply_hardware(state: &AppState) {
    let profile = state.hardware();
    state.llm.set_profile(profile.clone());
    state.ext.on_hardware(profile);
    state
        .scheduler
        .set_budget(budget_of(state).map(|b| b.main_mb));
}

pub fn budget_of(state: &AppState) -> Option<MemoryBudget> {
    let percent = state.settings().unified_memory_percent;
    state
        .hardware()
        .map(|p| lokalsi_hwprobe::budget(&p, percent))
}

fn hardware_view(state: &AppState) -> HardwareView {
    HardwareView {
        profile: state.hardware(),
        budget: budget_of(state),
    }
}

/// Probes the hardware off the main thread, stores it and notifies the UI.
/// Without `force`, a profile probed meanwhile by another caller is reused.
pub async fn probe_hardware(app: &AppHandle, force: bool) -> Result<HardwareView, String> {
    let state = app.state::<AppState>();
    let _guard = state.probe_lock.lock().await;
    if !force && state.hardware().is_some() {
        return Ok(hardware_view(&state));
    }
    let profile = tauri::async_runtime::spawn_blocking(lokalsi_hwprobe::probe)
        .await
        .map_err(|e| e.to_string())?;
    if let Err(e) = state.db.save_hardware(&profile) {
        tracing::warn!(error = %e, "cannot save the hardware profile");
    }
    *state.hardware.lock().unwrap_or_else(|p| p.into_inner()) = Some(profile);
    apply_hardware(&state);
    let view = hardware_view(&state);
    let _ = app.emit(HARDWARE_UPDATED, view.clone());
    Ok(view)
}

#[tauri::command]
pub async fn get_hardware(refresh: bool, app: AppHandle) -> Res<HardwareView> {
    tracing::debug!(command = "get_hardware", refresh);
    probe_hardware(&app, refresh)
        .await
        .map_err(|e| CmdError::new("error", e))
}

// --- data folder --------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct DiskOption {
    name: String,
    mount_point: PathBuf,
    total_bytes: u64,
    available_bytes: u64,
    kind: String,
    removable: bool,
    suggested_dir: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct StorageOptions {
    current: Option<PathBuf>,
    suggested: PathBuf,
    disks: Vec<DiskOption>,
}

#[tauri::command]
pub async fn get_storage_options(state: State<'_, AppState>) -> Res<StorageOptions> {
    tracing::debug!(command = "get_storage_options");
    let profile = match state.hardware() {
        Some(p) => p,
        None => tauri::async_runtime::spawn_blocking(lokalsi_hwprobe::probe)
            .await
            .map_err(|e| CmdError::new("error", e.to_string()))?,
    };
    let disks = profile
        .disks
        .iter()
        .map(|d| DiskOption {
            name: d.name.clone(),
            mount_point: d.mount_point.clone(),
            total_bytes: d.total_bytes,
            available_bytes: d.available_bytes,
            kind: d.kind.clone(),
            removable: d.removable,
            suggested_dir: paths::suggested_data_dir(&state.home, Some(&d.mount_point)),
        })
        .collect();
    Ok(StorageOptions {
        current: state.settings().data_dir.clone(),
        suggested: paths::suggested_data_dir(&state.home, None),
        disks,
    })
}

/// Creates the folder, checks it is writable and saves it in settings.
#[tauri::command]
pub fn set_data_dir(path: String, app: AppHandle, state: State<'_, AppState>) -> Res<SettingsView> {
    tracing::debug!(command = "set_data_dir", %path);
    let dir = PathBuf::from(path.trim());
    if !dir.is_absolute() {
        return Err(CmdError::new(
            "invalid_path",
            "the folder must be an absolute path",
        ));
    }
    let probe = dir.join(".lokalsi-write-test");
    std::fs::create_dir_all(library::models_dir(&dir))
        .and_then(|_| std::fs::write(&probe, b"ok"))
        .and_then(|_| std::fs::remove_file(&probe))
        .map_err(|e| {
            tracing::warn!(dir = %dir.display(), error = %e, "data folder is not writable");
            CmdError::new("not_writable", e.to_string())
        })?;
    let mut settings = state.settings().clone();
    settings.data_dir = Some(dir.clone());
    settings::save(&state.settings_path, &settings)
        .map_err(|e| CmdError::new("error", e.to_string()))?;
    *state.settings() = settings;
    state.llm.set_engines_dir(dir.join("engines"));
    state.queue.set_data_dir(dir.clone());
    state.ext.on_data_dir(&app, &dir);
    tracing::info!(dir = %dir.display(), "data folder set");
    Ok(crate::commands::settings_view(&state))
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> Res<()> {
    let dir = state
        .settings()
        .data_dir
        .clone()
        .ok_or_else(|| CmdError::new("no_data_dir", "no data folder"))?;
    tracing::debug!(command = "open_data_dir", dir = %dir.display());
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| CmdError::new("error", e.to_string()))
}

// --- model library -------------------------------------------------------------

fn data_dir(state: &AppState) -> Res<PathBuf> {
    state
        .settings()
        .data_dir
        .clone()
        .ok_or_else(|| LibraryError::NoDataDir.into())
}

fn views(state: &AppState) -> Res<Vec<ModelView>> {
    let downloads = state.downloads.list()?;
    let budget = budget_of(state);
    let benchmark = state.benchmark().clone();
    Ok(Library::new(&state.catalog, &state.db).list(
        budget.as_ref(),
        &downloads,
        benchmark.as_ref(),
    )?)
}

#[tauri::command]
pub fn list_models(state: State<'_, AppState>) -> Res<Vec<ModelView>> {
    tracing::trace!(command = "list_models");
    views(&state).map_err(log_err("list_models"))
}

#[tauri::command]
pub fn get_starter_pack(state: State<'_, AppState>) -> Res<Vec<StarterPick>> {
    tracing::debug!(command = "get_starter_pack");
    let Some(budget) = budget_of(&state) else {
        return Ok(Vec::new());
    };
    let polish = state.settings().language == Language::Pl;
    let picks = library::starter_pack(&views(&state)?, &budget, polish);
    tracing::debug!(picks = ?picks.iter().map(|p| format!("{}/{}", p.model_id, p.variant_id)).collect::<Vec<_>>(), "starter pack");
    Ok(picks)
}

#[tauri::command]
pub fn download_variant(
    model_id: String,
    variant_id: String,
    state: State<'_, AppState>,
) -> Res<EnqueueOutcome> {
    tracing::debug!(command = "download_variant", %model_id, %variant_id);
    let dir = data_dir(&state)?;
    let plan = Library::new(&state.catalog, &state.db).plan(&model_id, &variant_id, &dir)?;
    state
        .downloads
        .enqueue_variant(&plan)
        .map_err(|e| log_err("download_variant")(CmdError::from(e)))
}

#[tauri::command]
pub fn delete_variant(model_id: String, variant_id: String, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "delete_variant", %model_id, %variant_id);
    let dir = state.settings().data_dir.clone();
    Library::new(&state.catalog, &state.db)
        .delete_variant(&state.downloads, &model_id, &variant_id, dir.as_deref())
        .map_err(CmdError::from)
}

#[tauri::command]
pub fn set_default_model(
    model_id: String,
    variant_id: String,
    state: State<'_, AppState>,
) -> Res<()> {
    tracing::debug!(command = "set_default_model", %model_id, %variant_id);
    Library::new(&state.catalog, &state.db)
        .set_default(&model_id, &variant_id)
        .map_err(CmdError::from)
}

#[tauri::command]
pub fn add_custom_model_file(path: String, state: State<'_, AppState>) -> Res<ModelView> {
    tracing::debug!(command = "add_custom_model_file", %path);
    Library::new(&state.catalog, &state.db)
        .add_custom_from_disk(Path::new(&path), None)
        .map_err(|e| log_err("add_custom_model_file")(CmdError::from(e)))
}

#[tauri::command]
pub async fn add_custom_model_hf(
    repo: String,
    path: String,
    state: State<'_, AppState>,
) -> Res<EnqueueOutcome> {
    let (repo, path) = (
        repo.trim().to_owned(),
        path.trim().trim_start_matches('/').to_owned(),
    );
    tracing::debug!(command = "add_custom_model_hf", %repo, %path);
    let dir = data_dir(&state)?;
    let token = state.download_config().hf_token;
    let row = Library::add_custom_from_hf(&state.db, &state.http, token.as_deref(), &repo, &path)
        .await
        .map_err(|e| log_err("add_custom_model_hf")(CmdError::from(e)))?;
    let plan =
        Library::new(&state.catalog, &state.db).plan(&row.id, library::CUSTOM_VARIANT, &dir)?;
    Ok(state.downloads.enqueue_variant(&plan)?)
}

#[tauri::command]
pub fn remove_custom_model(id: String, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "remove_custom_model", %id);
    let dir = state.settings().data_dir.clone();
    Library::new(&state.catalog, &state.db)
        .remove_custom(&state.downloads, &id, dir.as_deref())
        .map_err(CmdError::from)
}

// --- downloads -------------------------------------------------------------------

#[tauri::command]
pub fn list_downloads(state: State<'_, AppState>) -> Res<Vec<DownloadItem>> {
    tracing::trace!(command = "list_downloads");
    Ok(state.downloads.list()?)
}

#[tauri::command]
pub fn pause_download(id: i64, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "pause_download", id);
    Ok(state.downloads.pause(id)?)
}

#[tauri::command]
pub fn resume_download(id: i64, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "resume_download", id);
    Ok(state.downloads.resume(id)?)
}

#[tauri::command]
pub fn cancel_download(id: i64, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "cancel_download", id);
    Ok(state.downloads.cancel(id)?)
}

/// Activity tray "cancel": stops every pending file of the variant, so a
/// multi-file model is never left half-downloaded by accident.
#[tauri::command]
pub fn cancel_variant_download(
    model_id: String,
    variant_id: String,
    state: State<'_, AppState>,
) -> Res<()> {
    tracing::debug!(command = "cancel_variant_download", %model_id, %variant_id);
    Ok(state.downloads.cancel_variant(&model_id, &variant_id)?)
}

#[tauri::command]
pub fn move_download(id: i64, up: bool, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "move_download", id, up);
    Ok(state.downloads.move_item(id, up)?)
}

#[tauri::command]
pub fn clear_finished_downloads(state: State<'_, AppState>) -> Res<usize> {
    tracing::debug!(command = "clear_finished_downloads");
    Ok(state.downloads.clear_finished()?)
}

// --- Hugging Face token -------------------------------------------------------------

#[tauri::command]
pub fn get_hf_token_status() -> Res<bool> {
    tracing::debug!(command = "get_hf_token_status");
    secrets::hf_token()
        .map(|t| t.is_some())
        .map_err(|e| CmdError::new("secret_store", e.to_string()))
}

/// Saves the token in the OS credential store; an empty value removes it.
#[tauri::command]
pub fn set_hf_token(token: String, state: State<'_, AppState>) -> Res<bool> {
    tracing::debug!(command = "set_hf_token", present = !token.trim().is_empty());
    secrets::set_hf_token(&token).map_err(|e| CmdError::new("secret_store", e.to_string()))?;
    state.downloads.set_config(state.download_config());
    Ok(!token.trim().is_empty())
}

/// Used by `update_settings` to apply download-related changes.
pub fn apply_download_settings(state: &AppState) {
    state.downloads.set_config(state.download_config());
}

/// Source, licence and categories of the AI Transformation benchmark, or
/// `None` when it could not be loaded (the UI then hides benchmark sorting).
#[tauri::command]
pub fn get_benchmark_meta(state: State<'_, AppState>) -> Option<BenchmarkMeta> {
    tracing::debug!(command = "get_benchmark_meta");
    state.benchmark().as_ref().map(|b| b.meta.clone())
}

/// Fetches the benchmark once; if the site does not respond it is ignored.
pub async fn load_benchmark(app: &AppHandle) {
    let state = app.state::<AppState>();
    match Benchmark::fetch(&state.http).await {
        Ok(bench) => {
            let missing = bench.missing_mappings(&state.catalog.models);
            if !missing.is_empty() {
                tracing::warn!(
                    ?missing,
                    "catalog models mapped to names missing from the benchmark"
                );
            }
            let meta = bench.meta.clone();
            *state.benchmark() = Some(bench);
            let _ = app.emit(BENCHMARK_UPDATED, meta);
        }
        Err(e) => {
            tracing::info!(error = %e, "benchmark unavailable, sorting by benchmark disabled")
        }
    }
}

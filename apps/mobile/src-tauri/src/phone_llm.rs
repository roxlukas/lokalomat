//! Models that run on the phone itself: the list for the UI, download, delete.
//! The model runs in the Kotlin plugin `tauri-plugin-localllm`; this layer only
//! manages the files (catalog and download live in `lokalsi-phone-models`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use lokalsi_phone_models::catalog::{Fit, PhoneCatalog, PhoneLicense};
use lokalsi_phone_models::download::{self, DownloadError};
use lokalsi_phone_models::storage;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::{AppState, MobileError, Res, err};

/// Running downloads by model id, so the UI can cancel them.
#[derive(Default)]
pub struct Downloads(Mutex<HashMap<String, CancellationToken>>);

impl Downloads {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, CancellationToken>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl From<DownloadError> for MobileError {
    fn from(e: DownloadError) -> Self {
        err(e.code(), e.to_string())
    }
}

#[derive(Debug, Serialize)]
pub struct PhoneModelView {
    id: String,
    name: String,
    publisher: String,
    description_pl: String,
    description_en: String,
    params_b: f32,
    languages: Vec<String>,
    license: PhoneLicense,
    size: u64,
    min_ram_mb: u64,
    recommended: bool,
    fit: Option<Fit>,
    installed: bool,
    /// Bytes of an interrupted download (0 = none).
    partial_bytes: u64,
    downloading: bool,
}

#[derive(Debug, Serialize)]
pub struct PhoneModels {
    /// Total RAM of this phone (MB), when known.
    ram_mb: Option<u64>,
    models: Vec<PhoneModelView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    downloaded: u64,
    total: u64,
}

fn root(state: &AppState) -> PathBuf {
    state.phone_root.clone()
}

fn catalog() -> PhoneCatalog {
    PhoneCatalog::embedded()
}

#[tauri::command]
pub fn phone_models(state: State<'_, AppState>, downloads: State<'_, Downloads>) -> PhoneModels {
    let root = root(&state);
    let ram_mb = storage::total_ram_mb();
    let running = downloads.lock();
    let models = catalog()
        .models
        .into_iter()
        .map(|m| PhoneModelView {
            fit: ram_mb.map(|r| m.fit(r)),
            installed: storage::is_installed(&root, &m),
            partial_bytes: storage::partial_bytes(&root, &m),
            downloading: running.contains_key(&m.id),
            size: m.file.size,
            id: m.id,
            name: m.name,
            publisher: m.publisher,
            description_pl: m.description_pl,
            description_en: m.description_en,
            params_b: m.params_b,
            languages: m.languages,
            license: m.license,
            min_ram_mb: m.min_ram_mb,
            recommended: m.recommended,
        })
        .collect();
    PhoneModels { ram_mb, models }
}

/// Downloads a model (resumes an interrupted one). Progress goes to `on_progress`;
/// a cancel returns the code `download_cancelled`.
#[tauri::command]
pub async fn phone_model_download(
    id: String,
    on_progress: Channel<DownloadProgress>,
    state: State<'_, AppState>,
    downloads: State<'_, Downloads>,
) -> Res<()> {
    let catalog = catalog();
    let model = catalog
        .find(&id)
        .ok_or_else(|| err("model_unknown", "unknown model"))?;
    let cancel = {
        let mut running = downloads.lock();
        if running.contains_key(&id) {
            return Err(err("download_running", "already downloading"));
        }
        let token = CancellationToken::new();
        running.insert(id.clone(), token.clone());
        token
    };
    let root = root(&state);
    let dest = storage::model_path(&root, model);
    let available = storage::available_space(&root);
    tracing::info!(model = %id, "phone model download started");
    let result = download::download(
        &download::client(),
        &model.file.url(),
        &model.file,
        &dest,
        available,
        &cancel,
        |p| {
            let _ = on_progress.send(DownloadProgress {
                downloaded: p.downloaded,
                total: p.total,
            });
        },
    )
    .await;
    downloads.lock().remove(&id);
    Ok(result?)
}

#[tauri::command]
pub fn phone_model_cancel(id: String, downloads: State<'_, Downloads>) {
    if let Some(token) = downloads.lock().get(&id) {
        token.cancel();
    }
}

/// Deletes a model and any partial download. Refused while it is downloading.
#[tauri::command]
pub fn phone_model_delete(
    id: String,
    state: State<'_, AppState>,
    downloads: State<'_, Downloads>,
) -> Res<()> {
    if downloads.lock().contains_key(&id) {
        return Err(err("download_running", "downloading"));
    }
    let catalog = catalog();
    let model = catalog
        .find(&id)
        .ok_or_else(|| err("model_unknown", "unknown model"))?;
    storage::delete(&root(&state), model).map_err(|e| err("download_io", e.to_string()))
}

/// Path of an installed model, for the plugin's `load`.
#[tauri::command]
pub fn phone_model_path(id: String, state: State<'_, AppState>) -> Res<String> {
    let catalog = catalog();
    let model = catalog
        .find(&id)
        .ok_or_else(|| err("model_unknown", "unknown model"))?;
    let root = root(&state);
    if !storage::is_installed(&root, model) {
        return Err(err("model_not_installed", "not installed"));
    }
    Ok(storage::model_path(&root, model)
        .to_string_lossy()
        .into_owned())
}

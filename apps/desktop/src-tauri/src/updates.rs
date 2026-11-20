//! Updates (PRD §10, M6, ADR 0029): Tauri's signed updater reads
//! `latest.json` from the newest GitHub release. Checking can be switched off
//! in the settings (`update_check`); it is the only network call the app makes
//! besides model and engine downloads.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::AppState;
use crate::models::CmdError;

type Res<T> = Result<T, CmdError>;

/// Event with the newest version, sent after the quiet check at startup.
pub const UPDATE_AVAILABLE: &str = "update://available";
/// Event with `(done, total)` bytes while an update downloads.
pub const UPDATE_PROGRESS: &str = "update://progress";

/// Delay before the quiet check, so it does not compete with the app's start.
const STARTUP_DELAY_S: u64 = 20;
/// Where to get a new version by hand (Linux `.deb` installs).
pub const RELEASES_URL: &str = "https://github.com/roxlukas/lokalomat/releases/latest";

/// The update found by the last check, kept for "Zainstaluj".
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Release notes from `latest.json`.
    pub notes: Option<String>,
    /// The app cannot install it itself (a Linux `.deb` install): the UI
    /// links to the release page instead.
    pub manual: bool,
}

fn err(e: impl std::fmt::Display) -> CmdError {
    let text = e.to_string();
    // Offline or GitHub unreachable: a plain message, details in the log.
    let code = if text.contains("error sending request") || text.contains("dns") {
        "update_offline"
    } else {
        "update_failed"
    };
    CmdError::new(code, text)
}

/// The updater can replace the running app: NSIS/MSI on Windows, an AppImage
/// on Linux, the app bundle on macOS. A `.deb` install is updated by hand.
fn can_install() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some()
}

/// Updates need the owner's public key in `tauri.conf.json`. Until it is
/// there (`PUBKEY_PENDING`), the app makes no update request at all.
fn enabled(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.is_empty() && !k.contains("PENDING"))
}

async fn check(app: &AppHandle) -> Res<Option<UpdateInfo>> {
    if !enabled(app) {
        return Err(CmdError::new("update_unavailable", "no updater public key"));
    }
    let update = app.updater().map_err(err)?.check().await.map_err(err)?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
        manual: !can_install(),
    });
    match &info {
        Some(i) => {
            tracing::info!(version = %i.version, current = %i.current_version, "update available")
        }
        None => tracing::info!("no update available"),
    }
    *app.state::<PendingUpdate>()
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = update;
    Ok(info)
}

/// "Sprawdź teraz" in the settings (works even when the quiet check is off).
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Res<Option<UpdateInfo>> {
    tracing::info!(command = "check_update");
    check(&app).await
}

/// "Zainstaluj i uruchom ponownie": downloads the update found by the last
/// check, verifies its signature (the updater refuses unsigned files),
/// installs it and restarts the app.
#[tauri::command]
pub async fn install_update(app: AppHandle, pending: State<'_, PendingUpdate>) -> Res<()> {
    tracing::info!(command = "install_update");
    if !can_install() {
        return Err(CmdError::new("update_manual", RELEASES_URL));
    }
    let update = pending
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
        .ok_or_else(|| CmdError::new("update_failed", "check for updates first"))?;
    let version = update.version.clone();
    let mut done = 0u64;
    let emitter = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                done += chunk as u64;
                let _ = emitter.emit(UPDATE_PROGRESS, (done, total.unwrap_or(0)));
            },
            || {},
        )
        .await
        .map_err(err)?;
    tracing::info!(%version, "update installed, restarting");
    // Engines and the servers stop with the app's normal exit path.
    app.restart();
}

/// The quiet check at startup, when the setting allows it.
pub fn spawn_startup_check(app: &AppHandle) {
    if !enabled(app) {
        tracing::info!("updates are off: no updater public key in this build");
        return;
    }
    if !app.state::<AppState>().settings().update_check {
        tracing::debug!("update check is off");
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(STARTUP_DELAY_S)).await;
        match check(&app).await {
            Ok(Some(info)) => {
                let _ = app.emit(UPDATE_AVAILABLE, info);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(error = ?e, "update check failed"),
        }
    });
}

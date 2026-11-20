//! IPC for the Serwer view (PRD §15 point 9, ADR 0025): the local
//! OpenAI-compatible server for agents and its access keys. Logic lives in
//! `lokalsi_api_gateway::openai` and `lokalsi_core::api_keys`.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use lokalsi_api_gateway::backend::ModesServer;
use lokalsi_api_gateway::openai::{LogEntry, OpenAiServer, ServerConfig};
use lokalsi_core::api_keys::{ApiKey, ApiKeys};
use lokalsi_core::{secrets, settings};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::AppState;
use crate::models::CmdError;

type Res<T> = Result<T, CmdError>;

#[derive(Debug, Serialize)]
pub struct ServerStatus {
    enabled: bool,
    port: u16,
    /// Listening on every interface (0.0.0.0).
    lan: bool,
    running: bool,
    /// `http://127.0.0.1:<port>/v1`.
    url: Option<String>,
    /// The address other computers use when `lan` is on.
    lan_url: Option<String>,
    /// Why the server could not start (`port_busy`, `error`).
    error: Option<String>,
    keys: Vec<ApiKey>,
}

#[derive(Debug, Serialize)]
pub struct NewKey {
    key: ApiKey,
    /// The whole key, shown once (also kept in the OS credential store).
    secret: String,
}

fn err(e: impl std::fmt::Display) -> CmdError {
    CmdError::new("error", e.to_string())
}

async fn status(state: &AppState) -> ServerStatus {
    let (enabled, port, lan) = {
        let s = state.settings();
        (s.server_enabled, s.server_port, s.server_lan)
    };
    let srv = state.server.lock().await;
    let running_port = srv.as_ref().map(|s| s.addr().port());
    ServerStatus {
        enabled,
        port,
        lan,
        running: srv.is_some(),
        url: running_port.map(|p| format!("http://127.0.0.1:{p}/v1")),
        lan_url: running_port.filter(|_| lan).and_then(|p| {
            lokalsi_api_gateway::server::lan_ip().map(|ip| format!("http://{ip}:{p}/v1"))
        }),
        error: state
            .server_error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone(),
        keys: ApiKeys::new(&state.db).list().unwrap_or_default(),
    }
}

/// Starts (or restarts) the server when it is switched on; stops it otherwise.
pub async fn start_server(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (enabled, port, lan) = {
        let s = state.settings();
        (s.server_enabled, s.server_port, s.server_lan)
    };
    let mut slot = state.server.lock().await;
    if let Some(old) = slot.take() {
        old.stop();
    }
    let set_error = |e: Option<String>| {
        *state.server_error.lock().unwrap_or_else(|p| p.into_inner()) = e;
    };
    if !enabled {
        set_error(None);
        return;
    }
    let config = ServerConfig {
        bind: if lan {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        },
        port,
    };
    let resolver = Arc::clone(&state.resolver) as Arc<dyn lokalsi_engines::modes::ModelResolver>;
    let backend = ModesServer::new(Arc::clone(&state.modes), resolver);
    // The edition adds its own endpoints and MCP (Pro; ADR 0035).
    let extension = state.ext.server_extension(app);
    match OpenAiServer::start(&config, backend, extension).await {
        Ok(s) => {
            *slot = Some(s);
            set_error(None);
        }
        Err(e) => {
            tracing::error!(port, lan, error = %e, "local server cannot start");
            set_error(Some(
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    "port_busy"
                } else {
                    "error"
                }
                .into(),
            ));
        }
    }
}

pub async fn stop_server(state: &AppState) {
    if let Some(s) = state.server.lock().await.take() {
        s.stop();
    }
}

#[tauri::command]
pub async fn get_server(state: State<'_, AppState>) -> Res<ServerStatus> {
    tracing::trace!(command = "get_server");
    Ok(status(&state).await)
}

/// The server switch, "Udostępnij w sieci lokalnej" and the port (Expert).
#[tauri::command]
pub async fn set_server(
    enabled: bool,
    lan: Option<bool>,
    port: Option<u16>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Res<ServerStatus> {
    tracing::info!(command = "set_server", enabled, ?lan, ?port);
    let updated = {
        let mut s = state.settings();
        s.server_enabled = enabled;
        if let Some(l) = lan {
            s.server_lan = l;
        }
        if let Some(p) = port.filter(|p| *p >= 1024) {
            s.server_port = p;
        }
        s.clone()
    };
    settings::save(&state.settings_path, &updated).map_err(err)?;
    start_server(&app).await;
    Ok(status(&state).await)
}

/// "Nowy klucz": the whole key is returned once and kept in the credential store.
#[tauri::command]
pub fn create_api_key(name: String, state: State<'_, AppState>) -> Res<NewKey> {
    tracing::info!(command = "create_api_key");
    let (key, secret) = ApiKeys::new(&state.db)
        .create(&name, None)
        .map_err(|e| match e {
            lokalsi_core::api_keys::ApiKeyError::BadName => CmdError::new("no_name", "key name"),
            other => err(other),
        })?;
    if let Err(e) = secrets::set_api_key(&key.id, &secret) {
        tracing::warn!(error = %e, "cannot keep the key in the credential store");
    }
    Ok(NewKey { key, secret })
}

/// "Kopiuj": the whole key from the credential store.
#[tauri::command]
pub fn api_key_secret(id: String) -> Res<String> {
    tracing::debug!(command = "api_key_secret", key = %id);
    secrets::api_key(&id)
        .map_err(err)?
        .ok_or_else(|| CmdError::new("not_found", "key"))
}

/// "Usuń": the key stops working at once.
#[tauri::command]
pub async fn revoke_api_key(id: String, state: State<'_, AppState>) -> Res<ServerStatus> {
    tracing::info!(command = "revoke_api_key", key = %id);
    ApiKeys::new(&state.db).revoke(&id).map_err(err)?;
    if let Err(e) = secrets::delete_api_key(&id) {
        tracing::warn!(error = %e, "cannot remove the key from the credential store");
    }
    Ok(status(&state).await)
}

/// "Ostatnie zapytania" (no prompt or answer content).
#[tauri::command]
pub async fn server_log(state: State<'_, AppState>) -> Res<Vec<LogEntry>> {
    tracing::trace!(command = "server_log");
    Ok(state
        .server
        .lock()
        .await
        .as_ref()
        .map(|s| s.log())
        .unwrap_or_default())
}

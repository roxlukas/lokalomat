//! IPC for Settings › Sieć (V8): sharing the local API in the local network,
//! pairing phones and the paired devices list. Logic lives in
//! `lokalsi_api_gateway`.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use lokalsi_api_gateway::backend::ModesBackend;
use lokalsi_api_gateway::keys::{OsKeyStore, load_or_create_identity};
use lokalsi_api_gateway::server::{self, Gateway, GatewayConfig, PairingView};
use lokalsi_core::devices::{Device, Devices};
use lokalsi_core::settings;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::AppState;
use crate::models::CmdError;

type Res<T> = Result<T, CmdError>;

#[derive(Debug, Serialize)]
pub struct NetworkStatus {
    enabled: bool,
    port: u16,
    running: bool,
    /// `host:port` phones connect to.
    address: Option<String>,
    /// Why the gateway could not start (`port_busy`, `error`).
    error: Option<String>,
    devices: Vec<Device>,
    pairing: Option<PairingView>,
}

fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Lokalomat".into())
}

async fn status(state: &AppState) -> NetworkStatus {
    let (enabled, port) = {
        let s = state.settings();
        (s.lan_enabled, s.lan_port)
    };
    let gw = state.gateway.lock().await;
    NetworkStatus {
        enabled,
        port,
        running: gw.is_some(),
        address: gw.as_ref().map(|g| {
            let host = server::lan_ip()
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "127.0.0.1".into());
            format!("{host}:{}", g.addr().port())
        }),
        error: state
            .gateway_error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone(),
        devices: Devices::new(&state.db).list().unwrap_or_default(),
        pairing: gw.as_ref().and_then(|g| g.pairing()),
    }
}

/// Starts the gateway when LAN sharing is on (at startup and when enabled).
pub async fn start_gateway(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (enabled, port) = {
        let s = state.settings();
        (s.lan_enabled, s.lan_port)
    };
    let mut slot = state.gateway.lock().await;
    if let Some(old) = slot.take() {
        old.stop().await;
    }
    let set_error = |e: Option<String>| {
        *state
            .gateway_error
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = e;
    };
    if !enabled {
        set_error(None);
        return;
    }
    let cert_path = state.home.join("gateway").join("cert.der");
    let (cert, key) = match load_or_create_identity(&cert_path, &OsKeyStore) {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "cannot prepare the TLS certificate");
            set_error(Some("error".into()));
            return;
        }
    };
    let host = server::lan_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "127.0.0.1".into());
    let config = GatewayConfig {
        bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        port,
        advertise_host: host,
        computer_name: computer_name(),
        version: env!("CARGO_PKG_VERSION").into(),
    };
    // Decisions reach the phone only in Expert mode, as in the Workshop (ADR 0027).
    let handle = app.clone();
    let backend = ModesBackend::with_expert(
        Arc::clone(&state.modes),
        Arc::clone(&state.resolver) as Arc<dyn lokalsi_engines::modes::ModelResolver>,
        Box::new(move || handle.state::<AppState>().settings().expert),
    );
    match Gateway::start(
        config,
        cert,
        key,
        Arc::clone(&state.db),
        Arc::new(OsKeyStore),
        backend,
        state.ext.gateway_extension(app),
        state.events.clone(),
    )
    .await
    {
        Ok(g) => {
            *slot = Some(g);
            set_error(None);
        }
        Err(e) => {
            tracing::error!(port, error = %e, "local API gateway cannot start");
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

pub async fn stop_gateway(state: &AppState) {
    if let Some(g) = state.gateway.lock().await.take() {
        g.stop().await;
    }
}

#[tauri::command]
pub async fn get_network(state: State<'_, AppState>) -> Res<NetworkStatus> {
    tracing::trace!(command = "get_network");
    Ok(status(&state).await)
}

/// The "Udostępniaj w sieci lokalnej" switch (and the port, Expert).
#[tauri::command]
pub async fn set_lan_sharing(
    enabled: bool,
    port: Option<u16>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Res<NetworkStatus> {
    tracing::info!(command = "set_lan_sharing", enabled, ?port);
    let updated = {
        let mut s = state.settings();
        s.lan_enabled = enabled;
        if let Some(p) = port.filter(|p| *p >= 1024) {
            s.lan_port = p;
        }
        s.clone()
    };
    settings::save(&state.settings_path, &updated)
        .map_err(|e| CmdError::new("error", e.to_string()))?;
    start_gateway(&app).await;
    Ok(status(&state).await)
}

/// "Połącz telefon": QR code with a one-time token.
#[tauri::command]
pub async fn start_pairing(state: State<'_, AppState>) -> Res<PairingView> {
    tracing::debug!(command = "start_pairing");
    let gw = state.gateway.lock().await;
    let g = gw
        .as_ref()
        .ok_or_else(|| CmdError::new("lan_disabled", "LAN sharing is off"))?;
    Ok(g.begin_pairing())
}

/// "Zgadza się" / "Nie zgadza się" after comparing the codes.
#[tauri::command]
pub async fn confirm_pairing(codes_match: bool, state: State<'_, AppState>) -> Res<bool> {
    tracing::debug!(command = "confirm_pairing", codes_match);
    let gw = state.gateway.lock().await;
    Ok(gw.as_ref().is_some_and(|g| g.confirm_pairing(codes_match)))
}

#[tauri::command]
pub async fn cancel_pairing(state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "cancel_pairing");
    if let Some(g) = state.gateway.lock().await.as_ref() {
        g.cancel_pairing();
    }
    Ok(())
}

/// "Odłącz".
#[tauri::command]
pub async fn unpair_device(id: String, state: State<'_, AppState>) -> Res<NetworkStatus> {
    tracing::info!(command = "unpair_device", device = %id);
    {
        let gw = state.gateway.lock().await;
        match gw.as_ref() {
            Some(g) => g.unpair(&id),
            None => server::unpair(&state.db, &OsKeyStore, &id),
        }
        .map_err(|e| CmdError::new("error", e))?;
    }
    Ok(status(&state).await)
}

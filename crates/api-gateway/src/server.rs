//! The gateway in the desktop app: a TLS listener with the pairing endpoint
//! and the `/v1/*` routes the phone uses. What the routes do is delegated
//! to a [`Backend`] (the app's modes, or a fake in tests).

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use lokalsi_core::db::Db;
use lokalsi_core::devices::{Device, Devices};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::api::{
    ChatChoice, ChatRequest, ChatResponse, DecisionJobRequest, ErrorBody, Info, JobView,
    ModelOption, PairRequest, PairResponse, Turn,
};
use crate::keys::KeyStore;
use crate::pairing::{
    self, CONFIRM_TTL_SECS, QrPayload, TOKEN_TTL_SECS, b64, comparison_code, ct_eq, sha256,
    signed_message, unb64, verify_signature,
};

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Largest recording the phone may upload (bytes).
pub const MAX_UPLOAD: usize = 200 * 1024 * 1024;
/// Wrong pairing tokens before the pairing is cancelled.
const MAX_TOKEN_FAILURES: u8 = 5;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub detail: String,
}

impl ApiError {
    pub fn new(status: u16, code: &str, detail: impl Into<String>) -> Self {
        ApiError {
            status,
            code: code.to_owned(),
            detail: detail.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (
            status,
            Json(ErrorBody {
                error: self.code,
                detail: self.detail,
            }),
        )
            .into_response()
    }
}

/// The paired device that sent a request (set by the auth layer).
#[derive(Debug, Clone)]
pub struct DeviceId(pub String);

/// What the routes of the free edition do: chat and decisions. The app
/// implements it with its modes ([`crate::backend::ModesBackend`]). Further
/// modes (speech, images, songs…) are added by a [`GatewayExtension`].
pub trait Backend: Send + Sync + 'static {
    /// Modes available now (`chat`, `decision`).
    fn modes(&self) -> Vec<String>;
    /// Installed models the phone may choose for a mode of [`Backend::modes`].
    fn models(&self, mode: &str) -> Vec<ModelOption>;
    /// Returns `(answer, model name)`. `model`: a [`ModelOption::id`] or the default.
    fn chat(
        &self,
        messages: Vec<Turn>,
        model: Option<String>,
    ) -> BoxFut<'_, Result<(String, String), ApiError>>;
    /// Creates a decision job for `device` and starts it in the background; the
    /// job keeps running when the phone disconnects.
    fn start_decision(&self, device: &str, req: DecisionJobRequest) -> Result<JobView, ApiError>;
    /// A job `device` started (`None` for other jobs, so phones see only their own).
    fn job(&self, device: &str, job_id: i64) -> Option<JobView>;
    /// Cancels a running job of `device`; `false` when it is not one or already ended.
    fn cancel_job(&self, device: &str, job_id: i64) -> bool;
    /// The computer is in Expert mode (sent to the phone in `/v1/info`, ADR 0037).
    fn expert(&self) -> bool {
        false
    }
    /// The computer runs the Pro edition (sent to the phone in `/v1/info`).
    fn pro(&self) -> bool {
        false
    }
}

/// More routes and modes added to the phone gateway (ADR 0035): the Pro
/// edition brings speech, transcription, images, songs and the gallery. The
/// routes are mounted under `/v1` behind the same device check, rate limit and
/// `no-store` header as the built-in ones.
pub trait GatewayExtension: Send + Sync + 'static {
    /// Extra modes offered now (`stt`, `tts`, `image`, `music`).
    fn modes(&self) -> Vec<String>;
    /// Models of an extra mode; `None` when the mode is not the extension's.
    fn models(&self, mode: &str) -> Option<Vec<ModelOption>>;
    /// Routes relative to `/v1` with their own state.
    fn routes(&self) -> Router;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingState {
    /// QR code shown, waiting for the phone.
    Waiting,
    /// The phone answered; the user compares the codes.
    Confirm,
    Paired,
    Rejected,
    Expired,
    Cancelled,
}

/// QR code modules row by row (`1` = dark), drawn by the UI with design tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QrMatrix {
    pub size: usize,
    pub modules: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingView {
    pub state: PairingState,
    pub uri: String,
    pub qr: Option<QrMatrix>,
    /// Seconds left for the current stage.
    pub expires_in: u64,
    pub code: Option<String>,
    pub device_name: Option<String>,
}

pub trait GatewayEvents: Send + Sync + 'static {
    fn pairing_changed(&self, _view: &PairingView) {}
    fn devices_changed(&self) {}
}

pub struct NoEvents;
impl GatewayEvents for NoEvents {}

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub bind: IpAddr,
    /// `0` = any free port (tests).
    pub port: u16,
    /// Address put into the QR code (the computer's LAN IP).
    pub advertise_host: String,
    pub computer_name: String,
    pub version: String,
}

enum Stage {
    Waiting,
    Confirm {
        device_name: String,
        code: String,
        since: Instant,
        reply: Option<oneshot::Sender<bool>>,
    },
}

struct Pending {
    token: String,
    created: Instant,
    failures: u8,
    uri: String,
    stage: Stage,
}

struct Shared {
    config: GatewayConfig,
    port: u16,
    fingerprint: [u8; 32],
    db: Arc<Db>,
    keys: Arc<dyn KeyStore>,
    backend: Arc<dyn Backend>,
    extension: Option<Arc<dyn GatewayExtension>>,
    events: Arc<dyn GatewayEvents>,
    pairing: Mutex<Option<Pending>>,
    /// Device id → SHA-256 of its access key.
    devices: Mutex<HashMap<String, [u8; 32]>>,
    last_seen: Mutex<HashMap<String, Instant>>,
    /// Device id → (start of the current minute, requests in it).
    rate: Mutex<HashMap<String, (Instant, u32)>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn qr_matrix(uri: &str) -> Option<QrMatrix> {
    let code = qrcode::QrCode::with_error_correction_level(uri, qrcode::EcLevel::M).ok()?;
    let modules = code
        .to_colors()
        .iter()
        .map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' })
        .collect();
    Some(QrMatrix {
        size: code.width(),
        modules,
    })
}

impl Shared {
    fn view(&self, p: &Pending) -> PairingView {
        let (state, code, device_name, expires_in) = match &p.stage {
            Stage::Waiting => (
                PairingState::Waiting,
                None,
                None,
                TOKEN_TTL_SECS.saturating_sub(p.created.elapsed().as_secs()),
            ),
            Stage::Confirm {
                device_name,
                code,
                since,
                ..
            } => (
                PairingState::Confirm,
                Some(code.clone()),
                Some(device_name.clone()),
                CONFIRM_TTL_SECS.saturating_sub(since.elapsed().as_secs()),
            ),
        };
        PairingView {
            state,
            uri: p.uri.clone(),
            qr: matches!(state, PairingState::Waiting)
                .then(|| qr_matrix(&p.uri))
                .flatten(),
            expires_in,
            code,
            device_name,
        }
    }

    fn ended(&self, state: PairingState) -> PairingView {
        PairingView {
            state,
            uri: String::new(),
            qr: None,
            expires_in: 0,
            code: None,
            device_name: None,
        }
    }

    /// Clears the pending pairing and publishes how it ended.
    fn finish_pairing(&self, state: PairingState) {
        *lock(&self.pairing) = None;
        self.events.pairing_changed(&self.ended(state));
    }

    /// Checks `Authorization: Bearer <id>.<key>`; returns the device id.
    fn authenticate(&self, headers: &HeaderMap) -> Option<String> {
        let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
        let (id, key) = value.strip_prefix("Bearer ")?.trim().split_once('.')?;
        let expected = *lock(&self.devices).get(id)?;
        if !ct_eq(&sha256(key.as_bytes()), &expected) {
            return None;
        }
        let touch = {
            let mut seen = lock(&self.last_seen);
            let due = seen
                .get(id)
                .is_none_or(|t| t.elapsed() > Duration::from_secs(60));
            if due {
                seen.insert(id.to_owned(), Instant::now());
            }
            due
        };
        if touch && let Err(e) = Devices::new(&self.db).touch(id) {
            tracing::warn!(error = %e, "cannot update last_seen of a device");
        }
        Some(id.to_owned())
    }
}

pub struct Gateway {
    shared: Arc<Shared>,
    addr: SocketAddr,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Gateway {
    /// Binds the listener and starts serving. Known devices are loaded with
    /// their keys from the key store.
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        config: GatewayConfig,
        cert_der: Vec<u8>,
        key_der: Vec<u8>,
        db: Arc<Db>,
        keys: Arc<dyn KeyStore>,
        backend: Arc<dyn Backend>,
        extension: Option<Arc<dyn GatewayExtension>>,
        events: Arc<dyn GatewayEvents>,
    ) -> std::io::Result<Gateway> {
        let fingerprint = crate::tls::fingerprint(&cert_der);
        let tls = crate::tls::server_config(cert_der, key_der)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let listener = TcpListener::bind((config.bind, config.port)).await?;
        let addr = listener.local_addr()?;
        let mut devices = HashMap::new();
        for d in Devices::new(&db).list().unwrap_or_default() {
            match keys.device_key(&d.id) {
                Some(k) => {
                    devices.insert(d.id, sha256(k.as_bytes()));
                }
                None => {
                    tracing::warn!(device = %d.id, "paired device has no key in the credential store; it must pair again")
                }
            }
        }
        tracing::info!(addr = %addr, devices = devices.len(), "local API gateway listening (TLS)");
        let shared = Arc::new(Shared {
            port: addr.port(),
            config,
            fingerprint,
            db,
            keys,
            backend,
            extension,
            events,
            pairing: Mutex::new(None),
            devices: Mutex::new(devices),
            last_seen: Mutex::new(HashMap::new()),
            rate: Mutex::new(HashMap::new()),
        });
        let stop = CancellationToken::new();
        let task = tokio::spawn(serve(
            listener,
            tokio_rustls::TlsAcceptor::from(tls),
            router(Arc::clone(&shared)),
            stop.clone(),
        ));
        Ok(Gateway {
            shared,
            addr,
            stop,
            task: Some(task),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        self.shared.fingerprint
    }

    /// "Połącz telefon": a fresh one-time token and the QR code. Replaces
    /// (and rejects) a pairing that was still pending.
    pub fn begin_pairing(&self) -> PairingView {
        let s = &self.shared;
        let token = pairing::new_token();
        let uri = QrPayload {
            host: s.config.advertise_host.clone(),
            port: s.port,
            fingerprint: s.fingerprint,
            token: token.clone(),
            name: s.config.computer_name.clone(),
        }
        .to_uri();
        let p = Pending {
            token,
            created: Instant::now(),
            failures: 0,
            uri,
            stage: Stage::Waiting,
        };
        let view = s.view(&p);
        // Dropping an old reply sender makes a waiting phone see "rejected".
        *lock(&s.pairing) = Some(p);
        tracing::info!(host = %s.config.advertise_host, port = s.port, "pairing started");
        s.events.pairing_changed(&view);
        view
    }

    /// The current pairing, if any (expired tokens end it).
    pub fn pairing(&self) -> Option<PairingView> {
        let s = &self.shared;
        let expired = {
            let g = lock(&s.pairing);
            let p = g.as_ref()?;
            matches!(p.stage, Stage::Waiting) && p.created.elapsed().as_secs() >= TOKEN_TTL_SECS
        };
        if expired {
            s.finish_pairing(PairingState::Expired);
            return Some(s.ended(PairingState::Expired));
        }
        lock(&s.pairing).as_ref().map(|p| s.view(p))
    }

    /// The user's answer to "Czy telefon pokazuje ten sam kod?". `false`
    /// aborts the pairing. Returns `false` when nothing waited for an answer.
    pub fn confirm_pairing(&self, codes_match: bool) -> bool {
        let reply = {
            let mut g = lock(&self.shared.pairing);
            match g.as_mut().map(|p| &mut p.stage) {
                Some(Stage::Confirm { reply, .. }) => reply.take(),
                _ => None,
            }
        };
        match reply {
            Some(tx) => {
                tracing::info!(codes_match, "pairing answered by the user");
                let delivered = tx.send(codes_match).is_ok();
                if !delivered {
                    // Nobody is waiting any more (the phone is gone): do not leave the question open.
                    self.shared.finish_pairing(PairingState::Cancelled);
                }
                delivered
            }
            None => false,
        }
    }

    pub fn cancel_pairing(&self) {
        if lock(&self.shared.pairing).is_some() {
            tracing::info!("pairing cancelled");
            self.shared.finish_pairing(PairingState::Cancelled);
        }
    }

    pub fn devices(&self) -> Vec<Device> {
        Devices::new(&self.shared.db).list().unwrap_or_default()
    }

    /// "Odłącz": the device's key stops working immediately.
    pub fn unpair(&self, device_id: &str) -> Result<bool, String> {
        unpair(&self.shared.db, self.shared.keys.as_ref(), device_id)?;
        let known = lock(&self.shared.devices).remove(device_id).is_some();
        self.shared.events.devices_changed();
        Ok(known)
    }

    pub async fn stop(mut self) {
        self.stop.cancel();
        self.cancel_pairing();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        tracing::info!("local API gateway stopped");
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// Removes a paired device (also while the gateway is off).
pub fn unpair(db: &Db, keys: &dyn KeyStore, device_id: &str) -> Result<bool, String> {
    keys.delete_device_key(device_id)?;
    let removed = Devices::new(db)
        .delete(device_id)
        .map_err(|e| e.to_string())?;
    tracing::info!(device = %device_id, removed, "device unpaired");
    Ok(removed)
}

async fn serve(
    listener: TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
    app: Router,
    stop: CancellationToken,
) {
    loop {
        let (tcp, peer) = tokio::select! {
            r = listener.accept() => match r {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
            _ = stop.cancelled() => return,
        };
        let acceptor = acceptor.clone();
        let app = app.clone();
        let stop = stop.clone();
        tokio::spawn(async move {
            let tls = match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await {
                Ok(Ok(tls)) => tls,
                Ok(Err(e)) => {
                    tracing::debug!(%peer, error = %e, "TLS handshake failed");
                    return;
                }
                Err(_) => {
                    tracing::debug!(%peer, "TLS handshake timed out");
                    return;
                }
            };
            let svc = hyper_util::service::TowerToHyperService::new(app);
            let conn = hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(tls), svc);
            tokio::select! {
                r = conn => if let Err(e) = r { tracing::debug!(%peer, error = %e, "connection ended with an error") },
                _ = stop.cancelled() => {}
            }
        });
    }
}

fn router(shared: Arc<Shared>) -> Router {
    let v1 = Router::new()
        .route("/info", get(info))
        .route("/chat/completions", post(chat))
        .route("/jobs/decision", post(start_decision))
        .route("/jobs/{job_id}", get(job_status))
        .route("/jobs/{job_id}/cancel", post(cancel_job))
        .route("/models", get(models))
        .with_state(Arc::clone(&shared));
    // Extra routes (the Pro edition) join before the device check, so they get
    // the same authentication, rate limit and `no-store` header.
    let v1 = match &shared.extension {
        Some(ext) => v1.merge(ext.routes()),
        None => v1,
    };
    let v1 = v1.route_layer(middleware::from_fn_with_state(
        Arc::clone(&shared),
        require_device,
    ));
    Router::new()
        .route("/pair", post(pair))
        .with_state(shared)
        .nest("/v1", v1)
        .layer(middleware::from_fn(reject_browsers))
}

/// Browsers send `Origin`; the phone app never does. Refusing it blocks web
/// pages (and DNS rebinding) from reaching the API.
async fn reject_browsers(req: Request, next: Next) -> Response {
    if req.headers().contains_key(header::ORIGIN) {
        tracing::warn!(path = %req.uri().path(), "request with an Origin header refused");
        return ApiError::new(403, "forbidden_origin", "").into_response();
    }
    next.run(req).await
}

/// Requests one device may send per minute (a gallery screen with thumbnails
/// needs a few dozen); more answers `429 rate_limited`.
const MAX_REQUESTS_PER_MINUTE: u32 = 600;

impl Shared {
    /// Counts a request of `device`; `false` when it is over the limit.
    fn within_rate(&self, device: &str) -> bool {
        let mut rate = lock(&self.rate);
        let entry = rate.entry(device.to_owned()).or_insert((Instant::now(), 0));
        if entry.0.elapsed() >= Duration::from_secs(60) {
            *entry = (Instant::now(), 0);
        }
        entry.1 += 1;
        entry.1 <= MAX_REQUESTS_PER_MINUTE
    }
}

async fn require_device(State(s): State<Arc<Shared>>, mut req: Request, next: Next) -> Response {
    match s.authenticate(req.headers()) {
        Some(id) => {
            if !s.within_rate(&id) {
                tracing::warn!(device = %id, "too many requests from a device");
                return ApiError::new(429, "rate_limited", "").into_response();
            }
            tracing::debug!(device = %id, method = %req.method(), path = %req.uri().path(), "api request");
            req.extensions_mut().insert(DeviceId(id));
            let mut res = next.run(req).await;
            // Results and thumbnails are private: keep them out of any cache.
            res.headers_mut().insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-store"),
            );
            res
        }
        None => {
            tracing::warn!(path = %req.uri().path(), "unpaired request refused");
            ApiError::new(401, "unauthorized", "").into_response()
        }
    }
}

// --- pairing --------------------------------------------------------------------------

/// Ends the confirmation on the computer when the phone's request is dropped.
struct ConfirmGuard<'a> {
    shared: &'a Shared,
    token: String,
    armed: bool,
}

impl Drop for ConfirmGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let ours = lock(&self.shared.pairing)
            .as_ref()
            .is_some_and(|p| p.token == self.token && matches!(p.stage, Stage::Confirm { .. }));
        if ours {
            tracing::info!("phone left during the pairing confirmation; pairing cancelled");
            self.shared.finish_pairing(PairingState::Cancelled);
        }
    }
}

async fn pair(State(s): State<Arc<Shared>>, body: Bytes) -> Result<Json<PairResponse>, ApiError> {
    let req: PairRequest = serde_json::from_slice(&body)
        .map_err(|e| ApiError::new(400, "bad_request", e.to_string()))?;
    let device_name: String = req.device_name.trim().chars().take(60).collect();
    let device_name = if device_name.is_empty() {
        "Telefon".to_owned()
    } else {
        device_name
    };

    let (rx, public_key, view, token) = {
        let mut g = lock(&s.pairing);
        let Some(p) = g.as_mut() else {
            return Err(ApiError::new(410, "pairing_expired", ""));
        };
        if !matches!(p.stage, Stage::Waiting) {
            return Err(ApiError::new(409, "pairing_busy", ""));
        }
        if p.created.elapsed().as_secs() >= TOKEN_TTL_SECS {
            drop(g);
            s.finish_pairing(PairingState::Expired);
            return Err(ApiError::new(410, "pairing_expired", ""));
        }
        if !ct_eq(req.token.as_bytes(), p.token.as_bytes()) {
            p.failures += 1;
            tracing::warn!(failures = p.failures, "wrong pairing token");
            if p.failures >= MAX_TOKEN_FAILURES {
                drop(g);
                s.finish_pairing(PairingState::Cancelled);
            }
            return Err(ApiError::new(401, "bad_token", ""));
        }
        let public_key = unb64(&req.public_key)
            .filter(|k| k.len() == 32)
            .ok_or_else(|| ApiError::new(400, "bad_request", "public key"))?;
        let signature =
            unb64(&req.signature).ok_or_else(|| ApiError::new(400, "bad_request", "signature"))?;
        if !verify_signature(
            &public_key,
            &signed_message(&p.token, &s.fingerprint),
            &signature,
        ) {
            return Err(ApiError::new(400, "bad_request", "signature"));
        }
        let code = comparison_code(&s.fingerprint, &public_key, &p.token);
        let (tx, rx) = oneshot::channel();
        p.stage = Stage::Confirm {
            device_name: device_name.clone(),
            code,
            since: Instant::now(),
            reply: Some(tx),
        };
        (rx, public_key, s.view(p), p.token.clone())
    };
    tracing::info!(device_name = %device_name, "phone answered the pairing; waiting for the user");
    s.events.pairing_changed(&view);

    // The phone gave up (cancelled, lost the network): this request is dropped
    // while it waits, and the question on the computer must go away with it.
    let mut guard = ConfirmGuard {
        shared: &s,
        token,
        armed: true,
    };
    let answer = tokio::time::timeout(Duration::from_secs(CONFIRM_TTL_SECS), rx).await;
    guard.armed = false;
    match answer {
        Ok(Ok(true)) => {}
        Ok(Ok(false)) | Ok(Err(_)) => {
            // `Err`: replaced or cancelled on the computer.
            let still_ours = matches!(
                lock(&s.pairing).as_ref().map(|p| &p.stage),
                Some(Stage::Confirm { reply: None, .. })
            );
            if still_ours {
                s.finish_pairing(PairingState::Rejected);
            }
            tracing::warn!("pairing rejected: the codes did not match or it was cancelled");
            return Err(ApiError::new(403, "pairing_rejected", ""));
        }
        Err(_) => {
            s.finish_pairing(PairingState::Expired);
            return Err(ApiError::new(410, "pairing_expired", ""));
        }
    }

    let device_id = hex(&pairing::random_bytes::<8>());
    let access_key = b64(&pairing::random_bytes::<32>());
    if let Err(e) = s.keys.set_device_key(&device_id, &access_key) {
        tracing::error!(error = %e, "cannot store the device key");
        s.finish_pairing(PairingState::Cancelled);
        return Err(ApiError::new(500, "keystore", e));
    }
    if let Err(e) = Devices::new(&s.db).insert(&device_id, &device_name, &b64(&public_key)) {
        let _ = s.keys.delete_device_key(&device_id);
        s.finish_pairing(PairingState::Cancelled);
        return Err(ApiError::new(500, "error", e.to_string()));
    }
    lock(&s.devices).insert(device_id.clone(), sha256(access_key.as_bytes()));
    tracing::info!(device = %device_id, "phone paired");
    s.finish_pairing(PairingState::Paired);
    s.events.devices_changed();
    Ok(Json(PairResponse {
        device_id,
        access_key,
        computer_name: s.config.computer_name.clone(),
    }))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// --- API ------------------------------------------------------------------------------

pub fn json_body<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|e| ApiError::new(400, "bad_request", e.to_string()))
}

async fn info(State(s): State<Arc<Shared>>) -> Json<Info> {
    Json(Info {
        computer_name: s.config.computer_name.clone(),
        version: s.config.version.clone(),
        modes: {
            let mut modes = s.backend.modes();
            if let Some(ext) = &s.extension {
                modes.extend(ext.modes());
            }
            modes
        },
        expert: s.backend.expert(),
        pro: s.backend.pro(),
    })
}

async fn chat(State(s): State<Arc<Shared>>, body: Bytes) -> Result<Json<ChatResponse>, ApiError> {
    let req: ChatRequest = json_body(&body)?;
    if req.messages.is_empty() || req.messages.len() > 200 {
        return Err(ApiError::new(400, "bad_request", "messages"));
    }
    let (answer, model) = s.backend.chat(req.messages, req.model).await?;
    Ok(Json(ChatResponse {
        object: "chat.completion".into(),
        model,
        choices: vec![ChatChoice {
            index: 0,
            message: Turn {
                role: "assistant".into(),
                content: answer,
            },
            finish_reason: "stop".into(),
        }],
    }))
}

pub async fn file_response(path: PathBuf, content_type: &str) -> Result<Response, ApiError> {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| ApiError::new(404, "not_found", ""))?;
    Ok((
        [(header::CONTENT_TYPE, content_type.to_owned())],
        Body::from(bytes),
    )
        .into_response())
}

// --- jobs (ADR 0027) ---------------------------------------------------------------------

async fn start_decision(
    State(s): State<Arc<Shared>>,
    Extension(DeviceId(device)): Extension<DeviceId>,
    body: Bytes,
) -> Result<Json<JobView>, ApiError> {
    let req: DecisionJobRequest = json_body(&body)?;
    let view = s.backend.start_decision(&device, req)?;
    tracing::info!(device = %device, kind = "decision", job_id = view.job_id, "job started by a phone");
    Ok(Json(view))
}

async fn job_status(
    State(s): State<Arc<Shared>>,
    Extension(DeviceId(device)): Extension<DeviceId>,
    Path(job_id): Path<i64>,
) -> Result<Json<JobView>, ApiError> {
    s.backend
        .job(&device, job_id)
        .map(Json)
        .ok_or_else(|| ApiError::new(404, "not_found", ""))
}

async fn cancel_job(
    State(s): State<Arc<Shared>>,
    Extension(DeviceId(device)): Extension<DeviceId>,
    Path(job_id): Path<i64>,
) -> Result<Json<JobView>, ApiError> {
    if s.backend.job(&device, job_id).is_none() {
        return Err(ApiError::new(404, "not_found", ""));
    }
    let cancelled = s.backend.cancel_job(&device, job_id);
    tracing::info!(device = %device, job_id, cancelled, "phone cancelled a job");
    s.backend
        .job(&device, job_id)
        .map(Json)
        .ok_or_else(|| ApiError::new(404, "not_found", ""))
}

#[derive(Deserialize)]
struct ModelsQuery {
    mode: String,
}

async fn models(
    State(s): State<Arc<Shared>>,
    Query(q): Query<ModelsQuery>,
) -> Result<Json<Vec<ModelOption>>, ApiError> {
    if !matches!(
        q.mode.as_str(),
        "chat" | "stt" | "tts" | "image" | "music" | "decision"
    ) {
        return Err(ApiError::new(400, "bad_request", "mode"));
    }
    if let Some(ext) = &s.extension
        && let Some(list) = ext.models(&q.mode)
    {
        return Ok(Json(list));
    }
    Ok(Json(s.backend.models(&q.mode)))
}

/// The computer's address in the local network (the interface of the
/// default route); no packet is sent.
pub fn lan_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

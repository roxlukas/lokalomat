//! The local OpenAI-compatible server (PRD §15 point 9, ADR 0025).
//!
//! Plain HTTP on 127.0.0.1 (or 0.0.0.0 when the user allows it), every
//! request with an access key. It does not reimplement the OpenAI API: after
//! checking the key, resolving `model` and loading it (the engine installs
//! itself on the first request), the request body goes to `llama-server`
//! byte for byte and its answer — streamed SSE, tool calls, usage — comes
//! back the same way. The loaded slot stays reserved until the answer ends
//! or the client disconnects.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

pub type BoxFut<'a, T> = Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Default port of the local server (the phone gateway uses 47821).
pub const DEFAULT_SERVER_PORT: u16 = 47_822;
/// Largest request body (prompts with long histories and tool lists).
pub const MAX_BODY: usize = 32 * 1024 * 1024;
/// Requests per minute and key.
const RATE_LIMIT: u32 = 600;
/// Requests kept in the "Ostatnie zapytania" list.
const LOG_SIZE: usize = 200;

/// Which `llama-server` slot a request goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Chat,
    Decision,
    /// Not served by `llama-server`: listed by `/v1/models`, used by the media endpoints.
    Image,
    Stt,
    Tts,
    Music,
}

/// A model listed by `/v1/models`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServedModel {
    /// `<model>:<variant>`.
    pub id: String,
    pub name: String,
    pub slot: Slot,
    /// The model used when a request names none (or `default`).
    pub default: bool,
}

/// A key that passed the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub key_id: String,
    pub name: String,
}

/// A loaded slot reserved for one request; dropping it frees the slot.
pub struct UpstreamHandle {
    pub base_url: String,
    pub token: String,
    pub model_name: String,
    pub hold: Box<dyn Send + Sync>,
}

/// Why a request could not be served, with an OpenAI-style error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OaError {
    pub status: u16,
    pub kind: &'static str,
    pub code: String,
    pub message: String,
}

impl OaError {
    pub fn new(status: u16, kind: &'static str, code: &str, message: impl Into<String>) -> Self {
        OaError {
            status,
            kind,
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

impl IntoResponse for OaError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (
            status,
            Json(json!({ "error": { "message": self.message, "type": self.kind, "code": self.code } })),
        )
            .into_response()
    }
}

/// What the server needs from the app.
pub trait ServerBackend: Send + Sync + 'static {
    /// The key a bearer token belongs to (and counts the request).
    fn verify_key(&self, token: &str) -> Option<Caller>;
    fn models(&self) -> Vec<ServedModel>;
    /// Loads `model` (`None` = the default one) in `slot` and reserves it.
    fn upstream(
        &self,
        slot: Slot,
        model: Option<String>,
        cancel: CancellationToken,
    ) -> BoxFut<'_, Result<UpstreamHandle, OaError>>;
}

/// More models, routes and MCP added to the local server (ADR 0035): the Pro
/// edition brings images, speech, transcription, songs, pipelines and the MCP
/// endpoint. Routes are mounted behind the same key check, rate limit and
/// `no-store` header as the built-in ones.
pub trait ServerExtension: Send + Sync + 'static {
    /// Models of the extra slots, listed by `/v1/models`.
    fn models(&self) -> Vec<ServedModel>;
    /// Routes relative to `/v1`.
    fn v1_routes(&self, shared: Arc<Shared>) -> Router;
    /// Routes at the root (the MCP endpoint).
    fn root_routes(&self, shared: Arc<Shared>) -> Router;
}

impl Shared {
    /// The models of the built-in slots and of the extension.
    pub fn all_models(&self) -> Vec<ServedModel> {
        let mut all = self.backend.models();
        if let Some(ext) = &self.extension {
            all.extend(ext.models());
        }
        all
    }
}

/// One line of the request log. Never holds prompt or answer content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogEntry {
    pub at: i64,
    pub key: String,
    pub method: String,
    pub path: String,
    pub model: Option<String>,
    pub status: u16,
    pub duration_ms: u64,
    pub stream: bool,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: IpAddr,
    pub port: u16,
}

pub struct Shared {
    pub backend: Arc<dyn ServerBackend>,
    pub http: reqwest::Client,
    log: Mutex<VecDeque<LogEntry>>,
    rate: Mutex<HashMap<String, (Instant, u32)>>,
    /// More models, routes and MCP from the Pro edition (ADR 0035).
    pub(crate) extension: Option<Arc<dyn ServerExtension>>,
}

impl Shared {
    pub fn record(&self, entry: LogEntry) {
        let mut log = self.log.lock().unwrap_or_else(|p| p.into_inner());
        if log.len() >= LOG_SIZE {
            log.pop_front();
        }
        log.push_back(entry);
    }
}

/// The running server. Dropping it stops it.
pub struct OpenAiServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
    stop: CancellationToken,
}

impl OpenAiServer {
    pub async fn start(
        config: &ServerConfig,
        backend: Arc<dyn ServerBackend>,
        extension: Option<Arc<dyn ServerExtension>>,
    ) -> std::io::Result<OpenAiServer> {
        let listener = TcpListener::bind((config.bind, config.port)).await?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            backend,
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .build()
                .map_err(std::io::Error::other)?,
            log: Mutex::new(VecDeque::new()),
            rate: Mutex::new(HashMap::new()),
            extension,
        });
        let stop = CancellationToken::new();
        let app = router(Arc::clone(&shared));
        let token = stop.clone();
        tokio::spawn(async move {
            let result = axum::serve(listener, app)
                .with_graceful_shutdown(async move { token.cancelled().await })
                .await;
            if let Err(e) = result {
                tracing::error!(error = %e, "local server stopped with an error");
            }
        });
        tracing::info!(%addr, "local OpenAI-compatible server started");
        Ok(OpenAiServer { addr, shared, stop })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The last requests, newest last.
    pub fn log(&self) -> Vec<LogEntry> {
        self.shared
            .log
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    pub fn stop(&self) {
        if !self.stop.is_cancelled() {
            tracing::info!(addr = %self.addr, "local server stopped");
        }
        self.stop.cancel();
    }
}

impl Drop for OpenAiServer {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn router(shared: Arc<Shared>) -> Router {
    let v1 = Router::new()
        .route("/models", get(models))
        .route("/chat/completions", post(chat_completions))
        .route("/completions", post(completions))
        .route("/systemone", post(systemone))
        .with_state(Arc::clone(&shared));
    let key_check = || middleware::from_fn_with_state(Arc::clone(&shared), require_key);
    // Extra routes (the Pro edition) join before the key check, so they get the
    // same key check, rate limit and `no-store` header.
    let (v1, root) = match &shared.extension {
        Some(ext) => (
            v1.merge(ext.v1_routes(Arc::clone(&shared))),
            // Model Context Protocol (ADR 0033): the same keys.
            ext.root_routes(Arc::clone(&shared))
                .route_layer(key_check()),
        ),
        None => (v1, Router::new()),
    };
    let v1 = v1.route_layer(key_check());
    Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .nest("/v1", v1)
        .merge(root)
        .fallback(|| async {
            OaError::new(
                404,
                "invalid_request_error",
                "not_found",
                "Unknown endpoint.",
            )
        })
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    Some(token.trim())
}

async fn require_key(State(shared): State<Arc<Shared>>, mut req: Request, next: Next) -> Response {
    let Some(caller) = bearer(req.headers()).and_then(|t| shared.backend.verify_key(t)) else {
        tracing::debug!(path = %req.uri().path(), "request without a valid key refused");
        return OaError::new(
            401,
            "invalid_request_error",
            "invalid_api_key",
            "Missing or wrong access key. Create one in Lokalomat › Serwer.",
        )
        .into_response();
    };
    {
        let mut rate = shared.rate.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let entry = rate.entry(caller.key_id.clone()).or_insert((now, 0));
        if now.duration_since(entry.0) >= Duration::from_secs(60) {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > RATE_LIMIT {
            return OaError::new(
                429,
                "rate_limit_error",
                "rate_limited",
                "Too many requests.",
            )
            .into_response();
        }
    }
    req.extensions_mut().insert(caller);
    let mut resp = next.run(req).await;
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

async fn models(State(shared): State<Arc<Shared>>) -> Response {
    let list = shared.all_models();
    let mut data: Vec<Value> = list
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "object": "model",
                "created": 0,
                "owned_by": "lokalomat",
                "name": m.name,
                "kind": m.slot,
            })
        })
        .collect();
    if let Some(d) = list.iter().find(|m| m.default && m.slot == Slot::Chat) {
        data.insert(
            0,
            json!({ "id": "default", "object": "model", "created": 0, "owned_by": "lokalomat", "name": d.name, "kind": Slot::Chat }),
        );
    }
    Json(json!({ "object": "list", "data": data })).into_response()
}

async fn chat_completions(
    State(shared): State<Arc<Shared>>,
    Extension(caller): Extension<Caller>,
    body: Bytes,
) -> Response {
    proxy(shared, caller, Slot::Chat, "/v1/chat/completions", body).await
}

async fn completions(
    State(shared): State<Arc<Shared>>,
    Extension(caller): Extension<Caller>,
    body: Bytes,
) -> Response {
    proxy(shared, caller, Slot::Chat, "/v1/completions", body).await
}

async fn systemone(
    State(shared): State<Arc<Shared>>,
    Extension(caller): Extension<Caller>,
    body: Bytes,
) -> Response {
    proxy(shared, caller, Slot::Decision, "/v1/systemone", body).await
}

// --- images, speech and transcription (ADR 0032) -----------------------------------

pub fn bad_request(code: &str, message: impl Into<String>) -> OaError {
    OaError::new(400, "invalid_request_error", code, message)
}

pub fn parse_json(body: &Bytes) -> Result<Value, OaError> {
    match serde_json::from_slice(body) {
        Ok(v @ Value::Object(_)) => Ok(v),
        _ => Err(bad_request(
            "invalid_json",
            "The body must be a JSON object.",
        )),
    }
}

pub fn text_field(json: &Value, name: &str) -> Option<String> {
    json.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Writes the log entry of a finished media request.
pub fn finish(
    shared: &Shared,
    caller: &Caller,
    path: &str,
    model: Option<String>,
    status: u16,
    started: Instant,
) {
    let ms = started.elapsed().as_millis() as u64;
    tracing::info!(path, status, key = %caller.name, ms, "server request answered");
    shared.record(LogEntry {
        at: lokalsi_core::db::now_secs(),
        key: caller.name.clone(),
        method: "POST".into(),
        path: path.to_owned(),
        model,
        status,
        duration_ms: ms,
        stream: false,
    });
}

/// `model` from the request: `None` for the default model.
pub fn requested_model(json: &Value) -> Option<String> {
    json.get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty() && *m != "default")
        .map(str::to_owned)
}

async fn proxy(
    shared: Arc<Shared>,
    caller: Caller,
    slot: Slot,
    path: &str,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            shared.record(LogEntry {
                at: lokalsi_core::db::now_secs(),
                key: caller.name.clone(),
                method: "POST".into(),
                path: path.to_owned(),
                model: None,
                status: 400,
                duration_ms: started.elapsed().as_millis() as u64,
                stream: false,
            });
            return OaError::new(
                400,
                "invalid_request_error",
                "invalid_json",
                "The body must be a JSON object.",
            )
            .into_response();
        }
    };
    let model = requested_model(&parsed);
    let stream = parsed
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut log = LogEntry {
        at: lokalsi_core::db::now_secs(),
        key: caller.name.clone(),
        method: "POST".into(),
        path: path.to_owned(),
        model: model.clone(),
        status: 0,
        duration_ms: 0,
        stream,
    };
    let cancel = CancellationToken::new();
    // A client that disconnects while the model loads drops this future,
    // and with it the guard that cancels the load.
    let _drop_guard = cancel.clone().drop_guard();
    let upstream = match shared.backend.upstream(slot, model, cancel.clone()).await {
        Ok(u) => u,
        Err(e) => {
            log.status = e.status;
            log.duration_ms = started.elapsed().as_millis() as u64;
            tracing::info!(path, status = e.status, code = %e.code, key = %caller.name, "server request refused");
            shared.record(log);
            return e.into_response();
        }
    };
    tracing::debug!(path, key = %caller.name, model = %upstream.model_name, stream, "server request forwarded");
    let sent = shared
        .http
        .post(format!("{}{path}", upstream.base_url))
        .bearer_auth(&upstream.token)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await;
    let resp = match sent {
        Ok(r) => r,
        Err(e) => {
            log.status = 502;
            log.duration_ms = started.elapsed().as_millis() as u64;
            shared.record(log);
            tracing::error!(error = %e, "engine did not answer a server request");
            return OaError::new(
                502,
                "api_error",
                "engine_error",
                "The engine did not answer.",
            )
            .into_response();
        }
    };
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    log.status = status;
    log.duration_ms = started.elapsed().as_millis() as u64;
    tracing::info!(path, status, key = %caller.name, model = %upstream.model_name, stream, ms = log.duration_ms, "server request answered");
    shared.record(log);
    // The slot stays reserved until the whole answer has been sent (or the
    // client went away and the body was dropped).
    let hold = upstream.hold;
    let body = resp.bytes_stream().map(move |chunk| {
        let _keep = &hold;
        chunk.map_err(std::io::Error::other)
    });
    let mut out = Response::new(Body::from_stream(body));
    *out.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    out.headers_mut().insert(header::CONTENT_TYPE, content_type);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fake `llama-server` answering chat completions with SSE.
    async fn fake_engine() -> (String, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                post(move |headers: HeaderMap, body: Bytes| {
                    let counter = Arc::clone(&counter);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(bearer(&headers), Some("engine-token"));
                        let req: Value = serde_json::from_slice(&body).unwrap();
                        assert!(req.get("tools").is_some(), "tools pass through");
                        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Cze\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"ść\"}}]}\n\ndata: [DONE]\n\n";
                        ([(header::CONTENT_TYPE, "text/event-stream")], sse)
                    }
                }),
            )
            .route(
                "/v1/systemone",
                post(|| async { (StatusCode::BAD_REQUEST, Json(json!({"error": {"message": "bad"}}))) }),
            );
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), hits)
    }

    struct Fake {
        engine: String,
        loads: AtomicUsize,
    }

    impl ServerBackend for Fake {
        fn verify_key(&self, token: &str) -> Option<Caller> {
            (token == "lk-test.secret").then(|| Caller {
                key_id: "test".into(),
                name: "Test".into(),
            })
        }

        fn models(&self) -> Vec<ServedModel> {
            vec![ServedModel {
                id: "bielik:q8_0".into(),
                name: "Bielik".into(),
                slot: Slot::Chat,
                default: true,
            }]
        }

        fn upstream(
            &self,
            slot: Slot,
            model: Option<String>,
            _cancel: CancellationToken,
        ) -> BoxFut<'_, Result<UpstreamHandle, OaError>> {
            Box::pin(async move {
                if model.as_deref().is_some_and(|m| m != "bielik:q8_0") {
                    return Err(OaError::new(
                        404,
                        "invalid_request_error",
                        "model_not_found",
                        "no such model",
                    ));
                }
                self.loads.fetch_add(1, Ordering::SeqCst);
                let _ = slot;
                Ok(UpstreamHandle {
                    base_url: self.engine.clone(),
                    token: "engine-token".into(),
                    model_name: "Bielik".into(),
                    hold: Box::new(()),
                })
            })
        }
    }

    #[tokio::test]
    async fn proxies_streams_and_checks_keys() {
        let (engine, hits) = fake_engine().await;
        let backend = Arc::new(Fake {
            engine,
            loads: AtomicUsize::new(0),
        });
        let server = OpenAiServer::start(
            &ServerConfig {
                bind: "127.0.0.1".parse().unwrap(),
                port: 0,
            },
            backend.clone(),
            None,
        )
        .await
        .unwrap();
        let base = format!("http://{}", server.addr());
        let http = reqwest::Client::new();
        let body = json!({"model": "bielik:q8_0", "stream": true, "messages": [{"role": "user", "content": "hej"}], "tools": [{"type": "function", "function": {"name": "f", "parameters": {}}}]});

        // No key → 401 in OpenAI shape, nothing loaded.
        let r = http
            .post(format!("{base}/v1/chat/completions"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        let e: Value = r.json().await.unwrap();
        assert_eq!(e["error"]["code"], "invalid_api_key");
        assert_eq!(backend.loads.load(Ordering::SeqCst), 0);

        // Health needs no key.
        assert_eq!(
            http.get(format!("{base}/health"))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );

        // Models in OpenAI list shape, with `default` first.
        let m: Value = http
            .get(format!("{base}/v1/models"))
            .bearer_auth("lk-test.secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(m["object"], "list");
        assert_eq!(m["data"][0]["id"], "default");
        assert_eq!(m["data"][1]["id"], "bielik:q8_0");

        // Streaming passes through unchanged.
        let r = http
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth("lk-test.secret")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()[header::CONTENT_TYPE], "text/event-stream");
        let text = r.text().await.unwrap();
        assert!(
            text.contains("Cze") && text.ends_with("data: [DONE]\n\n"),
            "{text}"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Unknown model → 404 from the backend; engine errors pass through.
        let mut other = body.clone();
        other["model"] = json!("gpt-4o");
        let r = http
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth("lk-test.secret")
            .json(&other)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404);
        let r = http
            .post(format!("{base}/v1/systemone"))
            .bearer_auth("lk-test.secret")
            .json(&json!({"state": "x"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        let r = http
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth("lk-test.secret")
            .body("not json")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);

        let log = server.log();
        assert_eq!(log.len(), 4, "{log:?}");
        assert!(log.iter().all(|l| l.key == "Test"));
        assert_eq!(log[0].status, 200);
        assert!(log[0].stream);
        server.stop();
    }
}

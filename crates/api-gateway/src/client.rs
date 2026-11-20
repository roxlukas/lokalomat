//! The phone side: pairing and calls to the computer over TLS pinned to the
//! certificate fingerprint from the QR code. One HTTP/1.1 connection per
//! request keeps it simple (a phone sends a request every few seconds at most).

use std::time::Duration;

use http_body_util::BodyExt as _;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

use crate::api::{
    ChatRequest, ChatResponse, DecisionJobRequest, ErrorBody, GalleryItem, ImageRequest,
    ImageResponse, Info, JobView, ModelOption, MusicJobRequest, PairRequest, PairResponse,
    SpeechRequest, Transcription, Turn, Voice, bearer,
};
use crate::pairing::{
    CONFIRM_TTL_SECS, DeviceIdentity, QrPayload, b64, comparison_code, signed_message, unb64,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Long jobs (image, first engine install) may take minutes.
const JOB_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// The computer is unreachable (off, other network, LAN sharing disabled).
    #[error("cannot reach the computer: {0}")]
    Connect(String),
    /// The phone's system refused the connection: Android 17+ needs the
    /// "devices on the local network" permission (ACCESS_LOCAL_NETWORK).
    #[error("local network access denied by the system: {0}")]
    LocalNetwork(String),
    /// The certificate differs from the paired one, or TLS failed.
    #[error("secure connection failed: {0}")]
    Tls(String),
    #[error("connection error: {0}")]
    Http(String),
    #[error("timed out")]
    Timeout,
    #[error("{code} ({status}): {detail}")]
    Api {
        status: u16,
        code: String,
        detail: String,
    },
    #[error("unexpected answer: {0}")]
    Decode(String),
}

/// TCP connect failure: a permission error means the phone's system blocked
/// the local network (Android 17+), not that the computer is away.
fn connect_error(e: std::io::Error) -> ClientError {
    tracing::warn!(error = %e, kind = ?e.kind(), "connecting to the computer failed");
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        ClientError::LocalNetwork(e.to_string())
    } else {
        ClientError::Connect(e.to_string())
    }
}

impl ClientError {
    /// Short code for the UI (`mobile.errors.*`).
    pub fn code(&self) -> String {
        match self {
            ClientError::Connect(_) => "unreachable".into(),
            ClientError::LocalNetwork(_) => "local_network".into(),
            ClientError::Tls(_) => "tls".into(),
            ClientError::Http(_) | ClientError::Decode(_) => "connection".into(),
            ClientError::Timeout => "timeout".into(),
            ClientError::Api { code, .. } => code.clone(),
        }
    }
}

/// What the phone keeps after pairing (the access key goes to the secret store).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub host: String,
    pub port: u16,
    /// Pinned certificate fingerprint (base64url).
    pub fingerprint: String,
    pub device_id: String,
    pub access_key: String,
    pub computer_name: String,
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub host: String,
    pub port: u16,
    pub fingerprint: [u8; 32],
    authorization: Option<String>,
}

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    /// `Cache-Control` of the answer (`no-store` for every `/v1/*` route).
    pub cache_control: String,
    pub body: Vec<u8>,
}

/// Percent-encodes a query value (model ids are plain, but stay safe).
fn query_value(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl Connection {
    pub fn unauthenticated(host: &str, port: u16, fingerprint: [u8; 32]) -> Self {
        Connection {
            host: host.to_owned(),
            port,
            fingerprint,
            authorization: None,
        }
    }

    pub fn from_credentials(c: &Credentials) -> Result<Self, ClientError> {
        let fingerprint = unb64(&c.fingerprint)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .ok_or_else(|| ClientError::Decode("fingerprint".into()))?;
        Ok(Connection {
            host: c.host.clone(),
            port: c.port,
            fingerprint,
            authorization: Some(bearer(&c.device_id, &c.access_key)),
        })
    }

    /// Replaces the credentials (tests: a wrong or revoked key).
    pub fn with_authorization(mut self, value: Option<String>) -> Self {
        self.authorization = value;
        self
    }

    /// One request; errors with a JSON body become [`ClientError::Api`].
    pub async fn send(
        &self,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
        timeout: Duration,
    ) -> Result<Reply, ClientError> {
        self.send_with(method, path, content_type, &[], body, timeout)
            .await
    }

    /// [`Connection::send`] with extra headers.
    pub async fn send_with(
        &self,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        headers: &[(&str, &str)],
        body: Vec<u8>,
        timeout: Duration,
    ) -> Result<Reply, ClientError> {
        tokio::time::timeout(
            timeout,
            self.send_inner(method, path, content_type, headers, body),
        )
        .await
        .map_err(|_| ClientError::Timeout)?
    }

    async fn send_inner(
        &self,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> Result<Reply, ClientError> {
        let tcp = tokio::time::timeout(
            CONNECT_TIMEOUT,
            TcpStream::connect((self.host.as_str(), self.port)),
        )
        .await
        .map_err(|_| ClientError::Connect("timeout".into()))?
        .map_err(connect_error)?;
        let config = crate::tls::client_config(self.fingerprint)
            .map_err(|e| ClientError::Tls(e.to_string()))?;
        let name = match self.host.parse::<std::net::IpAddr>() {
            Ok(ip) => ServerName::IpAddress(ip.into()),
            Err(_) => ServerName::try_from(self.host.clone())
                .map_err(|e| ClientError::Connect(e.to_string()))?,
        };
        let tls = tokio_rustls::TlsConnector::from(config)
            .connect(name, tcp)
            .await
            .map_err(|e| ClientError::Tls(e.to_string()))?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let mut req = hyper::Request::builder()
            .method(method)
            .uri(path)
            .header(hyper::header::HOST, format!("{}:{}", self.host, self.port));
        if let Some(ct) = content_type {
            req = req.header(hyper::header::CONTENT_TYPE, ct);
        }
        if let Some(auth) = &self.authorization {
            req = req.header(hyper::header::AUTHORIZATION, auth);
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let req = req
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| ClientError::Http(e.to_string()))?;
        let resp = sender
            .send_request(req)
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(hyper::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let cache_control = resp
            .headers()
            .get(hyper::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let body = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?
            .to_bytes()
            .to_vec();
        if !(200..300).contains(&status) {
            let err: Option<ErrorBody> = serde_json::from_slice(&body).ok();
            return Err(ClientError::Api {
                status,
                code: err
                    .as_ref()
                    .map(|e| e.error.clone())
                    .unwrap_or_else(|| "error".into()),
                detail: err.map(|e| e.detail).unwrap_or_default(),
            });
        }
        Ok(Reply {
            status,
            content_type,
            cache_control,
            body,
        })
    }

    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&impl Serialize>,
        timeout: Duration,
    ) -> Result<T, ClientError> {
        let bytes = match body {
            Some(b) => serde_json::to_vec(b).map_err(|e| ClientError::Decode(e.to_string()))?,
            None => Vec::new(),
        };
        let reply = self
            .send(
                method,
                path,
                body.map(|_| "application/json"),
                bytes,
                timeout,
            )
            .await?;
        serde_json::from_slice(&reply.body).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn info(&self) -> Result<Info, ClientError> {
        self.json("GET", "/v1/info", None::<&()>, CONNECT_TIMEOUT * 2)
            .await
    }

    /// Models installed on the computer for `mode` (`chat`, `stt`, `tts`, `image`).
    pub async fn models(&self, mode: &str) -> Result<Vec<ModelOption>, ClientError> {
        self.json(
            "GET",
            &format!("/v1/models?mode={}", query_value(mode)),
            None::<&()>,
            CONNECT_TIMEOUT * 2,
        )
        .await
    }

    /// `model`: a [`ModelOption::id`]; `None` = the computer's default.
    pub async fn chat(
        &self,
        messages: Vec<Turn>,
        model: Option<&str>,
    ) -> Result<String, ClientError> {
        let resp: ChatResponse = self
            .json(
                "POST",
                "/v1/chat/completions",
                Some(&ChatRequest {
                    messages,
                    model: model.map(str::to_owned),
                }),
                JOB_TIMEOUT,
            )
            .await?;
        resp.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| ClientError::Decode("no choices".into()))
    }

    /// `ext`: the recording's file extension (`m4a`, `webm`, `wav`).
    pub async fn transcribe(
        &self,
        audio: Vec<u8>,
        ext: &str,
        language: Option<&str>,
        model: Option<&str>,
    ) -> Result<Transcription, ClientError> {
        let ext: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let mut path = format!("/v1/audio/transcriptions?ext={ext}");
        if let Some(l) = language.filter(|l| l.chars().all(|c| c.is_ascii_alphabetic())) {
            path.push_str(&format!("&language={l}"));
        }
        if let Some(m) = model.filter(|m| !m.is_empty()) {
            path.push_str(&format!("&model={}", query_value(m)));
        }
        let reply = self
            .send(
                "POST",
                &path,
                Some("application/octet-stream"),
                audio,
                JOB_TIMEOUT,
            )
            .await?;
        serde_json::from_slice(&reply.body).map_err(|e| ClientError::Decode(e.to_string()))
    }

    /// WAV bytes.
    pub async fn speak(&self, req: &SpeechRequest) -> Result<Vec<u8>, ClientError> {
        let body = serde_json::to_vec(req).map_err(|e| ClientError::Decode(e.to_string()))?;
        Ok(self
            .send(
                "POST",
                "/v1/audio/speech",
                Some("application/json"),
                body,
                JOB_TIMEOUT,
            )
            .await?
            .body)
    }

    /// PNG images as base64.
    pub async fn image(&self, req: &ImageRequest) -> Result<Vec<String>, ClientError> {
        let resp: ImageResponse = self
            .json("POST", "/v1/images/generations", Some(req), JOB_TIMEOUT)
            .await?;
        Ok(resp.data.into_iter().map(|d| d.b64_json).collect())
    }

    /// Starts an image job on the computer (ADR 0027); poll it with [`Connection::job`].
    pub async fn start_image(&self, req: &ImageRequest) -> Result<JobView, ClientError> {
        self.json("POST", "/v1/jobs/image", Some(req), CONNECT_TIMEOUT * 3)
            .await
    }

    /// Starts a song job (ADR 0022).
    pub async fn start_music(&self, req: &MusicJobRequest) -> Result<JobView, ClientError> {
        self.json("POST", "/v1/jobs/music", Some(req), CONNECT_TIMEOUT * 3)
            .await
    }

    /// Starts a decision job (ADR 0024; `403 expert_only` outside Expert mode).
    pub async fn start_decision(&self, req: &DecisionJobRequest) -> Result<JobView, ClientError> {
        self.json("POST", "/v1/jobs/decision", Some(req), CONNECT_TIMEOUT * 3)
            .await
    }

    /// A job this phone started (`404 not_found` for any other).
    pub async fn job(&self, job_id: i64) -> Result<JobView, ClientError> {
        self.json(
            "GET",
            &format!("/v1/jobs/{job_id}"),
            None::<&()>,
            CONNECT_TIMEOUT * 2,
        )
        .await
    }

    pub async fn cancel_job(&self, job_id: i64) -> Result<JobView, ClientError> {
        self.json(
            "POST",
            &format!("/v1/jobs/{job_id}/cancel"),
            None::<&()>,
            CONNECT_TIMEOUT * 2,
        )
        .await
    }

    pub async fn voices(&self) -> Result<Vec<Voice>, ClientError> {
        self.json("GET", "/v1/voices", None::<&()>, CONNECT_TIMEOUT * 2)
            .await
    }

    pub async fn gallery(&self, limit: u32) -> Result<Vec<GalleryItem>, ClientError> {
        self.json(
            "GET",
            &format!("/v1/gallery?limit={limit}"),
            None::<&()>,
            CONNECT_TIMEOUT * 2,
        )
        .await
    }

    /// Small JPEG of an image result (`404 not_found` for audio).
    pub async fn gallery_thumb(&self, job_id: i64, index: usize) -> Result<Reply, ClientError> {
        self.send(
            "GET",
            &format!("/v1/gallery/{job_id}/files/{index}/thumb"),
            None,
            Vec::new(),
            Duration::from_secs(60),
        )
        .await
    }

    pub async fn gallery_file(&self, job_id: i64, index: usize) -> Result<Reply, ClientError> {
        self.send(
            "GET",
            &format!("/v1/gallery/{job_id}/files/{index}"),
            None,
            Vec::new(),
            Duration::from_secs(120),
        )
        .await
    }
}

/// The code the phone shows while the user compares it with the computer.
pub fn code_for(payload: &QrPayload, identity: &DeviceIdentity) -> String {
    comparison_code(&payload.fingerprint, &identity.public_key(), &payload.token)
}

/// Sends the phone's key and waits until the user confirms (or rejects) the
/// comparison code on the computer.
pub async fn pair(
    payload: &QrPayload,
    identity: &DeviceIdentity,
    device_name: &str,
) -> Result<Credentials, ClientError> {
    let conn = Connection::unauthenticated(&payload.host, payload.port, payload.fingerprint);
    let req = PairRequest {
        token: payload.token.clone(),
        device_name: device_name.to_owned(),
        public_key: b64(&identity.public_key()),
        signature: b64(&identity.sign(&signed_message(&payload.token, &payload.fingerprint))),
    };
    let resp: PairResponse = conn
        .json(
            "POST",
            "/pair",
            Some(&req),
            Duration::from_secs(CONFIRM_TTL_SECS + 30),
        )
        .await?;
    Ok(Credentials {
        host: payload.host.clone(),
        port: payload.port,
        fingerprint: b64(&payload.fingerprint),
        device_id: resp.device_id,
        access_key: resp.access_key,
        computer_name: resp.computer_name,
    })
}

#[cfg(test)]
mod tests {
    use super::{ClientError, connect_error};
    use std::io::{Error, ErrorKind};

    #[test]
    fn blocked_local_network_has_its_own_code() {
        let e = connect_error(Error::from(ErrorKind::PermissionDenied));
        assert!(matches!(e, ClientError::LocalNetwork(_)));
        assert_eq!(e.code(), "local_network");
    }

    #[test]
    fn other_connect_errors_mean_unreachable() {
        let e = connect_error(Error::from(ErrorKind::ConnectionRefused));
        assert_eq!(e.code(), "unreachable");
    }
}

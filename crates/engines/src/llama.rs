//! llama.cpp adapter: `llama-server` process supervision and the
//! OpenAI-compatible streaming chat client.
//!
//! The server listens only on 127.0.0.1, on a random port, and requires a
//! random token (PRD §10). Its stdout/stderr go to `engine-llama-cpp.log`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::manifest::Launch;

/// How long a model may take to load before the start is considered failed.
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("cannot start the engine: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("the engine stopped while loading the model: {0}")]
    Exited(String),
    #[error("the model did not load within {0} s")]
    Timeout(u64),
    #[error("start cancelled")]
    Cancelled,
    #[error(transparent)]
    MissingLibrary(crate::syslibs::MissingLibrary),
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub binary: PathBuf,
    pub model_path: PathBuf,
    pub ctx: u32,
    /// Flash attention and a q8_0 KV cache (ADR 0021).
    pub kv_compact: bool,
    /// `-ngl`: layers on the GPU set in the Inspector (ADR 0036); `None` = the engine decides.
    pub gpu_layers: Option<u32>,
    pub log_file: PathBuf,
    pub launch: Launch,
}

/// `llama-server` arguments for the compact KV cache: q8_0 keys and values
/// take about half the memory of f16 with a barely measurable quality loss;
/// a quantised V cache needs flash attention.
pub const KV_COMPACT_ARGS: [&str; 6] = ["-fa", "on", "-ctk", "q8_0", "-ctv", "q8_0"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitInfo {
    /// Exit code, `None` when killed by a signal.
    pub code: Option<i32>,
    /// True when we stopped it on purpose.
    pub requested: bool,
}

/// A running `llama-server`. Dropping it kills the process.
pub struct RunningServer {
    pub port: u16,
    token: String,
    pub model_path: PathBuf,
    pub started: Instant,
    kill: Option<oneshot::Sender<()>>,
    exit: watch::Receiver<Option<ExitInfo>>,
}

impl RunningServer {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// `Some` once the process has exited (crash or stop).
    pub fn exit_info(&self) -> Option<ExitInfo> {
        self.exit.borrow().clone()
    }

    /// Resolves when the process exits for any reason.
    pub fn exited(&self) -> watch::Receiver<Option<ExitInfo>> {
        self.exit.clone()
    }

    /// Kills the process and waits (briefly) until it is gone.
    pub async fn stop(mut self) {
        if let Some(kill) = self.kill.take() {
            let _ = kill.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(10), self.exit.wait_for(|e| e.is_some()))
            .await;
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(kill) = self.kill.take() {
            let _ = kill.send(());
        }
    }
}

pub fn random_token() -> String {
    let mut buf = [0u8; 24];
    getrandom::fill(&mut buf).expect("OS random generator available");
    hex::encode(buf)
}

pub fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}

fn substitute(args: &[String], model: &Path, port: u16, token: &str, ctx: u32) -> Vec<String> {
    args.iter()
        .map(|a| {
            a.replace("{model_path}", &model.to_string_lossy())
                .replace("{port}", &port.to_string())
                .replace("{token}", token)
                .replace("{ctx}", &ctx.to_string())
        })
        .collect()
}

/// Command line of `llama-server` for `config`.
fn launch_args(config: &ServerConfig, port: u16, token: &str) -> Vec<String> {
    let mut args = substitute(
        &config.launch.args,
        &config.model_path,
        port,
        token,
        config.ctx,
    );
    if config.kv_compact {
        args.extend(KV_COMPACT_ARGS.iter().map(|a| (*a).to_owned()));
    }
    if let Some(n) = config.gpu_layers {
        args.extend(["-ngl".to_owned(), n.to_string()]);
    }
    args
}

/// Last `max` lines of the engine log, for error messages.
pub fn log_tail(path: &Path, max: usize) -> String {
    let Ok(mut f) = File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(16 * 1024)));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(max)..].join("\n")
}

/// Starts `llama-server` and waits until `/health` reports the model loaded.
pub async fn start(
    config: &ServerConfig,
    client: &reqwest::Client,
    cancel: &CancellationToken,
) -> Result<RunningServer, StartError> {
    let port = free_port()?;
    let token = random_token();
    let args = launch_args(config, port, &token);
    if let Some(parent) = config.log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log = File::create(&config.log_file)?;
    let log_err = log.try_clone()?;

    let mut cmd = tokio::process::Command::new(&config.binary);
    cmd.args(&args)
        .current_dir(config.binary.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    tracing::info!(binary = %config.binary.display(), model = %config.model_path.display(), port, ctx = config.ctx, kv_compact = config.kv_compact, gpu_layers = ?config.gpu_layers, "starting llama-server");
    tracing::debug!(args = ?lokalsi_core::redact::redact(&args.join(" ")), "llama-server arguments");
    let mut child = cmd.spawn()?;
    let pid = child.id();

    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let (exit_tx, exit_rx) = watch::channel::<Option<ExitInfo>>(None);
    tokio::spawn(async move {
        let info = tokio::select! {
            status = child.wait() => ExitInfo { code: status.ok().and_then(|s| s.code()), requested: false },
            _ = kill_rx => {
                let _ = child.kill().await;
                ExitInfo { code: None, requested: true }
            }
        };
        if info.requested {
            tracing::info!(?pid, "llama-server stopped");
        } else {
            tracing::warn!(?pid, code = ?info.code, "llama-server exited unexpectedly");
        }
        let _ = exit_tx.send(Some(info));
    });

    let server = RunningServer {
        port,
        token,
        model_path: config.model_path.clone(),
        started: Instant::now(),
        kill: Some(kill_tx),
        exit: exit_rx,
    };

    let health = format!("{}/health", server.base_url());
    let deadline = Instant::now() + LOAD_TIMEOUT;
    loop {
        if let Some(info) = server.exit_info() {
            let tail = log_tail(&config.log_file, 12);
            tracing::error!(code = ?info.code, log = %tail, "llama-server failed to start");
            if let Some(missing) = crate::syslibs::detect(&tail) {
                return Err(StartError::MissingLibrary(missing));
            }
            return Err(StartError::Exited(
                last_error_line(&tail).unwrap_or_else(|| format!("exit code {:?}", info.code)),
            ));
        }
        if Instant::now() > deadline {
            server.stop().await;
            return Err(StartError::Timeout(LOAD_TIMEOUT.as_secs()));
        }
        if cancel.is_cancelled() {
            server.stop().await;
            return Err(StartError::Cancelled);
        }
        if let Ok(resp) = client
            .get(&health)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            && resp.status().is_success()
        {
            tracing::info!(
                port,
                load_ms = server.started.elapsed().as_millis() as u64,
                "model loaded"
            );
            return Ok(server);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn last_error_line(tail: &str) -> Option<String> {
    tail.lines()
        .rev()
        .find(|l| {
            let l = l.to_lowercase();
            l.contains("error") || l.contains("failed") || l.contains("unable")
        })
        .map(|l| l.trim().chars().take(300).collect())
}

// --- chat -----------------------------------------------------------------------

/// One message of the OpenAI-style conversation sent to `llama-server`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Assistant turn that asked for tools: `[{id, type, function: {name, arguments}}]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    /// Tool result turn: the call it answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        ChatMessage {
            role: role.into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant_tool_calls(content: &str, calls: &[ToolCall]) -> Self {
        ChatMessage {
            role: "assistant".into(),
            content: (!content.is_empty()).then(|| content.to_owned()),
            tool_calls: Some(serde_json::Value::Array(
                calls
                    .iter()
                    .map(|c| serde_json::json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments } }))
                    .collect(),
            )),
            tool_call_id: None,
        }
    }

    pub fn tool_result(call_id: &str, content: impl Into<String>) -> Self {
        ChatMessage {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
        }
    }
}

/// A complete tool call requested by the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON text as produced by the model.
    pub arguments: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl ChatParams {
    /// Fills the fields this request leaves open with the model's defaults
    /// from the Inspector (ADR 0036); values set by the caller win.
    pub fn with_model_defaults(mut self, d: &lokalsi_core::model_params::ModelParams) -> Self {
        self.temperature = self.temperature.or(d.temperature);
        self.top_p = self.top_p.or(d.top_p);
        self.top_k = self.top_k.or(d.top_k);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatOutcome {
    pub content: String,
    pub reasoning: String,
    pub tokens: Option<i64>,
    pub tokens_per_second: Option<f64>,
    pub cancelled: bool,
    /// Tools the model wants to call before it can answer.
    pub tool_calls: Vec<ToolCall>,
    /// Why the engine stopped: `stop` (model ended), `length` (token limit or
    /// full context window), `tool_calls`.
    pub finish_reason: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("connection to the engine failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("the engine returned HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("the engine stopped responding mid-answer")]
    Interrupted,
}

impl ChatError {
    /// llama-server rejected the prompt as longer than the context window.
    pub fn is_context_full(&self) -> bool {
        matches!(self, Self::Status { status: 400, body } if body.contains("exceed_context_size"))
    }
}

/// Failure of a `/v1/systemone` request (ADR 0024).
#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    #[error("connection to the engine failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("the engine returned HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("unexpected answer from the engine: {0}")]
    Parse(String),
}

impl DecideError {
    /// Stable code for the UI (`errors.*`).
    pub fn code(&self) -> &'static str {
        match self {
            // 501: the model has no decision metadata (or images unsupported).
            DecideError::Status { status: 501, .. } => {
                crate::service::error_codes::NOT_DECISION_MODEL
            }
            DecideError::Status { status: 400, body } if body.contains("exceed_context_size") => {
                crate::service::error_codes::CONTEXT_FULL
            }
            DecideError::Status { status: 400, .. } => {
                crate::service::error_codes::DECISION_INVALID
            }
            _ => crate::service::error_codes::ENGINE_ERROR,
        }
    }
}

/// Typed decision at a running `llama-server` with a decision model
/// (`POST /v1/systemone`, the TypeSafe System One API, ADR 0024).
pub async fn decide_at(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    state: &str,
    questions: &[lokalsi_core::decisions::Question],
) -> Result<lokalsi_core::decisions::Decision, DecideError> {
    let body = lokalsi_core::decisions::systemone_request(state, questions);
    let resp = client
        .post(format!("{base_url}/v1/systemone"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(DecideError::Status {
            status: status.as_u16(),
            body: body.chars().take(500).collect(),
        });
    }
    let value: serde_json::Value = resp.json().await?;
    lokalsi_core::decisions::parse_systemone(questions, &value).map_err(DecideError::Parse)
}

/// One parsed SSE event of a streamed chat completion.
#[derive(Debug, Default, PartialEq)]
pub struct Delta {
    pub content: String,
    pub reasoning: String,
    pub done: bool,
    pub tokens: Option<i64>,
    pub tokens_per_second: Option<f64>,
    /// `(index, id, name fragment, arguments fragment)` – streamed in pieces.
    pub tool_calls: Vec<(usize, Option<String>, String, String)>,
    pub finish_reason: Option<String>,
}

/// Parses one `data:` payload of `/v1/chat/completions` with `stream: true`.
pub fn parse_event(data: &str) -> Option<Delta> {
    let data = data.trim();
    if data == "[DONE]" {
        return Some(Delta {
            done: true,
            ..Default::default()
        });
    }
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let choice = &v["choices"][0];
    let delta = &choice["delta"];
    Some(Delta {
        content: delta["content"].as_str().unwrap_or_default().to_owned(),
        reasoning: delta["reasoning_content"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        done: false,
        tokens: v["timings"]["predicted_n"]
            .as_i64()
            .or_else(|| v["usage"]["completion_tokens"].as_i64()),
        tokens_per_second: v["timings"]["predicted_per_second"].as_f64(),
        finish_reason: choice["finish_reason"].as_str().map(str::to_owned),
        tool_calls: delta["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .map(|c| {
                        (
                            c["index"].as_u64().unwrap_or(0) as usize,
                            c["id"].as_str().map(str::to_owned),
                            c["function"]["name"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            c["function"]["arguments"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Pulls `<tool_call>{"name": …, "arguments": {…}}</tool_call>` blocks out of
/// plain text. Returns the text without them and the parsed calls; blocks that
/// are not valid JSON with a `name` stay in the text.
pub fn extract_text_tool_calls(text: &str) -> (String, Vec<ToolCall>) {
    const OPEN: &str = "<tool_call>";
    const CLOSE: &str = "</tool_call>";
    let mut rest = String::new();
    let mut calls = Vec::new();
    let mut tail = text;
    while let Some(start) = tail.find(OPEN) {
        let after = &tail[start + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else { break };
        let block = after[..end].trim();
        let parsed = serde_json::from_str::<serde_json::Value>(block)
            .ok()
            .and_then(|v| {
                let name = v["name"].as_str()?.to_owned();
                let args = match &v["arguments"] {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => "{}".to_owned(),
                    other => other.to_string(),
                };
                Some((name, args))
            });
        match parsed {
            Some((name, arguments)) => {
                rest.push_str(&tail[..start]);
                calls.push(ToolCall {
                    id: format!("call_text_{}", calls.len()),
                    name,
                    arguments,
                });
            }
            None => rest.push_str(&tail[..start + OPEN.len() + end + CLOSE.len()]),
        }
        tail = &after[end + CLOSE.len()..];
    }
    rest.push_str(tail);
    (rest.trim().to_owned(), calls)
}

/// Streams a chat completion. `on_delta(content, reasoning)` gets every
/// fragment. Cancelling drops the HTTP stream, which makes `llama-server`
/// stop generating (PRD §8.1); partial text is returned with `cancelled`.
#[allow(clippy::too_many_arguments)] // mirrors the request fields; a struct would only add noise
pub async fn stream_chat_at(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    params: &ChatParams,
    cancel: &CancellationToken,
    on_delta: &mut (dyn FnMut(&str, &str) + Send),
) -> Result<ChatOutcome, ChatError> {
    let mut body = serde_json::json!({
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !tools.is_empty() {
        body["tools"] = serde_json::Value::Array(tools.to_vec());
    }
    if let (Some(obj), serde_json::Value::Object(p)) = (
        body.as_object_mut(),
        serde_json::to_value(params).unwrap_or_default(),
    ) {
        obj.extend(p);
    }
    let req = client
        .post(format!("{base_url}/v1/chat/completions"))
        .bearer_auth(token)
        .json(&body);
    let resp = tokio::select! {
        r = req.send() => r?,
        _ = cancel.cancelled() => return Ok(ChatOutcome { cancelled: true, ..Default::default() }),
    };
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(500)
            .collect();
        return Err(ChatError::Status { status, body });
    }

    let mut out = ChatOutcome::default();
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    loop {
        let chunk = tokio::select! {
            c = stream.next() => c,
            _ = cancel.cancelled() => {
                out.cancelled = true;
                return Ok(out);
            }
        };
        let Some(chunk) = chunk else {
            // Stream ended without [DONE]: the engine died or cut the connection.
            return Err(ChatError::Interrupted);
        };
        let chunk = chunk.map_err(|_| ChatError::Interrupted)?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let Some(data) = line.trim_end().strip_prefix("data:") else {
                continue;
            };
            let Some(delta) = parse_event(data) else {
                continue;
            };
            if delta.done {
                out.tool_calls.retain(|t| !t.name.is_empty());
                if !tools.is_empty() && out.tool_calls.is_empty() {
                    // Fallback: the chat template was not recognised by llama-server
                    // and the call arrived as text (Hermes / Bielik `<tool_call>`).
                    let (rest, calls) = extract_text_tool_calls(&out.content);
                    if !calls.is_empty() {
                        tracing::debug!(
                            count = calls.len(),
                            "tool calls recovered from text output"
                        );
                        out.content = rest;
                        out.tool_calls = calls;
                    }
                }
                return Ok(out);
            }
            for (index, id, name, args) in delta.tool_calls {
                while out.tool_calls.len() <= index {
                    out.tool_calls.push(ToolCall {
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                }
                let call = &mut out.tool_calls[index];
                if let Some(id) = id {
                    call.id = id;
                }
                call.name.push_str(&name);
                call.arguments.push_str(&args);
            }
            if !delta.content.is_empty() || !delta.reasoning.is_empty() {
                out.content.push_str(&delta.content);
                out.reasoning.push_str(&delta.reasoning);
                on_delta(&delta.content, &delta.reasoning);
            }
            if delta.tokens.is_some() {
                out.tokens = delta.tokens;
            }
            if delta.tokens_per_second.is_some() {
                out.tokens_per_second = delta.tokens_per_second;
            }
            if delta.finish_reason.is_some() {
                out.finish_reason = delta.finish_reason;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_defaults_fill_only_open_fields() {
        let defaults = lokalsi_core::model_params::ModelParams {
            temperature: Some(0.2),
            top_p: Some(0.9),
            top_k: Some(40),
            gpu_layers: None,
        };
        let caller = ChatParams {
            temperature: Some(1.0),
            ..Default::default()
        };
        let p = caller.with_model_defaults(&defaults);
        assert_eq!(p.temperature, Some(1.0), "the caller's value wins");
        assert_eq!(p.top_p, Some(0.9));
        assert_eq!(p.top_k, Some(40));
        let auto = ChatParams::default().with_model_defaults(&Default::default());
        assert_eq!(
            serde_json::to_value(&auto).unwrap(),
            serde_json::json!({}),
            "Auto sends nothing"
        );
    }

    #[test]
    fn parses_stream_events() {
        let d = parse_event(r#"{"choices":[{"delta":{"content":"Cześć"}}]}"#).unwrap();
        assert_eq!(d.content, "Cześć");
        let r = parse_event(r#"{"choices":[{"delta":{"reasoning_content":"hmm"}}]}"#).unwrap();
        assert_eq!(r.reasoning, "hmm");
        let t = parse_event(r#"{"choices":[{"delta":{},"finish_reason":"stop"}],"timings":{"predicted_n":42,"predicted_per_second":55.5}}"#).unwrap();
        assert_eq!(t.tokens, Some(42));
        assert_eq!(t.tokens_per_second, Some(55.5));
        assert_eq!(t.finish_reason.as_deref(), Some("stop"));
        assert!(parse_event(" [DONE]").unwrap().done);
        assert!(parse_event("not json").is_none());
    }

    #[test]
    fn parses_streamed_tool_calls() {
        let d = parse_event(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"grafana__search","arguments":"{\"q"}}]}}]}"#).unwrap();
        assert_eq!(
            d.tool_calls,
            vec![(
                0,
                Some("call_1".into()),
                "grafana__search".into(),
                "{\"q".into()
            )]
        );
    }

    #[test]
    fn recovers_tool_calls_from_text() {
        let text = "Sprawdzę.\n<tool_call>{\"name\": \"grafana__search_dashboards\", \"arguments\": {\"query\": \"\"}}</tool_call>";
        let (rest, calls) = extract_text_tool_calls(text);
        assert_eq!(rest, "Sprawdzę.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "grafana__search_dashboards");
        assert_eq!(calls[0].arguments, r#"{"query":""}"#);

        let (rest, calls) = extract_text_tool_calls("<tool_call>not json</tool_call> ok");
        assert!(calls.is_empty());
        assert_eq!(rest, "<tool_call>not json</tool_call> ok");
        assert!(extract_text_tool_calls("plain").1.is_empty());
    }

    #[test]
    fn substitutes_launch_placeholders() {
        let args = vec![
            "-m".into(),
            "{model_path}".into(),
            "--port".into(),
            "{port}".into(),
            "--api-key".into(),
            "{token}".into(),
            "-c".into(),
            "{ctx}".into(),
        ];
        let out = substitute(&args, Path::new("/m/x.gguf"), 4242, "tok", 8192);
        assert_eq!(
            out,
            [
                "-m",
                "/m/x.gguf",
                "--port",
                "4242",
                "--api-key",
                "tok",
                "-c",
                "8192"
            ]
        );
    }

    #[test]
    fn compact_kv_adds_flash_attention_and_q8_cache() {
        let mut config = ServerConfig {
            binary: PathBuf::from("llama-server"),
            model_path: PathBuf::from("/m/x.gguf"),
            ctx: 8192,
            kv_compact: false,
            gpu_layers: None,
            log_file: PathBuf::from("log"),
            launch: crate::manifest::EngineManifest::llama_cpp().launch,
        };
        let plain = launch_args(&config, 1, "t");
        assert!(!plain.iter().any(|a| a == "-ctk"));
        assert!(
            !plain.iter().any(|a| a == "-ngl"),
            "Auto leaves it to the engine"
        );
        config.kv_compact = true;
        let compact = launch_args(&config, 1, "t");
        assert_eq!(compact[plain.len()..], KV_COMPACT_ARGS.map(String::from));
        config.gpu_layers = Some(0);
        let cpu = launch_args(&config, 1, "t");
        assert_eq!(cpu[compact.len()..], ["-ngl", "0"]);
    }

    #[test]
    fn finds_error_lines_in_log() {
        let tail = "load: ok\nllama_model_load: error loading model: invalid magic\nmain: exiting";
        assert_eq!(
            last_error_line(tail).unwrap(),
            "llama_model_load: error loading model: invalid magic"
        );
    }

    #[test]
    fn tokens_are_random() {
        assert_ne!(random_token(), random_token());
        assert_eq!(random_token().len(), 48);
    }
}

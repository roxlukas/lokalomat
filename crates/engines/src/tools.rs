//! Tools for the chat model (M2b). The engine crate does not know where tools
//! come from; the app plugs in an MCP-backed [`ToolHost`].

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Maximum model ↔ tool rounds in one answer (guards against loops).
pub const MAX_TOOL_ROUNDS: usize = 6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    /// Unique name the model sees, e.g. `grafana__search_dashboards`.
    pub function_name: String,
    pub server_id: i64,
    pub server_name: String,
    pub tool: String,
    pub description: String,
    pub parameters: serde_json::Value,
    /// `true` = ask the user before every call; `false` = always allowed.
    pub ask: bool,
    /// The tool sends data off this device.
    pub remote: bool,
}

impl ToolDef {
    /// OpenAI `tools` entry for `/v1/chat/completions`.
    pub fn to_openai(&self) -> serde_json::Value {
        let params = match &self.parameters {
            serde_json::Value::Object(o) if !o.is_empty() => self.parameters.clone(),
            _ => serde_json::json!({ "type": "object", "properties": {} }),
        };
        serde_json::json!({
            "type": "function",
            "function": { "name": self.function_name, "description": self.description, "parameters": params }
        })
    }
}

pub trait ToolHost: Send + Sync + 'static {
    /// Tools of all enabled servers (blocked tools left out).
    fn tools(&self) -> BoxFuture<'_, Vec<ToolDef>>;
    /// Runs a tool; `Ok((text, is_error))` or a transport error.
    fn call<'a>(
        &'a self,
        def: &'a ToolDef,
        arguments: serde_json::Value,
    ) -> BoxFuture<'a, Result<(String, bool), String>>;
    /// "Always allow" chosen in the consent prompt.
    fn remember_allow(&self, def: &ToolDef);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// Waiting for the user's consent.
    Pending,
    Running,
    Done,
    Error,
    Denied,
}

/// One tool call shown in the answer card and stored on the message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub function_name: String,
    pub server_name: String,
    pub tool: String,
    pub remote: bool,
    pub arguments: String,
    pub status: ToolCallStatus,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

/// The user's answer to a consent prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub allow: bool,
    /// Do not ask again for this tool.
    pub remember: bool,
}

/// What the model gets when a call cannot run.
pub fn denied_result() -> String {
    "The user did not allow this tool call. Answer without it.".to_owned()
}

/// Keeps results short in the UI record (the model gets the full text).
pub fn preview(text: &str) -> String {
    const MAX: usize = 4000;
    match text.char_indices().nth(MAX) {
        Some((idx, _)) => format!("{}…", &text[..idx]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_tool_entry_has_an_object_schema() {
        let def = ToolDef {
            function_name: "g__t".into(),
            server_id: 1,
            server_name: "g".into(),
            tool: "t".into(),
            description: "d".into(),
            parameters: serde_json::json!({}),
            ask: true,
            remote: false,
        };
        let v = def.to_openai();
        assert_eq!(v["function"]["parameters"]["type"], "object");
        assert_eq!(v["function"]["name"], "g__t");
    }

    #[test]
    fn previews_are_bounded() {
        assert_eq!(preview("abc"), "abc");
        assert!(preview(&"x".repeat(5000)).chars().count() <= 4001);
    }
}

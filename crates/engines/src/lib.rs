//! Engine adapters (PRD §4.3): llama.cpp (chat server), the engine installer,
//! the VRAM scheduler and the job queue. The media engines (whisper.cpp,
//! stable-diffusion.cpp, the Python workers) are in `lokalsi-engines-media`
//! (Pro edition, ADR 0035).

pub mod cli;
pub mod install;
pub mod llama;
pub mod manifest;
pub mod modes;
pub mod queue;
pub mod scheduler;
pub mod service;
pub mod syslibs;
pub mod tools;

use serde::{Deserialize, Serialize};

/// What an engine can do (`capabilities` in `engine.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Chat,
    Embeddings,
    Image,
    Stt,
    Tts,
    VoiceCloning,
    /// Songs (ADR 0022).
    Music,
    /// MCP tool server (M2b).
    Tools,
}

/// How an engine is run (`kind` in `engine.json`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    #[default]
    Native,
    Python,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_use_manifest_spelling() {
        let caps: Vec<Capability> = serde_json::from_str(r#"["chat","voice_cloning"]"#).unwrap();
        assert_eq!(caps, vec![Capability::Chat, Capability::VoiceCloning]);
    }
}

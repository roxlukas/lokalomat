//! Lokalomat AI Studio core library.
//!
//! Everything here is testable without Tauri; the desktop shell only maps
//! these functions onto IPC commands and events.

pub mod api_keys;
pub mod benchmark;
pub mod catalog;
pub mod chats;
pub mod db;
pub mod decisions;
pub mod devices;
pub mod diagnostics;
pub mod downloads;
pub mod edition;
pub mod hf;
pub mod jobs;
pub mod library;
pub mod logging;
pub mod mcp_clients;
pub mod paths;
pub mod pipelines;
pub mod redact;
pub mod secrets;
pub mod settings;
pub mod speed;
pub mod transcript;
pub mod voices;

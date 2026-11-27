//! User preferences persisted as JSON in the application home.
//!
//! M0 keeps them in `settings.json` because the log mode must be known before
//! anything else starts. From M1 the rest of the state lives in SQLite
//! (see docs/adr/0005-settings-file.md).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::logging::LogMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Pl,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Which key sends a chat prompt (ADR 0036).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SendKey {
    /// Enter adds a new line, Ctrl/⌘+Enter sends (PRD V2).
    #[default]
    CtrlEnter,
    /// Enter sends, Shift+Enter adds a new line.
    Enter,
}

/// Default port of the LAN gateway (`lokalsi_api_gateway::DEFAULT_LAN_PORT`).
pub const DEFAULT_LAN_PORT: u16 = 47_821;
/// Default port of the local OpenAI-compatible server (ADR 0025).
pub const DEFAULT_SERVER_PORT: u16 = 47_822;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub language: Language,
    pub theme: Theme,
    pub log_mode: LogMode,
    /// Write prompt/result contents to logs. Only honoured in verbose mode.
    pub log_prompts: bool,
    /// Where models and outputs live (chosen in the first-run wizard).
    pub data_dir: Option<PathBuf>,
    /// First-run wizard finished.
    pub onboarded: bool,
    /// Download speed limit in KiB/s; `None` = unlimited.
    pub bandwidth_limit_kbps: Option<u32>,
    /// Share of RAM a unified-memory GPU may use (PRD §5, default 70%).
    pub unified_memory_percent: u8,
    /// Expert mode shows advanced options; Basic (default) hides them (PRD §15).
    pub expert: bool,
    /// Context window in tokens; `None` = automatic (fits free GPU memory),
    /// `Some(0)` = the model's own maximum (ADR 0042).
    pub context_size: Option<u32>,
    /// Keep the chat model loaded when other modes need GPU memory (ADR 0015).
    pub keep_llm_loaded: bool,
    /// Share the local API in the local network for paired phones (PRD §15
    /// item 12). Off by default; the gateway then does not listen at all.
    pub lan_enabled: bool,
    /// TCP port of the LAN gateway (TLS).
    pub lan_port: u16,
    /// Local OpenAI-compatible server for agents (PRD §15 point 9, ADR 0025).
    pub server_enabled: bool,
    /// Its TCP port (plain HTTP).
    pub server_port: u16,
    /// Listen on every network interface (0.0.0.0) instead of 127.0.0.1 only.
    pub server_lan: bool,
    /// Context window of the server's model, like `context_size` (ADR 0042);
    /// `None` = the largest that fits, up to 64k.
    pub server_context_size: Option<u32>,
    /// Check GitHub for a new version at startup (PRD §10: switchable; ADR 0029).
    pub update_check: bool,
    /// Which key sends a chat prompt (ADR 0036).
    pub send_key: SendKey,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            language: Language::default(),
            theme: Theme::default(),
            log_mode: LogMode::default(),
            log_prompts: false,
            data_dir: None,
            onboarded: false,
            bandwidth_limit_kbps: None,
            unified_memory_percent: 70,
            expert: false,
            context_size: None,
            keep_llm_loaded: false,
            lan_enabled: false,
            lan_port: DEFAULT_LAN_PORT,
            server_enabled: false,
            server_port: DEFAULT_SERVER_PORT,
            server_lan: false,
            server_context_size: None,
            update_check: true,
            send_key: SendKey::default(),
        }
    }
}

#[derive(Debug)]
pub struct Loaded {
    pub settings: AppSettings,
    /// Set when the file existed but could not be read or parsed.
    pub warning: Option<String>,
}

/// Loads settings; a missing or corrupt file falls back to defaults.
pub fn load(path: &Path) -> Loaded {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(settings) => Loaded {
                settings,
                warning: None,
            },
            Err(e) => Loaded {
                settings: AppSettings::default(),
                warning: Some(format!("settings file is corrupt, using defaults: {e}")),
            },
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Loaded {
            settings: AppSettings::default(),
            warning: None,
        },
        Err(e) => Loaded {
            settings: AppSettings::default(),
            warning: Some(format!("cannot read settings file, using defaults: {e}")),
        },
    }
}

/// Saves settings atomically (write to a temp file, then rename).
pub fn save(path: &Path, settings: &AppSettings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(&dir.path().join("settings.json"));
        assert_eq!(loaded.settings, AppSettings::default());
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.settings.language, Language::Pl);
    }

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.json");
        let s = AppSettings {
            language: Language::En,
            theme: Theme::Dark,
            log_mode: LogMode::Verbose,
            log_prompts: true,
            data_dir: Some(PathBuf::from("D:/Lokalsi")),
            onboarded: true,
            bandwidth_limit_kbps: Some(2048),
            unified_memory_percent: 60,
            expert: true,
            context_size: Some(16384),
            keep_llm_loaded: true,
            lan_enabled: true,
            lan_port: 50_000,
            server_enabled: true,
            server_port: 50_001,
            server_lan: true,
            server_context_size: Some(0),
            update_check: false,
            send_key: SendKey::Enter,
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path).settings, s);
    }

    #[test]
    fn corrupt_file_gives_defaults_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.settings, AppSettings::default());
        assert!(loaded.warning.is_some());
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"theme":"light","future_field":1}"#).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.settings.theme, Theme::Light);
        assert_eq!(loaded.settings.log_mode, LogMode::Normal);
        assert_eq!(
            loaded.settings.unified_memory_percent, 70,
            "M0 files get M1 defaults"
        );
        assert!(!loaded.settings.onboarded);
        assert_eq!(loaded.settings.send_key, SendKey::CtrlEnter);
    }

    #[test]
    fn send_key_uses_snake_case() {
        let json = serde_json::to_value(SendKey::CtrlEnter).unwrap();
        assert_eq!(json, "ctrl_enter");
        let key: SendKey = serde_json::from_str(r#""enter""#).unwrap();
        assert_eq!(key, SendKey::Enter);
    }
}

//! Well-known application locations.
//!
//! Until the user picks a data directory in the first-run wizard (M1),
//! settings and logs live in the per-user local data directory.

use std::path::{Path, PathBuf};

/// Overrides the application home directory (useful for tests and portable setups).
pub const ENV_HOME: &str = "STUDIO_LOKALSI_HOME";

const APP_DIR_NAME: &str = "Studio Lokalsi";

/// Per-user application home, e.g. `%LOCALAPPDATA%\Studio Lokalsi` on Windows
/// or `~/Library/Application Support/Studio Lokalsi` on macOS.
pub fn app_home() -> PathBuf {
    if let Some(dir) = std::env::var_os(ENV_HOME).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

pub fn logs_dir(home: &Path) -> PathBuf {
    home.join("logs")
}

pub fn settings_path(home: &Path) -> PathBuf {
    home.join("settings.json")
}

/// SQLite database. Kept in the app home (not the data dir) so it is
/// available before the user picks a data folder (docs/adr/0007).
pub fn db_path(home: &Path) -> PathBuf {
    home.join("app.db")
}

/// Proposed data folder: `<home>/data` on the system disk, or
/// `<mount>/Studio Lokalsi` on any other disk.
pub fn suggested_data_dir(home: &Path, mount: Option<&Path>) -> PathBuf {
    match mount {
        Some(m) if !home.starts_with(m) => m.join(APP_DIR_NAME),
        _ => home.join("data"),
    }
}

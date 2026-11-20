//! Diagnostic logging with two user-facing modes:
//!
//! * **Normal** – INFO and above: start/stop, installs, downloads, jobs, errors.
//! * **Verbose** – DEBUG/TRACE for our crates: IPC commands, engine launch
//!   arguments, HTTP calls to engines, scheduler decisions, timings.
//!
//! The mode can be switched at runtime without a restart, and forced at
//! startup with `--verbose` or `STUDIO_LOKALSI_LOG=verbose`.
//! Prompt/result contents are never logged unless the user enables it
//! explicitly *and* verbose mode is on (see [`loggable_content`]).

use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder as RollingBuilder, InitError, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry, fmt, reload};

use crate::redact::redact;

pub const ENV_LOG_MODE: &str = "STUDIO_LOKALSI_LOG";
pub const VERBOSE_FLAG: &str = "--verbose";
pub const LOG_FILE_PREFIX: &str = "app";
pub const LOG_FILE_SUFFIX: &str = "log";
/// Daily files kept on disk (older ones are deleted on rotation).
pub const MAX_LOG_FILES: usize = 14;

const TAIL_READ_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogMode {
    #[default]
    Normal,
    Verbose,
}

impl LogMode {
    /// `EnvFilter` directive for the mode. Verbose raises our own crates to
    /// TRACE while keeping chatty third-party crates at INFO.
    pub fn directive(self) -> &'static str {
        match self {
            LogMode::Normal => "info",
            LogMode::Verbose => concat!(
                "debug,",
                "lokalsi_core=trace,lokalsi_engines=trace,lokalsi_hwprobe=trace,",
                "studio_lokalsi_lib=trace,ui=trace,",
                "tao=info,wry=info,hyper=info,hyper_util=info,reqwest=info,h2=info,rustls=info"
            ),
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "normal" | "normalny" | "info" => Some(LogMode::Normal),
            "verbose" | "szczegolowy" | "szczegółowy" | "debug" | "trace" => {
                Some(LogMode::Verbose)
            }
            _ => None,
        }
    }
}

/// Decides the startup log mode. Returns `(mode, overridden)`; when
/// `overridden` is true the mode came from the CLI flag or env variable and
/// the value from settings is ignored for this run.
pub fn effective_mode<I, S>(
    configured: LogMode,
    args: I,
    env_value: Option<&str>,
) -> (LogMode, bool)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if args.into_iter().any(|a| a.as_ref() == VERBOSE_FLAG) {
        return (LogMode::Verbose, true);
    }
    if let Some(mode) = env_value.and_then(LogMode::parse) {
        return (mode, true);
    }
    (configured, false)
}

#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("cannot create log directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot create log file: {0}")]
    Appender(#[from] InitError),
    #[error("a global logger is already initialised")]
    AlreadyInitialised,
    #[error("cannot change log mode: {0}")]
    Reload(#[from] reload::Error),
}

static VERBOSE: AtomicBool = AtomicBool::new(false);
static LOG_PROMPTS: AtomicBool = AtomicBool::new(false);

/// Owns the logging pipeline. Dropping it flushes pending log lines.
pub struct LogHandle {
    dir: PathBuf,
    filter: reload::Handle<EnvFilter, Registry>,
    mode: Mutex<LogMode>,
    _guard: WorkerGuard,
}

impl LogHandle {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn mode(&self) -> LogMode {
        *self.mode.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Switches the mode at runtime, without restarting the application.
    pub fn set_mode(&self, mode: LogMode) -> Result<(), LoggingError> {
        let mut current = self.mode.lock().unwrap_or_else(|p| p.into_inner());
        if *current == mode {
            return Ok(());
        }
        self.filter.reload(EnvFilter::new(mode.directive()))?;
        let previous = std::mem::replace(&mut *current, mode);
        drop(current);
        VERBOSE.store(mode == LogMode::Verbose, Ordering::Relaxed);
        tracing::info!(from = ?previous, to = ?mode, "log mode changed");
        Ok(())
    }
}

/// Initialises the global logger: daily-rotated `app.YYYY-MM-DD.log` files in
/// `dir` plus stderr. Can be called once per process.
pub fn init(dir: &Path, mode: LogMode) -> Result<LogHandle, LoggingError> {
    fs::create_dir_all(dir).map_err(|source| LoggingError::CreateDir {
        path: dir.to_path_buf(),
        source,
    })?;
    let appender = RollingBuilder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(dir)?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let (filter, handle) = reload::Layer::new(EnvFilter::new(mode.directive()));

    let file_layer = fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(true);
    let stderr_layer = fmt::layer().with_writer(io::stderr).with_target(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(|_| LoggingError::AlreadyInitialised)?;

    VERBOSE.store(mode == LogMode::Verbose, Ordering::Relaxed);
    Ok(LogHandle {
        dir: dir.to_path_buf(),
        filter: handle,
        mode: Mutex::new(mode),
        _guard: guard,
    })
}

/// Logs panics (with a backtrace in verbose mode) before the default hook runs.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if VERBOSE.load(Ordering::Relaxed) {
            let backtrace = std::backtrace::Backtrace::force_capture();
            tracing::error!(target: "panic", "{info}\n{backtrace}");
        } else {
            tracing::error!(target: "panic", "{info}");
        }
        previous(info);
    }));
}

/// User preference: allow prompt/result contents in logs.
pub fn set_log_prompts(enabled: bool) {
    LOG_PROMPTS.store(enabled, Ordering::Relaxed);
}

/// True only when the user opted in *and* verbose mode is active.
pub fn prompt_logging_enabled() -> bool {
    LOG_PROMPTS.load(Ordering::Relaxed) && VERBOSE.load(Ordering::Relaxed)
}

/// Use for any user content (prompts, transcripts, model output) that goes
/// into a log line. By default only its length is logged.
pub fn loggable_content(text: &str) -> Cow<'static, str> {
    if prompt_logging_enabled() {
        Cow::Owned(redact(text))
    } else {
        Cow::Owned(format!("<hidden: {} chars>", text.chars().count()))
    }
}

/// All log files in `dir`, newest first.
pub fn log_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let is_log = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(&format!(".{LOG_FILE_SUFFIX}")));
        if is_log && entry.file_type()?.is_file() {
            let modified = entry.metadata()?.modified()?;
            files.push((modified, path));
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Ok(files.into_iter().map(|(_, p)| p).collect())
}

/// Last `max_lines` lines of the newest application log.
pub fn tail(dir: &Path, max_lines: usize) -> io::Result<String> {
    let newest = log_files(dir)?.into_iter().find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(&format!("{LOG_FILE_PREFIX}.")))
    });
    let Some(path) = newest else {
        return Ok(String::new());
    };
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(TAIL_READ_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // first line is probably cut in half
    }
    let skip = lines.len().saturating_sub(max_lines);
    Ok(lines[skip..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modes() {
        assert_eq!(LogMode::parse("VERBOSE"), Some(LogMode::Verbose));
        assert_eq!(LogMode::parse(" szczegółowy "), Some(LogMode::Verbose));
        assert_eq!(LogMode::parse("normal"), Some(LogMode::Normal));
        assert_eq!(LogMode::parse("loud"), None);
    }

    #[test]
    fn cli_flag_wins_over_env_and_settings() {
        let args = ["studio-lokalsi.exe", "--verbose"];
        assert_eq!(
            effective_mode(LogMode::Normal, args, Some("normal")),
            (LogMode::Verbose, true)
        );
    }

    #[test]
    fn env_overrides_settings() {
        let args = ["studio-lokalsi.exe"];
        assert_eq!(
            effective_mode(LogMode::Normal, args, Some("verbose")),
            (LogMode::Verbose, true)
        );
        assert_eq!(
            effective_mode(LogMode::Verbose, args, Some("")),
            (LogMode::Verbose, false)
        );
    }

    #[test]
    fn settings_used_without_overrides() {
        let args: [&str; 0] = [];
        assert_eq!(
            effective_mode(LogMode::Verbose, args, None),
            (LogMode::Verbose, false)
        );
    }

    #[test]
    fn tail_returns_last_lines_of_newest_app_log() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("engine-llama.log"), "engine\n").unwrap();
        let content: String = (1..=50).map(|i| format!("line {i}\n")).collect();
        fs::write(dir.path().join("app.2026-11-11.log"), content).unwrap();
        let out = tail(dir.path(), 3).unwrap();
        assert_eq!(out, "line 48\nline 49\nline 50");
    }

    #[test]
    fn tail_of_empty_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(tail(dir.path(), 10).unwrap(), "");
    }

    #[test]
    fn content_hidden_by_default() {
        assert_eq!(loggable_content("tajny prompt"), "<hidden: 12 chars>");
    }
}

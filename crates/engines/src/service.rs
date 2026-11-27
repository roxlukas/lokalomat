//! Engine manager + chat jobs for llama.cpp (V5 and the Chat mode of V2).
//!
//! * install / uninstall builds from the manifest,
//! * keep at most one model loaded in `llama-server`, (re)starting it on demand,
//! * run chat jobs one at a time; cancelling drops the stream (PRD §8.1),
//! * a crashed engine never takes the app down: the job fails with a message,
//!   the state becomes `crashed`, and the next request starts it again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use lokalsi_core::chats::{Chats, ContextUsage, Message, MessageStatus, Role};
use lokalsi_core::db::{Db, DbError, now_secs};
use lokalsi_core::model_params::ModelSettings;
use lokalsi_hwprobe::{ESTIMATE_CTX_TOKENS, kv_per_1k_mb};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::install::{self, InstallError};
use crate::llama::{
    self, ChatError, ChatMessage, ChatParams, RunningServer, ServerConfig, StartError,
};
use crate::manifest::{Build, EngineManifest};
use crate::scheduler::Scheduler;
use crate::tools::{
    Decision, MAX_TOOL_ROUNDS, ToolCallRecord, ToolCallStatus, ToolDef, ToolHost, denied_result,
    preview,
};

/// Context window used when starting `llama-server`.
pub const DEFAULT_CTX: u32 = 8192;
/// Error codes stored on failed messages, translated in the UI (`chat.errors.*`).
pub mod error_codes {
    pub const ENGINE_MISSING: &str = "engine_missing";
    pub const ENGINE_INSTALL: &str = "engine_install_failed";
    pub const ENGINE_START: &str = "engine_start_failed";
    pub const ENGINE_CRASHED: &str = "engine_crashed";
    pub const MODEL_FILES: &str = "model_files_missing";
    pub const ENGINE_ERROR: &str = "engine_error";
    pub const INTERRUPTED: &str = "interrupted";
    /// Prompt (history + tool descriptions) does not fit the context window.
    pub const CONTEXT_FULL: &str = "context_full";
    /// A system library is missing (ADR 0017); the detail is the apt package.
    pub const MISSING_LIBRARY: &str = "engine_missing_library";
    /// The decision request is malformed (ADR 0024).
    pub const DECISION_INVALID: &str = "decision_invalid";
    /// The loaded model cannot answer typed questions (not a decision model).
    pub const NOT_DECISION_MODEL: &str = "not_decision_model";
}

/// Error codes stored on failed jobs (`jobs.errors.*` in the UI).
pub mod job_errors {
    pub use super::error_codes::*;
    /// No build of the engine runs on this computer (ADR 0015).
    pub const ENGINE_UNAVAILABLE: &str = "engine_unavailable";
    pub const AUDIO_UNSUPPORTED: &str = "audio_unsupported";
    pub const OUT_OF_MEMORY: &str = "out_of_memory";
    pub const NO_RESULT: &str = "no_result";
    /// The reference recording of a voice is gone.
    pub const VOICE_MISSING: &str = "voice_missing";
    /// The chosen model cannot speak with a cloned voice.
    pub const VOICE_UNSUPPORTED: &str = "voice_unsupported";
}

/// Largest context window started automatically for tool use.
pub const MAX_TOOLS_CTX: u32 = 32768;
/// Largest window the local server loads for agents (ADR 0028): some agents
/// (Hermes) need 64k for their tool descriptions and memory.
pub const MAX_SERVER_CTX: u32 = 65536;
/// Smallest context window ever started.
pub const MIN_CTX: u32 = 2048;
/// GPU memory kept free next to the model and its context.
const CTX_RESERVE_MB: u64 = 512;

/// Largest automatic context window for a model of `model_mb` in a memory
/// budget of `budget_mb` (GPU, or the unified-memory share). The KV cache is
/// estimated as ~2% of the model size per 1 000 tokens (e.g. 11B Q6_K ≈ 9 GB
/// → ≈ 180 MB per 1k tokens), rounded down to a power of two. Small cards get
/// small windows; large windows need a lot of free VRAM.
pub fn ctx_limit(budget_mb: Option<u64>, model_mb: u64) -> u32 {
    ctx_plan(budget_mb, model_mb).ctx_limit
}

/// Context budget of one load: the largest automatic window and whether the
/// KV cache is stored compactly to get it (ADR 0021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CtxPlan {
    pub ctx_limit: u32,
    /// Start `llama-server` with flash attention and a q8_0 KV cache, which
    /// takes about half the memory of the default f16 cache.
    pub kv_compact: bool,
}

/// Picks the KV cache format: the default f16 cache when memory is plentiful
/// (the largest automatic window fits), otherwise the compact q8_0 cache at
/// half the size per token, which gives tight cards such as 8 GB ones a usable
/// window. The model's estimate already includes an 8k f16 cache
/// ([`ESTIMATE_CTX_TOKENS`]); the compact window is computed without it.
pub fn ctx_plan(budget_mb: Option<u64>, model_mb: u64) -> CtxPlan {
    ctx_plan_up_to(budget_mb, model_mb, MAX_TOOLS_CTX)
}

/// [`ctx_plan`] with a different largest window (the server's
/// [`MAX_SERVER_CTX`]).
pub fn ctx_plan_up_to(budget_mb: Option<u64>, model_mb: u64, max: u32) -> CtxPlan {
    let Some(budget) = budget_mb else {
        return CtxPlan {
            ctx_limit: DEFAULT_CTX,
            kv_compact: false,
        };
    };
    let kv_per_1k = kv_per_1k_mb(model_mb);
    let full = window(budget, model_mb, kv_per_1k, max);
    if full >= max {
        return CtxPlan {
            ctx_limit: full,
            kv_compact: false,
        };
    }
    let weights = model_mb.saturating_sub(kv_per_1k * ESTIMATE_CTX_TOKENS / 1024);
    CtxPlan {
        ctx_limit: window(budget, weights, (kv_per_1k / 2).max(8), max).max(full),
        kv_compact: true,
    }
}

/// Tokens of KV cache at `kv_per_1k` MB per 1 000 tokens that fit next to
/// `base_mb`, rounded down to a power of two within `MIN_CTX..=max`.
fn window(budget: u64, base_mb: u64, kv_per_1k: u64, max: u32) -> u32 {
    let free = budget.saturating_sub(base_mb + CTX_RESERVE_MB);
    let tokens = (free * 1024 / kv_per_1k).min(u64::from(max)) as u32;
    if tokens < MIN_CTX {
        return MIN_CTX;
    }
    // Round down to a power of two.
    1 << (31 - tokens.leading_zeros())
}

/// Room left for the conversation and the answer next to tool descriptions.
const CTX_HEADROOM: u32 = 6144;

/// Room for a one-shot answer when the caller sets no limit: thinking models
/// spend thousands of tokens before they answer.
pub const DEFAULT_ANSWER_TOKENS: u32 = 4096;

/// `max_tokens` of a one-shot completion without its own limit (pipelines,
/// phone): [`DEFAULT_ANSWER_TOKENS`] plus about as much as the prompt (≈3
/// characters per token), so a translation of a long text still fits, but
/// never more than what is left of the context window `ctx`.
pub fn default_answer_tokens(messages: &[llama::ChatMessage], ctx: u32) -> u32 {
    let chars: usize = messages
        .iter()
        .filter_map(|m| m.content.as_deref())
        .map(|c| c.chars().count())
        .sum();
    let prompt = u32::try_from(chars / 3).unwrap_or(u32::MAX);
    let left = ctx.saturating_sub(prompt).max(256);
    DEFAULT_ANSWER_TOKENS.saturating_add(prompt).min(left)
}

/// Context window for a chat offering `tools`: the default without tools,
/// otherwise enough for their descriptions (≈3 characters per token) plus
/// headroom, rounded up to a power of two and capped at [`MAX_TOOLS_CTX`].
pub fn context_for(tools: &[serde_json::Value]) -> u32 {
    if tools.is_empty() {
        return DEFAULT_CTX;
    }
    let chars: usize = tools.iter().map(|t| t.to_string().len()).sum();
    let need = (chars / 3) as u32 + CTX_HEADROOM;
    need.next_power_of_two().clamp(DEFAULT_CTX, MAX_TOOLS_CTX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    NotInstalled,
    Installing,
    Stopped,
    Starting,
    Running,
    Crashed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledBuild {
    pub build_id: String,
    pub version: String,
    pub backend: String,
    pub dir: PathBuf,
    pub installed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildOption {
    pub id: String,
    pub backend: String,
    pub size: u64,
    pub recommended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedModel {
    pub model_id: String,
    pub variant_id: String,
    pub name: String,
    pub memory_estimate_mb: u64,
    pub since: i64,
    /// Context window the server was started with.
    #[serde(default)]
    pub ctx: u32,
    /// Started with the compact (q8_0) KV cache.
    #[serde(default)]
    pub kv_compact: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    pub id: String,
    pub name: String,
    pub version: String,
    pub capabilities: Vec<crate::Capability>,
    pub license: String,
    pub state: EngineState,
    pub installed: Option<InstalledBuild>,
    /// `true` when the installed build is not the manifest's version.
    pub update_available: bool,
    pub builds: Vec<BuildOption>,
    /// `(done, total)` bytes while installing.
    pub install_progress: Option<(u64, u64)>,
    pub loaded: Option<LoadedModel>,
    pub error: Option<String>,
    pub log_path: PathBuf,
    /// `python` for engines that run in an environment created by `uv` (ADR 0018).
    #[serde(default)]
    pub kind: crate::EngineKind,
    /// Python engines while installing: `uv`, `python`, `packages` or `verify`.
    #[serde(default)]
    pub stage: Option<String>,
    /// Python engines: size of the environment on disk.
    #[serde(default)]
    pub disk_size: Option<u64>,
}

/// A loaded `llama-server` reserved for one proxied request (ADR 0025).
pub struct Upstream {
    pub base_url: String,
    pub token: String,
    /// Display name of the loaded model.
    pub model_name: String,
    _turn: tokio::sync::OwnedMutexGuard<()>,
}

/// What to load for a chat: resolved by the caller from the model library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadRequest {
    pub model_id: String,
    pub variant_id: String,
    pub name: String,
    pub path: PathBuf,
    pub memory_estimate_mb: u64,
    /// The model's chat template supports function calling (catalog feature `tools`).
    pub supports_tools: bool,
    /// Context window chosen by the user; `None` = automatic.
    pub ctx_size: Option<u32>,
    /// Largest automatic context window that fits free GPU memory ([`ctx_plan`]).
    pub ctx_limit: u32,
    /// Compact (q8_0) KV cache, see [`CtxPlan::kv_compact`].
    pub kv_compact: bool,
}

impl LoadRequest {
    /// Context window for this load when `tools` are offered: the user's
    /// choice, otherwise what the tools need, capped by free memory.
    pub fn context(&self, tools: &[serde_json::Value]) -> u32 {
        self.ctx_size
            .unwrap_or_else(|| context_for(tools).min(self.ctx_limit))
            .max(MIN_CTX)
    }
}

pub trait EngineEvents: Send + Sync + 'static {
    fn engine_changed(&self, _status: &EngineStatus) {}
    fn chat_delta(&self, _message_id: i64, _content: &str, _reasoning: &str) {}
    fn chat_updated(&self, _message: &Message) {}
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Install(#[from] InstallError),
    #[error("no build of {0} for this computer")]
    NoBuild(String),
    #[error("the engine is not installed")]
    NotInstalled,
    #[error("an installation is already running")]
    Busy,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Setting up a Python environment failed (ADR 0018).
    #[error("{0}")]
    Setup(String),
}

impl From<rusqlite::Error> for EngineError {
    fn from(e: rusqlite::Error) -> Self {
        EngineError::Db(DbError::Sql(e))
    }
}

struct Inner {
    state: EngineState,
    install_progress: Option<(u64, u64)>,
    install_cancel: Option<CancellationToken>,
    server: Option<RunningServer>,
    loaded: Option<LoadedModel>,
    /// `-ngl` the loaded model was started with (not on `LoadedModel`, which
    /// other crates build; ADR 0036).
    loaded_gpu_layers: Option<u32>,
    error: Option<String>,
}

pub struct EngineService {
    manifest: EngineManifest,
    db: Arc<Db>,
    http: reqwest::Client,
    platform: String,
    engines_dir: Mutex<PathBuf>,
    log_path: PathBuf,
    events: Arc<dyn EngineEvents>,
    inner: Mutex<Inner>,
    /// Serialises model loads and generations (`llama-server` runs with one slot).
    /// In an `Arc` so a proxied request can own its turn (ADR 0025).
    generation: Arc<tokio::sync::Mutex<()>>,
    jobs: Mutex<HashMap<i64, CancellationToken>>,
    /// One installation at a time; automatic installs wait for a running one.
    install_lock: tokio::sync::Mutex<()>,
    /// Latest hardware profile, used to pick the build for automatic installs.
    profile: Mutex<Option<lokalsi_hwprobe::HardwareProfile>>,
    /// MCP tools for the chat (set by the app).
    tool_host: Mutex<Option<Arc<dyn ToolHost>>>,
    /// Tool calls waiting for the user's consent.
    consents: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Decision>>>,
    /// VRAM scheduler (ADR 0015); the loaded model is a resident there.
    scheduler: Mutex<Option<Arc<Scheduler>>>,
    /// Name of this server slot: the engine id, or e.g. `llama-cpp-decision`
    /// for the decision models' own `llama-server` (ADR 0024). It keys the
    /// scheduler resident and the log file.
    slot: String,
    /// Set for a second slot: installs and the engine status belong to it.
    primary: Option<Arc<EngineService>>,
    /// Second slots of this engine; stopped before installing or removing it.
    slots: Mutex<Vec<std::sync::Weak<EngineService>>>,
}

impl EngineService {
    pub fn new(
        manifest: EngineManifest,
        db: Arc<Db>,
        http: reqwest::Client,
        platform: String,
        engines_dir: PathBuf,
        logs_dir: &Path,
        events: Arc<dyn EngineEvents>,
    ) -> Arc<Self> {
        let log_path = logs_dir.join(format!("engine-{}.log", manifest.id));
        let slot = manifest.id.clone();
        let service = Arc::new(EngineService {
            manifest,
            db,
            http,
            platform,
            engines_dir: Mutex::new(engines_dir),
            log_path,
            events,
            inner: Mutex::new(Inner {
                state: EngineState::NotInstalled,
                install_progress: None,
                install_cancel: None,
                server: None,
                loaded: None,
                loaded_gpu_layers: None,
                error: None,
            }),
            generation: Arc::new(tokio::sync::Mutex::new(())),
            jobs: Mutex::new(HashMap::new()),
            install_lock: tokio::sync::Mutex::new(()),
            profile: Mutex::new(None),
            tool_host: Mutex::new(None),
            consents: Mutex::new(HashMap::new()),
            scheduler: Mutex::new(None),
            slot,
            primary: None,
            slots: Mutex::new(Vec::new()),
        });
        let installed = service.installed().ok().flatten();
        service.lock().state = if installed.is_some() {
            EngineState::Stopped
        } else {
            EngineState::NotInstalled
        };
        if service
            .manifest
            .capabilities
            .contains(&crate::Capability::Chat)
        {
            match Chats::new(&service.db).fail_unfinished(error_codes::INTERRUPTED) {
                Ok(n) if n > 0 => tracing::info!(
                    count = n,
                    "chat jobs interrupted by the previous run marked as failed"
                ),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "cannot clean up unfinished chat jobs"),
            }
        }
        service
    }

    /// A second `llama-server` of the same engine (ADR 0024: decision models
    /// next to the chat model). It shares the installed build, installs
    /// through `primary` and runs its own process, log and scheduler resident
    /// named `slot`; it does not publish engine status (the primary does).
    pub fn second_slot(primary: &Arc<EngineService>, slot: &str) -> Arc<Self> {
        let log_dir = primary
            .log_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let service = Arc::new(EngineService {
            manifest: primary.manifest.clone(),
            db: Arc::clone(&primary.db),
            http: primary.http.clone(),
            platform: primary.platform.clone(),
            engines_dir: Mutex::new(primary.engines_dir()),
            log_path: log_dir.join(format!("engine-{slot}.log")),
            events: Arc::clone(&primary.events),
            inner: Mutex::new(Inner {
                state: EngineState::Stopped,
                install_progress: None,
                install_cancel: None,
                server: None,
                loaded: None,
                loaded_gpu_layers: None,
                error: None,
            }),
            generation: Arc::new(tokio::sync::Mutex::new(())),
            jobs: Mutex::new(HashMap::new()),
            install_lock: tokio::sync::Mutex::new(()),
            profile: Mutex::new(None),
            tool_host: Mutex::new(None),
            consents: Mutex::new(HashMap::new()),
            scheduler: Mutex::new(None),
            slot: slot.to_owned(),
            primary: Some(Arc::clone(primary)),
            slots: Mutex::new(Vec::new()),
        });
        primary
            .slots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Arc::downgrade(&service));
        if let Some(scheduler) = primary.scheduler() {
            service.set_scheduler(scheduler);
        }
        service
    }

    /// Scheduler resident key and log name of this slot.
    pub fn slot(&self) -> &str {
        &self.slot
    }

    /// The model loaded in this slot, if any.
    pub fn loaded(&self) -> Option<LoadedModel> {
        self.lock().loaded.clone()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn manifest(&self) -> &EngineManifest {
        &self.manifest
    }

    /// Connects the VRAM scheduler: the loaded model becomes a resident that
    /// the scheduler may unload (least recently used first) for other jobs.
    pub fn set_scheduler(self: &Arc<Self>, scheduler: Arc<Scheduler>) {
        let weak = Arc::downgrade(self);
        scheduler.register_evictor(
            &self.slot,
            Arc::new(move || {
                let weak = weak.clone();
                Box::pin(async move {
                    match weak.upgrade() {
                        Some(s) => s.evict().await,
                        None => false,
                    }
                })
            }),
        );
        *self.scheduler.lock().unwrap_or_else(|p| p.into_inner()) = Some(scheduler);
    }

    fn scheduler(&self) -> Option<Arc<Scheduler>> {
        self.scheduler
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Unloads the model unless it is answering right now.
    async fn evict(&self) -> bool {
        let Ok(_guard) = self.generation.try_lock() else {
            tracing::debug!(engine = %self.slot, "model is busy, not unloading");
            return false;
        };
        tracing::info!(engine = %self.slot, "unloading model to free memory");
        self.stop_server().await;
        true
    }

    pub fn set_profile(&self, profile: Option<lokalsi_hwprobe::HardwareProfile>) {
        *self.profile.lock().unwrap_or_else(|p| p.into_inner()) = profile;
    }

    fn profile(&self) -> Option<lokalsi_hwprobe::HardwareProfile> {
        self.profile
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Plug and play (PRD §15): installs the best build for this computer if
    /// the engine is missing; waits for an installation already in progress.
    pub async fn ensure_installed(self: &Arc<Self>) -> Result<(), EngineError> {
        if let Some(primary) = &self.primary {
            return Box::pin(primary.ensure_installed()).await;
        }
        let _lock = self.install_lock.lock().await;
        if self.installed()?.is_some() {
            return Ok(());
        }
        tracing::info!(engine = %self.manifest.id, "engine missing, installing automatically");
        let profile = self.profile();
        self.install_locked(None, profile.as_ref()).await
    }

    pub fn set_engines_dir(&self, dir: PathBuf) {
        *self.engines_dir.lock().unwrap_or_else(|p| p.into_inner()) = dir;
    }

    fn engines_dir(&self) -> PathBuf {
        if let Some(primary) = &self.primary {
            return primary.engines_dir();
        }
        self.engines_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn installed(&self) -> Result<Option<InstalledBuild>, EngineError> {
        Ok(self
            .db
            .conn()
            .query_row(
                "SELECT build_id, version, backend, dir, installed_at FROM installed_engines WHERE engine_id = ?1",
                params![self.manifest.id],
                |r| {
                    Ok(InstalledBuild {
                        build_id: r.get(0)?,
                        version: r.get(1)?,
                        backend: r.get(2)?,
                        dir: PathBuf::from(r.get::<_, String>(3)?),
                        installed_at: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn status(&self, profile: Option<&lokalsi_hwprobe::HardwareProfile>) -> EngineStatus {
        let installed = self.installed().ok().flatten();
        let builds = self.manifest.builds_for(&self.platform, profile);
        let inner = self.lock();
        EngineStatus {
            id: self.manifest.id.clone(),
            name: self.manifest.name.clone(),
            version: self.manifest.version.clone(),
            capabilities: self.manifest.capabilities.clone(),
            license: self.manifest.license.name.clone(),
            state: inner.state,
            update_available: installed
                .as_ref()
                .is_some_and(|i| i.version != self.manifest.version),
            installed,
            builds: builds
                .iter()
                .enumerate()
                .map(|(i, b)| BuildOption {
                    id: b.id.clone(),
                    backend: format!("{:?}", b.backend).to_lowercase(),
                    size: b.total_size(),
                    recommended: i == 0,
                })
                .collect(),
            install_progress: inner.install_progress,
            loaded: inner.loaded.clone(),
            error: inner.error.clone(),
            log_path: self.log_path.clone(),
            kind: self.manifest.kind,
            stage: None,
            disk_size: None,
        }
    }

    fn notify(&self, profile: Option<&lokalsi_hwprobe::HardwareProfile>) {
        if self.primary.is_some() {
            // The engine's status is the primary's; a second slot would
            // overwrite it in the UI.
            return;
        }
        self.events.engine_changed(&self.status(profile));
    }

    fn set_state(&self, state: EngineState, error: Option<String>) {
        {
            let mut inner = self.lock();
            inner.state = state;
            inner.error = error;
        }
        self.notify(None);
    }

    // --- install ------------------------------------------------------------------

    /// Installs `build_id` (or the best build for this hardware).
    pub async fn install(
        self: &Arc<Self>,
        build_id: Option<&str>,
        profile: Option<&lokalsi_hwprobe::HardwareProfile>,
    ) -> Result<(), EngineError> {
        let _lock = self.install_lock.lock().await;
        self.install_locked(build_id, profile).await
    }

    async fn install_locked(
        self: &Arc<Self>,
        build_id: Option<&str>,
        profile: Option<&lokalsi_hwprobe::HardwareProfile>,
    ) -> Result<(), EngineError> {
        let build: Build = match build_id {
            Some(id) => self.manifest.build(id).cloned(),
            None => self
                .manifest
                .builds_for(&self.platform, profile)
                .first()
                .map(|b| (*b).clone()),
        }
        .ok_or_else(|| EngineError::NoBuild(self.manifest.name.clone()))?;

        let cancel = CancellationToken::new();
        {
            let mut inner = self.lock();
            if inner.state == EngineState::Installing {
                return Err(EngineError::Busy);
            }
            inner.state = EngineState::Installing;
            inner.install_progress = Some((0, build.total_size()));
            inner.install_cancel = Some(cancel.clone());
            inner.error = None;
        }
        self.notify(profile);
        self.stop_all_slots().await;

        let this = Arc::clone(self);
        let last = Mutex::new(Instant::now() - Duration::from_secs(1));
        let progress = move |done: u64, total: u64| {
            let mut last = last.lock().unwrap_or_else(|p| p.into_inner());
            if last.elapsed() >= Duration::from_millis(250) || done >= total {
                *last = Instant::now();
                this.lock().install_progress = Some((done, total));
                this.notify(None);
            }
        };
        let dir = self.engines_dir();
        let result =
            install::install(&self.http, &self.manifest, &build, &dir, &cancel, &progress).await;

        let previous = self.installed().ok().flatten();
        match result {
            Ok(binary) => {
                let target = install::build_dir(&dir, &self.manifest, &build);
                self.db.conn().execute(
                    "INSERT INTO installed_engines (engine_id, build_id, version, backend, dir, installed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(engine_id) DO UPDATE SET build_id = excluded.build_id, version = excluded.version,
                        backend = excluded.backend, dir = excluded.dir, installed_at = excluded.installed_at",
                    params![
                        self.manifest.id,
                        build.id,
                        self.manifest.version,
                        format!("{:?}", build.backend).to_lowercase(),
                        target.to_string_lossy(),
                        now_secs()
                    ],
                )?;
                if let Some(old) = previous.filter(|p| p.dir != target) {
                    let _ = std::fs::remove_dir_all(&old.dir);
                }
                {
                    let mut inner = self.lock();
                    inner.install_progress = None;
                    inner.install_cancel = None;
                }
                // Linux: tell about a missing system library right away (ADR 0017).
                let missing = crate::syslibs::probe(&binary).await;
                self.set_state(EngineState::Stopped, missing.map(|m| m.to_string()));
                Ok(())
            }
            Err(e) => {
                tracing::error!(engine = %self.manifest.id, build = %build.id, error = %e, "engine installation failed");
                {
                    let mut inner = self.lock();
                    inner.install_progress = None;
                    inner.install_cancel = None;
                }
                let state = if previous.is_some() {
                    EngineState::Stopped
                } else {
                    EngineState::NotInstalled
                };
                let msg = (!matches!(e, InstallError::Cancelled)).then(|| e.to_string());
                self.set_state(state, msg);
                Err(e.into())
            }
        }
    }

    pub fn cancel_install(&self) {
        if let Some(c) = self.lock().install_cancel.as_ref() {
            c.cancel();
        }
    }

    pub async fn uninstall(&self) -> Result<(), EngineError> {
        self.stop_all_slots().await;
        if let Some(installed) = self.installed()? {
            match std::fs::remove_dir_all(&installed.dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            self.db.conn().execute(
                "DELETE FROM installed_engines WHERE engine_id = ?1",
                params![self.manifest.id],
            )?;
            tracing::info!(engine = %self.manifest.id, "engine uninstalled");
        }
        self.set_state(EngineState::NotInstalled, None);
        Ok(())
    }

    // --- server lifecycle ---------------------------------------------------------------

    /// Stops this server and every second slot of the engine (they run from
    /// the same files, which an install or uninstall replaces).
    async fn stop_all_slots(&self) {
        let slots: Vec<Arc<EngineService>> = self
            .slots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .collect();
        for slot in slots {
            slot.stop_server().await;
        }
        self.stop_server().await;
    }

    /// Unloads the model and stops the process ("Zwolnij" in the memory meter).
    pub async fn stop_server(&self) {
        let (server, was_loaded) = {
            let mut inner = self.lock();
            let was_loaded = inner.loaded.take().is_some();
            (inner.server.take(), was_loaded)
        };
        if was_loaded && let Some(s) = self.scheduler() {
            s.resident_unloaded(&self.slot);
        }
        if let Some(server) = server {
            server.stop().await;
        }
        let installed = self.installed().ok().flatten().is_some();
        let state = self.lock().state;
        if matches!(
            state,
            EngineState::Running | EngineState::Starting | EngineState::Crashed
        ) {
            self.set_state(
                if installed {
                    EngineState::Stopped
                } else {
                    EngineState::NotInstalled
                },
                None,
            );
        }
    }

    /// Restarts with the model that was loaded (or just clears a crash).
    /// Errors are `(code, detail)` like chat jobs (`error_codes`).
    pub async fn restart(self: &Arc<Self>) -> Result<(), (&'static str, String)> {
        let loaded = self.lock().loaded.clone();
        let path = self.lock().server.as_ref().map(|s| s.model_path.clone());
        self.stop_server().await;
        if let (Some(loaded), Some(path)) = (loaded, path) {
            let req = LoadRequest {
                model_id: loaded.model_id,
                variant_id: loaded.variant_id,
                name: loaded.name,
                path,
                memory_estimate_mb: loaded.memory_estimate_mb,
                supports_tools: false,
                ctx_size: Some(loaded.ctx.max(MIN_CTX)),
                ctx_limit: loaded.ctx,
                kv_compact: loaded.kv_compact,
            };
            let ctx = req.context(&[]);
            let _guard = self.generation.lock().await;
            self.ensure_loaded(&req, ctx, &CancellationToken::new())
                .await?;
        }
        Ok(())
    }

    /// Makes sure `llama-server` runs with `req` loaded; restarts it for a
    /// different model, a too small context window (`ctx`) or after a crash.
    /// Caller holds `generation`.
    async fn ensure_loaded(
        self: &Arc<Self>,
        req: &LoadRequest,
        ctx: u32,
        cancel: &CancellationToken,
    ) -> Result<(), (&'static str, String)> {
        if !self.needs_load(req, ctx) {
            return Ok(());
        }
        self.stop_server().await;
        let installed = self.installed().ok().flatten().ok_or((
            error_codes::ENGINE_MISSING,
            "engine not installed".to_owned(),
        ))?;
        let binary = install::find_binary(&installed.dir, &self.manifest.launch.cmd).ok_or((
            error_codes::ENGINE_MISSING,
            "engine files missing".to_owned(),
        ))?;
        if !req.path.is_file() {
            return Err((error_codes::MODEL_FILES, req.path.display().to_string()));
        }
        self.set_state(EngineState::Starting, None);
        let gpu_layers = self.gpu_layers(req);
        let config = ServerConfig {
            binary,
            model_path: req.path.clone(),
            ctx,
            kv_compact: req.kv_compact,
            gpu_layers,
            log_file: self.log_path.clone(),
            launch: self.manifest.launch.clone(),
        };
        match llama::start(&config, &self.http, cancel).await {
            Ok(server) => {
                let exited = server.exited();
                {
                    let mut inner = self.lock();
                    inner.server = Some(server);
                    inner.loaded = Some(LoadedModel {
                        model_id: req.model_id.clone(),
                        variant_id: req.variant_id.clone(),
                        name: req.name.clone(),
                        memory_estimate_mb: req.memory_estimate_mb,
                        since: now_secs(),
                        ctx,
                        kv_compact: req.kv_compact,
                    });
                    inner.loaded_gpu_layers = gpu_layers;
                }
                self.set_state(EngineState::Running, None);
                if let Some(s) = self.scheduler() {
                    s.resident_loaded(&self.slot, &req.name, self.gpu_memory_mb(req, gpu_layers));
                }
                self.watch_crash(exited);
                Ok(())
            }
            Err(StartError::Cancelled) => {
                self.set_state(EngineState::Stopped, None);
                Err((error_codes::ENGINE_START, "cancelled".into()))
            }
            Err(StartError::MissingLibrary(m)) => {
                tracing::error!(lib = %m.lib, package = %m.package, "llama-server cannot start: missing system library");
                self.set_state(EngineState::Crashed, Some(m.to_string()));
                Err((error_codes::MISSING_LIBRARY, m.package))
            }
            Err(e) => {
                self.set_state(EngineState::Crashed, Some(e.to_string()));
                Err((error_codes::ENGINE_START, e.to_string()))
            }
        }
    }

    /// True when `req` with context `ctx` needs a (re)start of the server.
    fn needs_load(&self, req: &LoadRequest, ctx: u32) -> bool {
        let gpu_layers = self.gpu_layers(req);
        let inner = self.lock();
        !matches!(
            (&inner.server, &inner.loaded),
            (Some(server), Some(loaded))
                if server.exit_info().is_none()
                    && loaded.model_id == req.model_id
                    && loaded.variant_id == req.variant_id
                    && loaded.ctx >= ctx
                    && loaded.kv_compact == req.kv_compact
                    && inner.loaded_gpu_layers == gpu_layers
        )
    }

    /// Marks the engine crashed when the process dies on its own.
    fn watch_crash(
        self: &Arc<Self>,
        mut exited: tokio::sync::watch::Receiver<Option<llama::ExitInfo>>,
    ) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            if exited.wait_for(|e| e.is_some()).await.is_err() {
                return;
            }
            let info = exited.borrow().clone();
            if let Some(info) = info.filter(|i| !i.requested) {
                let tail = llama::log_tail(&this.log_path, 6);
                tracing::error!(code = ?info.code, log = %tail, "engine crashed");
                {
                    let mut inner = this.lock();
                    inner.server = None;
                    inner.loaded = None;
                }
                if let Some(s) = this.scheduler() {
                    s.resident_unloaded(&this.slot);
                }
                this.set_state(
                    EngineState::Crashed,
                    Some(format!("exit code {:?}", info.code)),
                );
            }
        });
    }

    // --- chat jobs ---------------------------------------------------------------------------

    pub fn cancel_job(&self, message_id: i64) -> bool {
        match self
            .jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&message_id)
        {
            Some(token) => {
                tracing::info!(message_id, "chat job cancelled by the user");
                token.cancel();
                true
            }
            None => false,
        }
    }

    fn update_message(&self, id: i64, status: MessageStatus, error: Option<&str>) {
        let chats = Chats::new(&self.db);
        if let Err(e) = chats.set_status(id, status, error) {
            tracing::warn!(message_id = id, error = %e, "cannot save chat job status");
        }
        if let Ok(Some(m)) = chats.message(id) {
            self.events.chat_updated(&m);
        }
    }

    /// Runs one chat job for the assistant message `message_id`, using the
    /// session history up to (and excluding) that message.
    pub async fn run_chat(
        self: Arc<Self>,
        message_id: i64,
        model: LoadRequest,
        params: ChatParams,
    ) {
        let cancel = CancellationToken::new();
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(message_id, cancel.clone());
        let started = Instant::now();
        let params = self.with_model_defaults(&model, params);
        self.run_chat_inner(message_id, &model, &params, &cancel)
            .await;
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&message_id);
        tracing::debug!(
            message_id,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "chat job finished"
        );
    }

    /// Layers on the GPU set in the Inspector for `req` (ADR 0036); `None` = Auto.
    fn gpu_layers(&self, req: &LoadRequest) -> Option<u32> {
        match ModelSettings::new(&self.db).get(&req.model_id, &req.variant_id) {
            Ok(p) => p.gpu_layers,
            Err(e) => {
                tracing::warn!(error = %e, model = %req.model_id, "cannot read model parameters, GPU layers on Auto");
                None
            }
        }
    }

    /// GPU memory `req` needs with `gpu_layers` on the card: the estimate
    /// scaled by the share of layers (the rest stays in RAM).
    fn gpu_memory_mb(&self, req: &LoadRequest, gpu_layers: Option<u32>) -> u64 {
        let Some(n) = gpu_layers else {
            return req.memory_estimate_mb;
        };
        let total = lokalsi_core::gguf::block_count(&req.path)
            .inspect_err(|e| tracing::debug!(error = %e, path = %req.path.display(), "cannot read layer count"))
            .ok()
            .flatten();
        gpu_share_mb(req.memory_estimate_mb, n, total)
    }

    /// `params` with the model's Inspector defaults filled in (ADR 0036).
    fn with_model_defaults(&self, model: &LoadRequest, params: ChatParams) -> ChatParams {
        let params = match ModelSettings::new(&self.db).get(&model.model_id, &model.variant_id) {
            Ok(defaults) => params.with_model_defaults(&defaults),
            Err(e) => {
                tracing::warn!(error = %e, model = %model.model_id, "cannot read model parameters, using Auto");
                params
            }
        };
        tracing::debug!(
            model = %model.model_id,
            temperature = ?params.temperature,
            top_p = ?params.top_p,
            top_k = ?params.top_k,
            "sampling parameters"
        );
        params
    }

    async fn run_chat_inner(
        self: &Arc<Self>,
        message_id: i64,
        model: &LoadRequest,
        params: &ChatParams,
        cancel: &CancellationToken,
    ) {
        let chats = Chats::new(&self.db);
        let Ok(Some(target)) = chats.message(message_id) else {
            return;
        };
        let history: Vec<ChatMessage> = match chats.messages(target.session_id) {
            Ok(all) => all
                .into_iter()
                // Earlier turns only; failed/cancelled answers are not fed back to the model.
                .filter(|m| m.id < message_id && !m.content.is_empty())
                .filter(|m| m.role != Role::Assistant || m.status == MessageStatus::Done)
                .map(|m| {
                    let role = match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                    };
                    ChatMessage::text(role, m.content)
                })
                .collect(),
            Err(e) => {
                self.update_message(
                    message_id,
                    MessageStatus::Failed,
                    Some(&format!("{}: {e}", error_codes::ENGINE_ERROR)),
                );
                return;
            }
        };

        // Wait for any running generation (one slot).
        self.update_message(message_id, MessageStatus::Queued, None);
        let _guard = tokio::select! {
            g = self.generation.lock() => g,
            _ = cancel.cancelled() => {
                self.update_message(message_id, MessageStatus::Cancelled, None);
                return;
            }
        };

        // Plug and play: install the engine on first use, the user just waits.
        if self.installed().ok().flatten().is_none() {
            self.update_message(message_id, MessageStatus::Preparing, None);
            let installed = tokio::select! {
                r = self.ensure_installed() => r,
                _ = cancel.cancelled() => {
                    self.update_message(message_id, MessageStatus::Cancelled, None);
                    return;
                }
            };
            if let Err(e) = installed {
                self.update_message(
                    message_id,
                    MessageStatus::Failed,
                    Some(&format!("{}: {e}", error_codes::ENGINE_INSTALL)),
                );
                return;
            }
        }

        // Tools from enabled MCP servers, only for models that can call tools.
        // Known before loading: their descriptions decide the context window.
        let host = self.tool_host();
        let tools: Vec<ToolDef> = match (&host, model.supports_tools) {
            (Some(h), true) => h.tools().await,
            _ => Vec::new(),
        };
        let openai_tools: Vec<serde_json::Value> = tools.iter().map(ToolDef::to_openai).collect();
        let ctx = model.context(&openai_tools);
        if !tools.is_empty() {
            tracing::debug!(tools = tools.len(), ctx, "tools offered to the model");
        }

        if self.needs_load(model, ctx) {
            self.update_message(message_id, MessageStatus::LoadingModel, None);
            // Make room in GPU memory: other models may be unloaded (LRU).
            // Chat never waits for running jobs; llama.cpp offloads if needed.
            if let Some(s) = self.scheduler() {
                let room = s
                    .make_room(
                        self.gpu_memory_mb(model, self.gpu_layers(model)),
                        &self.slot,
                        false,
                        cancel,
                    )
                    .await;
                if room.is_none() {
                    self.update_message(message_id, MessageStatus::Cancelled, None);
                    return;
                }
            }
        }
        if let Err((code, detail)) = self.ensure_loaded(model, ctx, cancel).await {
            if cancel.is_cancelled() {
                self.update_message(message_id, MessageStatus::Cancelled, None);
            } else {
                self.update_message(
                    message_id,
                    MessageStatus::Failed,
                    Some(&format!("{code}: {detail}")),
                );
            }
            return;
        }

        self.update_message(message_id, MessageStatus::Running, None);
        if let Some(s) = self.scheduler() {
            s.touch(&self.slot);
        }
        tracing::info!(message_id, model = %model.model_id, history = history.len(), "chat generation started");
        tracing::debug!(
            prompt = %lokalsi_core::logging::loggable_content(history.last().and_then(|m| m.content.as_deref()).unwrap_or("")),
            "chat prompt"
        );

        let mut convo = history;
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut records: Vec<ToolCallRecord> = Vec::new();
        let mut last: Option<llama::ChatOutcome> = None;
        // Characters of (system, messages, tools) sent in the latest round.
        let mut shape = (0, 0, 0);

        for round in 0..=MAX_TOOL_ROUNDS {
            let endpoint = self
                .lock()
                .server
                .as_ref()
                .map(|s| (s.base_url(), s.token().to_owned()));
            let Some(endpoint) = endpoint else {
                self.update_message(
                    message_id,
                    MessageStatus::Failed,
                    Some(error_codes::ENGINE_CRASHED),
                );
                return;
            };
            if round > 0 && !content.is_empty() {
                content.push_str("\n\n");
                self.events.chat_delta(message_id, "\n\n", "");
            }
            let events = Arc::clone(&self.events);
            let mut on_delta = move |c: &str, r: &str| events.chat_delta(message_id, c, r);
            // After the last allowed round the model must answer without tools.
            let offered: &[serde_json::Value] = if round < MAX_TOOL_ROUNDS {
                &openai_tools
            } else {
                &[]
            };
            shape = prompt_shape(&convo, offered);
            let result = stream_with(
                &self.http,
                &endpoint,
                &convo,
                offered,
                params,
                cancel,
                &mut on_delta,
            )
            .await;

            let out = match result {
                Ok(out) => out,
                Err(failed) => {
                    let (partial, err) = *failed;
                    content.push_str(&partial.content);
                    reasoning.push_str(&partial.reasoning);
                    let _ = chats.save_output(message_id, &content, &reasoning, None, None);
                    // Give the crash watcher a moment to notice a dead process.
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    let crashed = self
                        .lock()
                        .server
                        .as_ref()
                        .is_none_or(|s| s.exit_info().is_some());
                    let code = if crashed {
                        error_codes::ENGINE_CRASHED
                    } else if err.is_context_full() {
                        error_codes::CONTEXT_FULL
                    } else {
                        error_codes::ENGINE_ERROR
                    };
                    tracing::error!(message_id, error = %err, crashed, "chat generation failed");
                    self.update_message(
                        message_id,
                        MessageStatus::Failed,
                        Some(&format!("{code}: {err}")),
                    );
                    return;
                }
            };
            content.push_str(&out.content);
            reasoning.push_str(&out.reasoning);

            if out.cancelled || out.tool_calls.is_empty() || host.is_none() {
                last = Some(out);
                break;
            }

            // The model asked for tools: run them (with consent) and continue.
            convo.push(ChatMessage::assistant_tool_calls(
                &out.content,
                &out.tool_calls,
            ));
            for call in &out.tool_calls {
                let call_id = if call.id.is_empty() {
                    format!("call_{message_id}_{}", records.len())
                } else {
                    call.id.clone()
                };
                let def = tools.iter().find(|t| t.function_name == call.name);
                let mut record = ToolCallRecord {
                    id: call_id.clone(),
                    function_name: call.name.clone(),
                    server_name: def.map(|d| d.server_name.clone()).unwrap_or_default(),
                    tool: def
                        .map(|d| d.tool.clone())
                        .unwrap_or_else(|| call.name.clone()),
                    remote: def.is_some_and(|d| d.remote),
                    arguments: call.arguments.clone(),
                    status: ToolCallStatus::Pending,
                    result: None,
                    duration_ms: None,
                };
                let model_text = match (def, &host) {
                    (Some(def), Some(host)) => {
                        let allowed = if def.ask {
                            records.push(record.clone());
                            self.publish_tools(message_id, &records);
                            let decision = self.wait_for_decision(&call_id, cancel).await;
                            records.pop();
                            match decision {
                                Some(d) => {
                                    if d.allow && d.remember {
                                        host.remember_allow(def);
                                    }
                                    d.allow
                                }
                                None => {
                                    // Cancelled while waiting for consent.
                                    record.status = ToolCallStatus::Denied;
                                    records.push(record);
                                    self.publish_tools(message_id, &records);
                                    let _ = chats
                                        .save_output(message_id, &content, &reasoning, None, None);
                                    self.update_message(message_id, MessageStatus::Cancelled, None);
                                    return;
                                }
                            }
                        } else {
                            true
                        };
                        if allowed {
                            record.status = ToolCallStatus::Running;
                            records.push(record.clone());
                            self.publish_tools(message_id, &records);
                            records.pop();
                            let args = serde_json::from_str(&call.arguments)
                                .unwrap_or(serde_json::Value::Null);
                            let started = Instant::now();
                            let result = tokio::select! {
                                r = host.call(def, args) => Some(r),
                                _ = cancel.cancelled() => None,
                            };
                            record.duration_ms = Some(started.elapsed().as_millis() as u64);
                            match result {
                                Some(Ok((text, is_error))) => {
                                    record.status = if is_error {
                                        ToolCallStatus::Error
                                    } else {
                                        ToolCallStatus::Done
                                    };
                                    record.result = Some(preview(&text));
                                    text
                                }
                                Some(Err(e)) => {
                                    record.status = ToolCallStatus::Error;
                                    record.result = Some(e.clone());
                                    format!("Tool error: {e}")
                                }
                                None => {
                                    record.status = ToolCallStatus::Error;
                                    records.push(record);
                                    self.publish_tools(message_id, &records);
                                    let _ = chats
                                        .save_output(message_id, &content, &reasoning, None, None);
                                    self.update_message(message_id, MessageStatus::Cancelled, None);
                                    return;
                                }
                            }
                        } else {
                            record.status = ToolCallStatus::Denied;
                            denied_result()
                        }
                    }
                    _ => {
                        record.status = ToolCallStatus::Error;
                        record.result = Some("unknown tool".into());
                        format!("Unknown tool `{}`.", call.name)
                    }
                };
                records.push(record);
                self.publish_tools(message_id, &records);
                convo.push(ChatMessage::tool_result(&call_id, model_text));
            }
            self.update_message(message_id, MessageStatus::Running, None);
        }

        let out = last.unwrap_or_default();
        let _ = chats.save_output(
            message_id,
            &content,
            &reasoning,
            out.tokens,
            out.tokens_per_second,
        );
        let ctx_size = self.lock().loaded.as_ref().map(|l| l.ctx);
        if let (Some(prompt_tokens), Some(ctx_size)) = (out.prompt_tokens, ctx_size) {
            let (system_chars, messages_chars, tools_chars) = shape;
            let (messages, system, tools) = lokalsi_core::chats::context_breakdown(
                prompt_tokens,
                system_chars,
                messages_chars,
                tools_chars,
            );
            let usage = ContextUsage {
                prompt_tokens,
                completion_tokens: answer_tokens(out.tokens, &out.content, &out.reasoning),
                ctx_size: i64::from(ctx_size),
                messages,
                system,
                tools,
            };
            tracing::debug!(
                message_id,
                prompt_tokens,
                used = usage.used(),
                ctx_size,
                "context window use"
            );
            if let Err(e) = chats.save_context(message_id, &usage) {
                tracing::warn!(error = %e, message_id, "cannot save context window use");
            }
        }
        tracing::info!(
            message_id,
            tokens = ?out.tokens,
            tokens_per_second = ?out.tokens_per_second,
            tool_calls = records.len(),
            cancelled = out.cancelled,
            finish_reason = out.finish_reason.as_deref().unwrap_or("-"),
            "chat generation finished"
        );
        tracing::debug!(answer = %lokalsi_core::logging::loggable_content(&content), "chat answer");
        let status = if out.cancelled {
            MessageStatus::Cancelled
        } else {
            MessageStatus::Done
        };
        self.update_message(message_id, status, None);
    }

    // --- one-shot completions (pipelines, phone; M5) ------------------------------------

    /// Waits for this slot's turn, installs the engine if needed, makes room
    /// in GPU memory and loads `model`. Returns the turn (hold it while the
    /// request runs) and the server's `(base_url, token)`.
    async fn ready(
        self: &Arc<Self>,
        model: &LoadRequest,
        cancel: &CancellationToken,
        on_status: &mut (dyn FnMut(MessageStatus) + Send),
    ) -> Result<(tokio::sync::MutexGuard<'_, ()>, (String, String)), (&'static str, String)> {
        let interrupted = || (error_codes::INTERRUPTED, "cancelled".to_owned());
        let guard = tokio::select! {
            g = self.generation.lock() => g,
            _ = cancel.cancelled() => return Err(interrupted()),
        };
        let endpoint = self.load_for_turn(model, cancel, on_status).await?;
        Ok((guard, endpoint))
    }

    /// Like [`Self::ready`] for the local server (ADR 0025): the returned
    /// [`Upstream`] owns the turn, so a streamed answer can be passed through
    /// to the client; dropping it lets the next request in.
    pub async fn upstream(
        self: &Arc<Self>,
        model: &LoadRequest,
        cancel: &CancellationToken,
        on_status: &mut (dyn FnMut(MessageStatus) + Send),
    ) -> Result<Upstream, (&'static str, String)> {
        let interrupted = || (error_codes::INTERRUPTED, "cancelled".to_owned());
        let turn = tokio::select! {
            g = Arc::clone(&self.generation).lock_owned() => g,
            _ = cancel.cancelled() => return Err(interrupted()),
        };
        let (base_url, token) = self.load_for_turn(model, cancel, on_status).await?;
        Ok(Upstream {
            base_url,
            token,
            model_name: model.name.clone(),
            _turn: turn,
        })
    }

    /// Installs the engine if needed, makes room and loads `model`.
    /// The caller holds the turn (`generation`).
    async fn load_for_turn(
        self: &Arc<Self>,
        model: &LoadRequest,
        cancel: &CancellationToken,
        on_status: &mut (dyn FnMut(MessageStatus) + Send),
    ) -> Result<(String, String), (&'static str, String)> {
        let interrupted = || (error_codes::INTERRUPTED, "cancelled".to_owned());
        if self.installed().ok().flatten().is_none() {
            on_status(MessageStatus::Preparing);
            tokio::select! {
                r = self.ensure_installed() => r.map_err(|e| (error_codes::ENGINE_INSTALL, e.to_string()))?,
                _ = cancel.cancelled() => return Err(interrupted()),
            }
        }
        let ctx = model.context(&[]);
        if self.needs_load(model, ctx) {
            on_status(MessageStatus::LoadingModel);
            if let Some(s) = self.scheduler() {
                s.make_room(
                    self.gpu_memory_mb(model, self.gpu_layers(model)),
                    &self.slot,
                    false,
                    cancel,
                )
                .await
                .ok_or_else(interrupted)?;
            }
        }
        self.ensure_loaded(model, ctx, cancel).await.map_err(|e| {
            if cancel.is_cancelled() {
                interrupted()
            } else {
                e
            }
        })?;
        on_status(MessageStatus::Running);
        if let Some(s) = self.scheduler() {
            s.touch(&self.slot);
        }
        self.lock()
            .server
            .as_ref()
            .map(|s| (s.base_url(), s.token().to_owned()))
            .ok_or((error_codes::ENGINE_CRASHED, String::new()))
    }

    /// Answers typed questions about `state` with a decision model through
    /// `/v1/systemone` (ADR 0024). Installs and loads like [`Self::complete`].
    pub async fn decide(
        self: &Arc<Self>,
        model: &LoadRequest,
        state: &str,
        questions: &[lokalsi_core::decisions::Question],
        cancel: &CancellationToken,
        on_status: &mut (dyn FnMut(MessageStatus) + Send),
    ) -> Result<lokalsi_core::decisions::Decision, (&'static str, String)> {
        let interrupted = || (error_codes::INTERRUPTED, "cancelled".to_owned());
        lokalsi_core::decisions::validate(state, questions)
            .map_err(|e| (error_codes::DECISION_INVALID, e.to_string()))?;
        let (_guard, (base_url, token)) = self.ready(model, cancel, on_status).await?;
        tracing::info!(model = %model.model_id, questions = questions.len(), "decision started");
        tracing::debug!(state = %lokalsi_core::logging::loggable_content(state), "decision state");
        let started = Instant::now();
        let out = tokio::select! {
            r = llama::decide_at(&self.http, &base_url, &token, state, questions) => r,
            _ = cancel.cancelled() => return Err(interrupted()),
        };
        let decision = out.map_err(|e| {
            tracing::error!(error = %e, "decision failed");
            (e.code(), e.to_string())
        })?;
        tracing::info!(
            tokens = ?decision.input_tokens,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "decision finished"
        );
        Ok(decision)
    }

    /// Generates one answer to `messages` without a chat session and without
    /// tools: installs the engine if needed, makes room in GPU memory, loads
    /// the model and waits for its turn like a chat job. `on_status` reports
    /// `preparing` / `loading_model` / `running`. Without `params.max_tokens`
    /// the answer is limited by [`default_answer_tokens`], so a thinking model
    /// cannot run until the context window is full (ADR 0025).
    /// Errors are `(code, detail)` with `error_codes`; cancelling returns `interrupted`.
    pub async fn complete(
        self: &Arc<Self>,
        model: &LoadRequest,
        messages: &[ChatMessage],
        params: &ChatParams,
        cancel: &CancellationToken,
        on_status: &mut (dyn FnMut(MessageStatus) + Send),
    ) -> Result<llama::ChatOutcome, (&'static str, String)> {
        let interrupted = || (error_codes::INTERRUPTED, "cancelled".to_owned());
        let (_guard, endpoint) = self.ready(model, cancel, on_status).await?;
        let mut params = self.with_model_defaults(model, params.clone());
        if params.max_tokens.is_none() {
            params.max_tokens = Some(default_answer_tokens(messages, model.context(&[])));
        }
        let params = &params;
        tracing::info!(model = %model.model_id, messages = messages.len(), max_tokens = ?params.max_tokens, "completion started");
        tracing::debug!(
            prompt = %lokalsi_core::logging::loggable_content(messages.last().and_then(|m| m.content.as_deref()).unwrap_or("")),
            "completion prompt"
        );
        let started = Instant::now();
        let out = stream_with(
            &self.http,
            &endpoint,
            messages,
            &[],
            params,
            cancel,
            &mut |_, _| {},
        )
        .await
        .map_err(|failed| {
            let (_, err) = *failed;
            let code = if err.is_context_full() {
                error_codes::CONTEXT_FULL
            } else {
                error_codes::ENGINE_ERROR
            };
            tracing::error!(error = %err, "completion failed");
            (code, err.to_string())
        })?;
        if out.cancelled {
            return Err(interrupted());
        }
        tracing::info!(
            tokens = ?out.tokens,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "completion finished"
        );
        tracing::debug!(answer = %lokalsi_core::logging::loggable_content(&out.content), "completion answer");
        Ok(out)
    }

    // --- tools (M2b) --------------------------------------------------------------------

    pub fn set_tool_host(&self, host: Arc<dyn ToolHost>) {
        *self.tool_host.lock().unwrap_or_else(|p| p.into_inner()) = Some(host);
    }

    fn tool_host(&self) -> Option<Arc<dyn ToolHost>> {
        self.tool_host
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn publish_tools(&self, message_id: i64, records: &[ToolCallRecord]) {
        let json = serde_json::to_value(records).unwrap_or_else(|_| serde_json::json!([]));
        let chats = Chats::new(&self.db);
        if let Err(e) = chats.save_tool_calls(message_id, &json) {
            tracing::warn!(message_id, error = %e, "cannot save tool calls");
        }
        if let Ok(Some(m)) = chats.message(message_id) {
            self.events.chat_updated(&m);
        }
    }

    /// Waits for the user's answer to a consent prompt; `None` when cancelled.
    async fn wait_for_decision(
        &self,
        call_id: &str,
        cancel: &CancellationToken,
    ) -> Option<Decision> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.consents
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(call_id.to_owned(), tx);
        tracing::info!(call_id, "waiting for the user to allow a tool call");
        let decision = tokio::select! {
            d = rx => d.ok(),
            _ = cancel.cancelled() => None,
        };
        self.consents
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(call_id);
        decision
    }

    /// The user's answer from the UI. Returns false when nothing was waiting.
    pub fn decide_tool(&self, call_id: &str, decision: Decision) -> bool {
        tracing::info!(
            call_id,
            allow = decision.allow,
            remember = decision.remember,
            "tool call decision"
        );
        match self
            .consents
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(call_id)
        {
            Some(tx) => tx.send(decision).is_ok(),
            None => false,
        }
    }
}
/// `stream_chat` against a server identified by `(base_url, token)`, keeping
/// the partial output when the stream breaks.
/// Tokens of the answer that the next prompt will contain: the reasoning is
/// not sent back, so its share (by characters) is left out (ADR 0041).
fn answer_tokens(tokens: Option<i64>, content: &str, reasoning: &str) -> i64 {
    let tokens = tokens.unwrap_or(0);
    let (c, r) = (content.chars().count(), reasoning.chars().count());
    if c + r == 0 {
        return 0;
    }
    (tokens as f64 * c as f64 / (c + r) as f64).round() as i64
}

/// Characters of the system message, the other messages and the tool
/// definitions of one request, for the context breakdown (ADR 0041).
fn prompt_shape(convo: &[ChatMessage], tools: &[serde_json::Value]) -> (usize, usize, usize) {
    let len = |m: &ChatMessage| {
        m.content.as_deref().map_or(0, |c| c.chars().count())
            + m.tool_calls
                .as_ref()
                .map_or(0, |t| t.to_string().chars().count())
    };
    let (system, rest) = match convo.split_first() {
        Some((first, rest)) if first.role == "system" => (len(first), rest),
        _ => (0, convo),
    };
    let tools = if tools.is_empty() {
        0
    } else {
        serde_json::Value::Array(tools.to_vec())
            .to_string()
            .chars()
            .count()
    };
    (system, rest.iter().map(len).sum(), tools)
}

async fn stream_with(
    http: &reqwest::Client,
    (base, token): &(String, String),
    history: &[ChatMessage],
    tools: &[serde_json::Value],
    params: &ChatParams,
    cancel: &CancellationToken,
    on_delta: &mut (dyn FnMut(&str, &str) + Send),
) -> Result<llama::ChatOutcome, Box<(llama::ChatOutcome, ChatError)>> {
    let mut partial = llama::ChatOutcome::default();
    let mut track = |c: &str, r: &str| {
        partial.content.push_str(c);
        partial.reasoning.push_str(r);
        on_delta(c, r);
    };
    let res = llama::stream_chat_at(
        http, base, token, history, tools, params, cancel, &mut track,
    )
    .await;
    res.map_err(|e| Box::new((partial, e)))
}

/// Share of `estimate_mb` on the GPU with `on_gpu` of `total` layers there.
/// Unknown `total`: all of it, unless nothing is on the GPU.
fn gpu_share_mb(estimate_mb: u64, on_gpu: u32, total: Option<u32>) -> u64 {
    match total {
        _ if on_gpu == 0 => 0,
        Some(total) if total > 0 && on_gpu < total => {
            estimate_mb * u64::from(on_gpu) / u64::from(total)
        }
        _ => estimate_mb,
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn answer_tokens_leave_reasoning_out() {
        assert_eq!(answer_tokens(Some(100), "abc", ""), 100);
        assert_eq!(answer_tokens(Some(100), "a", "bcd"), 25);
        assert_eq!(answer_tokens(Some(100), "", ""), 0);
        assert_eq!(answer_tokens(None, "abc", "d"), 0);
    }

    use super::*;

    #[test]
    fn gpu_share_scales_with_offloaded_layers() {
        assert_eq!(gpu_share_mb(8000, 0, Some(32)), 0, "processor only");
        assert_eq!(gpu_share_mb(8000, 16, Some(32)), 4000);
        assert_eq!(gpu_share_mb(8000, 999, Some(32)), 8000, "all layers");
        assert_eq!(
            gpu_share_mb(8000, 16, None),
            8000,
            "unknown count keeps the estimate"
        );
    }

    struct NoEvents;
    impl EngineEvents for NoEvents {}

    #[test]
    fn one_shot_answers_get_a_limit() {
        let short = [llama::ChatMessage::text(
            "user",
            "Napisz jedno zdanie o górach.",
        )];
        assert_eq!(
            default_answer_tokens(&short, 32_768),
            DEFAULT_ANSWER_TOKENS + 9
        );
        // A long text to translate: room for an answer as long as the text.
        let long = [llama::ChatMessage::text("user", "x".repeat(30_000))];
        assert_eq!(
            default_answer_tokens(&long, 32_768),
            DEFAULT_ANSWER_TOKENS + 10_000
        );
        // Never more than what is left of the window.
        assert_eq!(default_answer_tokens(&long, 12_000), 2_000);
        assert_eq!(default_answer_tokens(&long, 8_192), 256);
    }

    #[test]
    fn context_fits_tools_and_memory() {
        let tools = vec![serde_json::json!({ "d": "x".repeat(90_000) })]; // ≈30k tokens
        assert_eq!(context_for(&[]), DEFAULT_CTX);
        assert_eq!(context_for(&tools), MAX_TOOLS_CTX);
        // 24 GB card, 9 GB model → room for the largest automatic window.
        assert_eq!(ctx_limit(Some(24_000), 9_000), MAX_TOOLS_CTX);
        assert!(!ctx_plan(Some(24_000), 9_000).kv_compact);
        // 12 GB card, same model: an f16 cache would give ≈14k tokens (8192);
        // the compact cache takes half the memory → the largest window.
        assert_eq!(
            ctx_plan(Some(12_000), 9_000),
            CtxPlan {
                ctx_limit: MAX_TOOLS_CTX,
                kv_compact: true
            }
        );
        // 8 GB card (7 680 MB budget), Gemma 4 12B IQ4_XS (7 713 MB estimate):
        // f16 leaves no room at all, compact gives 8k (measured: 32k fits).
        assert_eq!(
            ctx_plan(Some(7_680), 7_713),
            CtxPlan {
                ctx_limit: 8192,
                kv_compact: true
            }
        );
        // Bielik 11B Q6_K (10 638 MB) on a 12 GB card: 16k with q8_0 measured
        // at ≈10.9 GB.
        assert_eq!(ctx_limit(Some(11_776), 10_638), 16384);
        // Model does not fit even without a cache: the smallest window.
        assert_eq!(ctx_limit(Some(9_200), 10_500), MIN_CTX);
        assert_eq!(ctx_plan(None, 9_000).ctx_limit, DEFAULT_CTX);
        assert!(!ctx_plan(None, 9_000).kv_compact);
        // The server's larger window: 24 GB card, 9 GB model → 64k compact,
        // 8 GB card → no more than the chat gets.
        assert_eq!(
            ctx_plan_up_to(Some(16_800), 8_500, MAX_SERVER_CTX),
            CtxPlan {
                ctx_limit: MAX_SERVER_CTX,
                kv_compact: true
            }
        );
        assert_eq!(
            ctx_plan_up_to(Some(7_680), 7_713, MAX_SERVER_CTX),
            ctx_plan(Some(7_680), 7_713)
        );

        let mut req = LoadRequest {
            model_id: "m".into(),
            variant_id: "v".into(),
            name: "M".into(),
            path: PathBuf::new(),
            memory_estimate_mb: 9_000,
            supports_tools: true,
            ctx_size: None,
            ctx_limit: 8192,
            kv_compact: true,
        };
        assert_eq!(req.context(&tools), 8192, "auto is capped by memory");
        req.ctx_size = Some(65536);
        assert_eq!(req.context(&tools), 65536, "the user's choice wins");
    }

    fn service(dir: &Path) -> Arc<EngineService> {
        service_on(dir, "windows-x64")
    }

    fn service_on(dir: &Path, platform: &str) -> Arc<EngineService> {
        let db = Arc::new(Db::open(&dir.join("app.db")).unwrap());
        EngineService::new(
            EngineManifest::llama_cpp(),
            db,
            reqwest::Client::new(),
            platform.into(),
            dir.join("engines"),
            &dir.join("logs"),
            Arc::new(NoEvents),
        )
    }

    #[test]
    fn starts_not_installed_with_build_options() {
        let dir = tempfile::tempdir().unwrap();
        let s = service(dir.path());
        let st = s.status(None);
        assert_eq!(st.state, EngineState::NotInstalled);
        assert_eq!(
            st.builds.len(),
            1,
            "without a hardware profile only the CPU build is offered"
        );
        assert!(st.builds[0].recommended);
    }

    #[tokio::test]
    async fn a_second_slot_shares_the_install_but_not_the_resident() {
        let dir = tempfile::tempdir().unwrap();
        let primary = service(dir.path());
        let scheduler = Scheduler::new();
        primary.set_scheduler(Arc::clone(&scheduler));
        let decider = EngineService::second_slot(&primary, "llama-cpp-decision");
        assert_eq!(decider.slot(), "llama-cpp-decision");
        assert_eq!(primary.slot(), "llama-cpp");
        assert!(decider.log_path.ends_with("engine-llama-cpp-decision.log"));
        assert_eq!(primary.slots.lock().unwrap().len(), 1);
        // Both slots see the same installed build.
        primary
            .db
            .conn()
            .execute(
                "INSERT INTO installed_engines (engine_id, build_id, version, backend, dir, installed_at)
                 VALUES ('llama-cpp', 'b', 'v', 'cpu', ?1, 0)",
                params![dir.path().join("engines").to_string_lossy()],
            )
            .unwrap();
        assert!(decider.installed().unwrap().is_some());
        // Two residents of one engine, released separately.
        scheduler.resident_loaded(primary.slot(), "chat", 1000);
        scheduler.resident_loaded(decider.slot(), "decision", 500);
        let keys: Vec<String> = scheduler
            .status()
            .items
            .into_iter()
            .map(|i| i.key)
            .collect();
        assert!(keys.contains(&"resident:llama-cpp".to_owned()), "{keys:?}");
        assert!(
            keys.contains(&"resident:llama-cpp-decision".to_owned()),
            "{keys:?}"
        );
        // Without a model file the decision fails before any server starts.
        let load = LoadRequest {
            model_id: "basal".into(),
            variant_id: "q8_0".into(),
            name: "Basal".into(),
            path: dir.path().join("missing.gguf"),
            memory_estimate_mb: 500,
            supports_tools: false,
            ctx_size: Some(4096),
            ctx_limit: 4096,
            kv_compact: false,
        };
        let q = lokalsi_core::decisions::Question {
            id: "q".into(),
            instructions: "Spam?".into(),
            kind: lokalsi_core::decisions::QuestionKind::YesNo,
        };
        let err = decider
            .decide(
                &load,
                "",
                std::slice::from_ref(&q),
                &CancellationToken::new(),
                &mut |_| {},
            )
            .await
            .unwrap_err();
        assert_eq!(err.0, error_codes::DECISION_INVALID);
        let err = decider
            .decide(&load, "tekst", &[q], &CancellationToken::new(), &mut |_| {})
            .await
            .unwrap_err();
        assert_eq!(err.0, error_codes::ENGINE_MISSING);
        assert!(primary.loaded().is_none());
    }

    #[tokio::test]
    async fn chat_installs_the_engine_first_and_reports_failures() {
        let dir = tempfile::tempdir().unwrap();
        // No build exists for this platform, so the automatic install fails fast (no network).
        let s = service_on(dir.path(), "plan9-x64");
        let chats = Chats::new(&s.db);
        let session = chats.create_session("t").unwrap();
        chats
            .add_message(session.id, Role::User, "hej", None, MessageStatus::Done)
            .unwrap();
        let reply = chats
            .add_message(
                session.id,
                Role::Assistant,
                "",
                Some(("m", "v")),
                MessageStatus::Queued,
            )
            .unwrap();
        let req = LoadRequest {
            model_id: "m".into(),
            variant_id: "v".into(),
            name: "M".into(),
            path: dir.path().join("missing.gguf"),
            memory_estimate_mb: 100,
            supports_tools: false,
            ctx_size: None,
            ctx_limit: DEFAULT_CTX,
            kv_compact: false,
        };
        Arc::clone(&s)
            .run_chat(reply.id, req, ChatParams::default())
            .await;
        let m = chats.message(reply.id).unwrap().unwrap();
        assert_eq!(m.status, MessageStatus::Failed);
        assert!(m.error.unwrap().starts_with(error_codes::ENGINE_INSTALL));
    }
}

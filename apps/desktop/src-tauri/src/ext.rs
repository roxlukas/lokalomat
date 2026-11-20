//! What an edition adds to the desktop shell (ADR 0035).
//!
//! The free app works with [`NoExtension`]. The Pro edition supplies an
//! [`Extension`]: its own IPC commands, engines (speech, images…), servers'
//! extra endpoints and the lifecycle work around them. The free code never
//! names a Pro type; it only calls these hooks.

use std::any::Any;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use lokalsi_api_gateway::openai::ServerExtension;
use lokalsi_api_gateway::server::GatewayExtension;
use lokalsi_core::catalog::Modality;
use lokalsi_core::db::Db;
use lokalsi_core::edition::Edition;
use lokalsi_engines::modes::ModeRunner;
use lokalsi_engines::queue::JobQueue;
use lokalsi_engines::scheduler::Scheduler;
use lokalsi_engines::service::{EngineService, EngineStatus};
use lokalsi_hwprobe::HardwareProfile;
use tauri::AppHandle;
use tauri::ipc::Invoke;

use crate::events::TauriEvents;
use crate::resolve::DesktopResolver;

pub type BoxFut<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// What the free app has built when it asks an edition for its extension.
pub struct ExtContext {
    pub db: Arc<Db>,
    pub events: Arc<TauriEvents>,
    pub llm: Arc<EngineService>,
    pub scheduler: Arc<Scheduler>,
    /// The job queue of the app (decisions run on it; Pro jobs join it).
    pub queue: Arc<JobQueue>,
    pub modes: Arc<ModeRunner>,
    pub resolver: Arc<DesktopResolver>,
    pub home: PathBuf,
    pub data_root: PathBuf,
    pub logs_dir: PathBuf,
}

/// Builds the extension of an edition once the free parts exist.
pub type ExtensionFactory = Box<dyn FnOnce(&ExtContext) -> Arc<dyn Extension>>;

/// Hooks of an edition. Every method has a free-edition default that does nothing.
pub trait Extension: Send + Sync + 'static {
    /// For the extension's own code to get its state back.
    fn as_any(&self) -> &dyn Any;

    /// Which edition runs (the free app: [`Edition::Free`]).
    fn edition(&self) -> Edition {
        Edition::current()
    }

    /// The kinds of models in this edition's catalog (the shipped list is filtered by them).
    fn modalities(&self) -> &'static [lokalsi_core::catalog::Modality] {
        self.edition().modalities()
    }

    // --- IPC ---------------------------------------------------------------------

    /// Is `command` one of the extension's IPC commands?
    fn handles(&self, _command: &str) -> bool {
        false
    }

    /// Runs an IPC command for which [`Extension::handles`] answered `true`.
    fn invoke(&self, _invoke: Invoke) -> bool {
        false
    }

    // --- engines -----------------------------------------------------------------

    /// Status of the engines the extension owns (after llama.cpp in the Engines view).
    fn engine_statuses(&self, _profile: Option<&HardwareProfile>) -> Vec<EngineStatus> {
        Vec::new()
    }

    /// Starts installing engine `id` in the background; `false` when it is not the extension's.
    fn install_engine(
        &self,
        _id: &str,
        _build_id: Option<String>,
        _engines_dir: PathBuf,
        _profile: Option<HardwareProfile>,
        _repair: bool,
    ) -> bool {
        false
    }

    fn cancel_engine_install(&self, _id: &str) -> bool {
        false
    }

    fn uninstall_engine(&self, _id: &str) -> Option<BoxFut<Result<(), String>>> {
        None
    }

    /// `Err((code, detail))` like the free engines.
    fn restart_engine(&self, _id: &str) -> Option<BoxFut<Result<(), (String, String)>>> {
        None
    }

    fn engine_log_path(&self, _id: &str) -> Option<PathBuf> {
        None
    }

    /// Installs the engine of a model of `modality` that just arrived or is already there
    /// (plug and play). The free app handles chat and decision models itself.
    fn ensure_engine_for(&self, _app: &AppHandle, _modality: Modality) {}

    // --- lifecycle ---------------------------------------------------------------

    /// The hardware profile changed (or was loaded).
    fn on_hardware(&self, _profile: Option<HardwareProfile>) {}

    /// The data folder changed.
    fn on_data_dir(&self, _app: &AppHandle, _dir: &Path) {}

    /// The app started: the Tauri setup closure.
    fn setup(&self, _app: &AppHandle) {}

    /// The app is closing: stop what the extension started (blocking).
    fn exit(&self) {}

    // --- servers -----------------------------------------------------------------

    /// More endpoints and MCP for the local OpenAI-compatible server.
    fn server_extension(&self, _app: &AppHandle) -> Option<Arc<dyn ServerExtension>> {
        None
    }

    /// More routes and modes for the phone gateway.
    fn gateway_extension(&self, _app: &AppHandle) -> Option<Arc<dyn GatewayExtension>> {
        None
    }
}

/// The free edition: nothing is added.
pub struct NoExtension;

impl Extension for NoExtension {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

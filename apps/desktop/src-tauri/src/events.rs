//! Core events forwarded to the UI as Tauri events.

use std::sync::OnceLock;

use lokalsi_core::chats::Message;
use lokalsi_core::downloads::{DownloadEvents, Progress};
use lokalsi_engines::service::{EngineEvents, EngineStatus};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const DOWNLOADS_CHANGED: &str = "downloads://changed";
pub const DOWNLOADS_PROGRESS: &str = "downloads://progress";
pub const MODEL_INSTALLED: &str = "models://installed";
pub const HARDWARE_UPDATED: &str = "hardware://updated";
pub const BENCHMARK_UPDATED: &str = "benchmark://updated";
pub const ENGINES_CHANGED: &str = "engines://changed";
pub const CHAT_DELTA: &str = "chat://delta";
pub const CHAT_UPDATED: &str = "chat://updated";
pub const JOBS_UPDATED: &str = "jobs://updated";
pub const JOBS_PROGRESS: &str = "jobs://progress";
pub const MEMORY_CHANGED: &str = "memory://changed";
pub const PAIRING_CHANGED: &str = "network://pairing";
pub const DEVICES_CHANGED: &str = "network://devices";

#[derive(Default)]
pub struct TauriEvents {
    app: OnceLock<AppHandle>,
}

impl TauriEvents {
    pub fn attach(&self, app: AppHandle) {
        let _ = self.app.set(app);
    }

    pub fn emit<T: Serialize + Clone>(&self, event: &str, payload: T) {
        if let Some(app) = self.app.get()
            && let Err(e) = app.emit(event, payload)
        {
            tracing::warn!(event, error = %e, "cannot emit event");
        }
    }
}

#[derive(Clone, Serialize)]
struct Installed<'a> {
    model_id: &'a str,
    variant_id: &'a str,
}

impl DownloadEvents for TauriEvents {
    fn progress(&self, progress: &Progress) {
        self.emit(DOWNLOADS_PROGRESS, progress.clone());
    }

    fn changed(&self) {
        self.emit(DOWNLOADS_CHANGED, ());
    }

    fn variant_installed(&self, model_id: &str, variant_id: &str) {
        // A chat model just arrived: get its engine ready right away (plug and play).
        if let Some(app) = self.app.get() {
            let state = tauri::Manager::state::<crate::AppState>(app);
            // Any model that just arrived gets its engine ready (plug and play).
            if let Ok((m, _)) =
                lokalsi_core::library::Library::new(&state.catalog, &state.db).find(model_id)
            {
                crate::chat::ensure_engine_for(app, m.modality);
            }
        }
        self.emit(
            MODEL_INSTALLED,
            Installed {
                model_id,
                variant_id,
            },
        );
    }
}

#[derive(Clone, Serialize)]
struct ChatDelta<'a> {
    message_id: i64,
    content: &'a str,
    reasoning: &'a str,
}

impl EngineEvents for TauriEvents {
    fn engine_changed(&self, status: &EngineStatus) {
        self.emit(ENGINES_CHANGED, status.clone());
    }

    fn chat_delta(&self, message_id: i64, content: &str, reasoning: &str) {
        self.emit(
            CHAT_DELTA,
            ChatDelta {
                message_id,
                content,
                reasoning,
            },
        );
    }

    fn chat_updated(&self, message: &Message) {
        self.emit(CHAT_UPDATED, message.clone());
    }
}

#[derive(Clone, Serialize)]
struct JobProgress {
    job_id: i64,
    progress: f64,
}

impl lokalsi_engines::queue::MediaEvents for TauriEvents {
    fn job_updated(&self, job: &lokalsi_core::jobs::Job) {
        self.emit(JOBS_UPDATED, job.clone());
    }

    fn job_progress(&self, job_id: i64, progress: f64) {
        self.emit(JOBS_PROGRESS, JobProgress { job_id, progress });
    }
}

impl TauriEvents {
    pub fn memory_changed(&self, status: &lokalsi_engines::scheduler::MemoryStatus) {
        self.emit(MEMORY_CHANGED, status.clone());
    }
}

impl lokalsi_api_gateway::server::GatewayEvents for TauriEvents {
    fn pairing_changed(&self, view: &lokalsi_api_gateway::server::PairingView) {
        self.emit(PAIRING_CHANGED, view.clone());
    }

    fn devices_changed(&self) {
        self.emit(DEVICES_CHANGED, ());
    }
}

//! Runs one Workshop mode to completion: used by pipelines (M5) and by the
//! local API for the phone. Chat uses a one-shot completion and decisions go through the
//! [`JobQueue`]; the media modes (image, speech, transcription, songs) are in
//! `media_modes.rs`.
//!
//! Which model to use is answered by a [`ModelResolver`]: the app resolves
//! like its own commands do (default model, else the first installed one);
//! [`LibraryResolver`] does the same from the catalog and the database.

use std::path::PathBuf;
use std::sync::Arc;

use lokalsi_core::catalog::ModelRef;
use lokalsi_core::catalog::{Catalog, Modality, Model, Variant};
use lokalsi_core::chats::MessageStatus;
use lokalsi_core::db::Db;
use lokalsi_core::decisions::{self, Decision, Question};
use lokalsi_core::jobs::{Job, JobStatus, Jobs};
use lokalsi_core::library::Library;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::llama::{ChatMessage, ChatParams};
use crate::queue::{DecisionRequest, JobQueue};
use crate::service::{
    EngineService, LoadRequest, MAX_SERVER_CTX, Upstream, chosen_ctx, ctx_plan, ctx_plan_up_to,
    error_codes,
};

/// A failure with a code the UI translates (`jobs.errors.*` / `errors.*`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModeError {
    pub code: String,
    pub detail: String,
}

impl ModeError {
    pub fn new(code: &str, detail: impl Into<String>) -> Self {
        ModeError {
            code: code.to_owned(),
            detail: detail.into(),
        }
    }

    /// From a job's stored error (`"code: detail"`).
    pub fn from_job(job: &Job) -> Self {
        if job.status == JobStatus::Cancelled {
            return ModeError::new(error_codes::INTERRUPTED, "cancelled");
        }
        let raw = job.error.clone().unwrap_or_default();
        match raw.split_once(": ") {
            Some((code, detail)) => ModeError::new(code, detail),
            None => ModeError::new(if raw.is_empty() { "error" } else { &raw }, ""),
        }
    }
}

impl std::fmt::Display for ModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

/// A downloaded model with the local path of every file (`(role, path)`).
#[derive(Debug, Clone)]
pub struct Resolved {
    pub model: Model,
    pub variant: Variant,
    pub files: Vec<(String, PathBuf)>,
}

impl Resolved {
    pub fn display_name(&self) -> String {
        format!("{} {}", self.model.name, self.variant.label)
    }

    pub fn main_file(&self) -> Option<PathBuf> {
        self.files
            .iter()
            .find(|(role, _)| role == "model")
            .or(self.files.first())
            .map(|(_, p)| p.clone())
    }
}

pub trait ModelResolver: Send + Sync + 'static {
    /// The requested model, else the default of `modality`, else the first
    /// installed one. `cloning`: speech needs a model that clones voices.
    fn resolve(
        &self,
        modality: Modality,
        requested: Option<&ModelRef>,
        cloning: bool,
    ) -> Result<Resolved, ModeError>;
    /// Every installed model of `modality` that `resolve` would accept when
    /// asked for it by name (model pickers on the phone).
    fn installed(&self, _modality: Modality) -> Vec<Resolved> {
        Vec::new()
    }
    /// Context window chosen in the settings (`None` = automatic,
    /// [`crate::service::CTX_MAX`] = the model's own maximum).
    fn context_size(&self) -> Option<u32> {
        None
    }
    /// Context window chosen for the local server, like [`Self::context_size`].
    fn server_context_size(&self) -> Option<u32> {
        None
    }
    /// GPU memory budget in MB, used for the automatic context window.
    fn budget_mb(&self) -> Option<u64> {
        None
    }
}

pub fn has_cloning(model: &Model) -> bool {
    model.features.iter().any(|f| f == "voice_cloning")
}

/// Resolves models from the catalog and the database (examples, tests).
pub struct LibraryResolver {
    pub catalog: Catalog,
    pub db: Arc<Db>,
    pub budget_mb: Option<u64>,
}

impl LibraryResolver {
    fn local(&self, m: &Model, v: &Variant) -> Option<Vec<(String, PathBuf)>> {
        let installed = self.db.installed_files().ok()?;
        v.files
            .iter()
            .map(|f| {
                installed
                    .iter()
                    .find(|i| i.model_id == m.id && i.variant_id == v.id && i.file_path == f.path)
                    .map(|i| {
                        (
                            f.role.clone().unwrap_or_else(|| "model".into()),
                            PathBuf::from(&i.local_path),
                        )
                    })
            })
            .collect()
    }
}

impl ModelResolver for LibraryResolver {
    fn resolve(
        &self,
        modality: Modality,
        requested: Option<&ModelRef>,
        cloning: bool,
    ) -> Result<Resolved, ModeError> {
        let lib = Library::new(&self.catalog, &self.db);
        let all = lib
            .all_models()
            .map_err(|e| ModeError::new("error", e.to_string()))?;
        let pick = |model_id: &str, variant_id: &str| -> Result<Resolved, ModeError> {
            let (model, _) = all
                .iter()
                .find(|(m, _)| m.id == model_id)
                .ok_or_else(|| ModeError::new("no_model", model_id))?;
            let variant = model
                .variant(variant_id)
                .ok_or_else(|| ModeError::new("no_model", variant_id))?;
            if model.modality != modality {
                return Err(ModeError::new("no_media_model", "wrong kind of model"));
            }
            let files = self
                .local(model, variant)
                .ok_or_else(|| ModeError::new("model_not_installed", model.name.clone()))?;
            Ok(Resolved {
                model: model.clone(),
                variant: variant.clone(),
                files,
            })
        };
        if let Some(r) = requested {
            return pick(&r.model_id, &r.variant_id);
        }
        let default = self
            .db
            .default_models()
            .ok()
            .and_then(|d| d.into_iter().find(|(k, _, _)| k == modality.as_str()));
        if let Some((_, m, v)) = default
            && let Ok(r) = pick(&m, &v)
            && (!cloning || has_cloning(&r.model))
        {
            return Ok(r);
        }
        all.iter()
            .filter(|(m, _)| m.modality == modality && (!cloning || has_cloning(m)))
            .find_map(|(m, _)| m.variants.iter().find_map(|v| pick(&m.id, &v.id).ok()))
            .ok_or_else(|| {
                if cloning {
                    ModeError::new("no_cloning_model", "no voice cloning model installed")
                } else if modality == Modality::Llm {
                    ModeError::new("no_model", "no chat model installed")
                } else {
                    ModeError::new("no_media_model", "no model installed")
                }
            })
    }

    fn installed(&self, modality: Modality) -> Vec<Resolved> {
        let lib = Library::new(&self.catalog, &self.db);
        let Ok(all) = lib.all_models() else {
            return Vec::new();
        };
        all.iter()
            .filter(|(m, _)| m.modality == modality)
            .flat_map(|(m, _)| {
                m.variants.iter().filter_map(|v| {
                    self.resolve(
                        modality,
                        Some(&ModelRef {
                            model_id: m.id.clone(),
                            variant_id: v.id.clone(),
                        }),
                        false,
                    )
                    .ok()
                })
            })
            .collect()
    }

    fn budget_mb(&self) -> Option<u64> {
        self.budget_mb
    }
}

/// Scheduler resident and log name of the decision models' llama-server.
pub const DECISION_SLOT: &str = "llama-cpp-decision";
/// Largest context window of the decision slot (states are short texts).
pub const DECISION_CTX: u32 = 16_384;

pub struct ModeRunner {
    db: Arc<Db>,
    queue: Arc<JobQueue>,
    llm: Arc<EngineService>,
    /// Second llama.cpp slot for decision models, so chat stays loaded (ADR 0024).
    decider: Arc<EngineService>,
    resolver: Arc<dyn ModelResolver>,
}

/// Status callback of a step: the job id once created, then status changes.
pub trait Progress: Send {
    fn job(&mut self, _job_id: i64) {}
    fn status(&mut self, _status: MessageStatus) {}
}

impl Progress for () {}

impl ModeRunner {
    pub fn new(
        db: Arc<Db>,
        queue: Arc<JobQueue>,
        llm: Arc<EngineService>,
        resolver: Arc<dyn ModelResolver>,
    ) -> Arc<Self> {
        let decider = EngineService::second_slot(&llm, DECISION_SLOT);
        Arc::new(ModeRunner {
            db,
            queue,
            llm,
            decider,
            resolver,
        })
    }

    pub fn db(&self) -> &Arc<Db> {
        &self.db
    }

    pub fn queue(&self) -> &Arc<JobQueue> {
        &self.queue
    }

    pub fn resolver(&self) -> &Arc<dyn ModelResolver> {
        &self.resolver
    }

    /// The decision models' llama.cpp slot (memory meter, stop, tests).
    pub fn decider(&self) -> &Arc<EngineService> {
        &self.decider
    }

    /// Waits for a media job: cancels it when `cancel` fires, then returns the stored job.
    pub async fn wait_job(
        &self,
        job_id: i64,
        run: impl std::future::Future<Output = ()>,
        cancel: &CancellationToken,
    ) -> Result<Job, ModeError> {
        let queue = Arc::clone(&self.queue);
        let watcher_cancel = cancel.clone();
        let done = CancellationToken::new();
        let done_watch = done.clone();
        let watcher = tokio::spawn(async move {
            tokio::select! {
                _ = watcher_cancel.cancelled() => { queue.cancel(job_id); }
                _ = done_watch.cancelled() => {}
            }
        });
        run.await;
        done.cancel();
        let _ = watcher.await;
        let job = Jobs::new(&self.db)
            .get(job_id)
            .map_err(|e| ModeError::new("error", e.to_string()))?
            .ok_or_else(|| ModeError::new("not_found", "job"))?;
        if job.status == JobStatus::Done {
            Ok(job)
        } else {
            Err(ModeError::from_job(&job))
        }
    }

    pub fn tag(&self, job_id: i64, source: Option<&serde_json::Value>) {
        if let Some(src) = source
            && let Err(e) = Jobs::new(&self.db).set_source(job_id, src)
        {
            tracing::warn!(job_id, error = %e, "cannot record the job source");
        }
    }

    // --- chat ----------------------------------------------------------------------

    /// What `llama-server` needs to load the chat model.
    pub fn load_request(&self, model: Option<&ModelRef>) -> Result<LoadRequest, ModeError> {
        let r = self.resolver.resolve(Modality::Llm, model, false)?;
        let path = r
            .main_file()
            .ok_or_else(|| ModeError::new("no_model", "variant without a model file"))?;
        let plan = ctx_plan(self.resolver.budget_mb(), r.variant.memory_estimate_mb);
        let ctx_size = chosen_ctx(self.resolver.context_size(), &path);
        Ok(LoadRequest {
            model_id: r.model.id.clone(),
            variant_id: r.variant.id.clone(),
            name: r.display_name(),
            path,
            memory_estimate_mb: r.variant.memory_estimate_mb,
            supports_tools: r.model.features.iter().any(|f| f == "tools"),
            ctx_size,
            ctx_limit: plan.ctx_limit,
            kv_compact: plan.kv_compact,
        })
    }

    /// [`Self::load_request`] for the local server (ADR 0025/0028): agents get
    /// the largest window that fits, up to [`MAX_SERVER_CTX`], unless the
    /// server's own setting fixes one (ADR 0042).
    pub fn server_load_request(&self, model: Option<&ModelRef>) -> Result<LoadRequest, ModeError> {
        let mut load = self.load_request(model)?;
        let plan = ctx_plan_up_to(
            self.resolver.budget_mb(),
            load.memory_estimate_mb,
            MAX_SERVER_CTX,
        );
        load.ctx_limit = plan.ctx_limit;
        load.kv_compact = plan.kv_compact;
        load.ctx_size =
            chosen_ctx(self.resolver.server_context_size(), &load.path).or(Some(plan.ctx_limit));
        Ok(load)
    }

    /// One answer to `messages` (no session, no tools). Returns `(answer, model name)`.
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        model: Option<&ModelRef>,
        params: &ChatParams,
        progress: &mut dyn Progress,
        cancel: &CancellationToken,
    ) -> Result<(String, String), ModeError> {
        let load = self.load_request(model)?;
        let out = self
            .llm
            .complete(&load, messages, params, cancel, &mut |s| progress.status(s))
            .await
            .map_err(|(code, detail)| ModeError::new(code, detail))?;
        let answer = strip_think(&out.content);
        if answer.is_empty() {
            // A thinking model can use the whole answer limit before it answers.
            let detail = if out.finish_reason.as_deref() == Some("length") {
                "the answer limit was used up before the answer"
            } else {
                "empty answer"
            };
            return Err(ModeError::new(
                crate::service::job_errors::NO_RESULT,
                detail,
            ));
        }
        Ok((answer, load.name))
    }
}

impl ModeRunner {
    // --- decisions (ADR 0024) ----------------------------------------------------------

    /// What the decision slot needs to load a decision model.
    pub fn decision_load_request(
        &self,
        model: Option<&ModelRef>,
    ) -> Result<LoadRequest, ModeError> {
        let r = self.resolver.resolve(Modality::Decision, model, false)?;
        let path = r
            .main_file()
            .ok_or_else(|| ModeError::new("no_model", "variant without a model file"))?;
        let plan = ctx_plan(self.resolver.budget_mb(), r.variant.memory_estimate_mb);
        let ctx = plan.ctx_limit.min(DECISION_CTX);
        Ok(LoadRequest {
            model_id: r.model.id.clone(),
            variant_id: r.variant.id.clone(),
            name: r.display_name(),
            path,
            memory_estimate_mb: r.variant.memory_estimate_mb,
            supports_tools: false,
            ctx_size: Some(ctx),
            ctx_limit: ctx,
            kv_compact: plan.kv_compact,
        })
    }

    /// Creates a decision job and returns it with its request; run it with
    /// [`JobQueue::run_decision`] in [`Self::decider`].
    pub fn create_decision(
        &self,
        state: &str,
        questions: Vec<Question>,
        model: Option<&ModelRef>,
    ) -> Result<(Job, DecisionRequest), ModeError> {
        decisions::validate(state, &questions)
            .map_err(|e| ModeError::new(error_codes::DECISION_INVALID, e.to_string()))?;
        let req = DecisionRequest {
            load: self.decision_load_request(model)?,
            state: state.to_owned(),
            questions,
        };
        let job = self
            .queue
            .create_decision(&req)
            .map_err(|e| ModeError::new("error", e.to_string()))?;
        Ok((job, req))
    }

    // --- local server (ADR 0025) ---------------------------------------------------------

    /// The chat model loaded and reserved for one proxied request. Agents send
    /// long prompts with tool descriptions, so the server loads with the
    /// largest window that fits (or the one set in the settings).
    pub async fn chat_upstream(
        &self,
        model: Option<&ModelRef>,
        cancel: &CancellationToken,
    ) -> Result<Upstream, ModeError> {
        let load = self.server_load_request(model)?;
        self.llm
            .upstream(&load, cancel, &mut |_| {})
            .await
            .map_err(|(code, detail)| ModeError::new(code, detail))
    }

    /// The decision model loaded in its own slot for one proxied `/v1/systemone`.
    pub async fn decision_upstream(
        &self,
        model: Option<&ModelRef>,
        cancel: &CancellationToken,
    ) -> Result<Upstream, ModeError> {
        let load = self.decision_load_request(model)?;
        self.decider
            .upstream(&load, cancel, &mut |_| {})
            .await
            .map_err(|(code, detail)| ModeError::new(code, detail))
    }

    /// Answers `questions` about `state` as a job and waits for it (pipelines).
    pub async fn decide(
        &self,
        state: &str,
        questions: Vec<Question>,
        model: Option<&ModelRef>,
        progress: &mut dyn Progress,
        cancel: &CancellationToken,
    ) -> Result<(Job, Decision), ModeError> {
        let (job, req) = self.create_decision(state, questions, model)?;
        progress.job(job.id);
        let run = Arc::clone(&self.queue).run_decision(job.id, Arc::clone(&self.decider), req);
        let job = self.wait_job(job.id, run, cancel).await?;
        let decision: Decision = serde_json::from_value(job.outputs.clone())
            .map_err(|e| ModeError::new(error_codes::ENGINE_ERROR, e.to_string()))?;
        Ok((job, decision))
    }
}

/// Drops a leading `<think>…</think>` block some models put in the content.
pub fn strip_think(text: &str) -> String {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("<think>")
        && let Some(end) = rest.find("</think>")
    {
        return rest[end + "</think>".len()..].trim().to_owned();
    }
    t.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn think_blocks_are_removed() {
        assert_eq!(strip_think("<think>hmm</think>\n Wynik"), "Wynik");
        assert_eq!(strip_think(" Wynik "), "Wynik");
    }

    #[test]
    fn job_errors_keep_their_code() {
        let mut job = Job {
            id: 1,
            kind: lokalsi_core::jobs::JobKind::Stt,
            status: JobStatus::Failed,
            progress: None,
            model_id: None,
            variant_id: None,
            model_name: String::new(),
            engine: String::new(),
            params: serde_json::Value::Null,
            inputs: serde_json::Value::Null,
            outputs: serde_json::Value::Null,
            text: String::new(),
            error: Some("out_of_memory: CUDA error".into()),
            created_at: 0,
            started_at: None,
            finished_at: None,
            duration_ms: None,
        };
        assert_eq!(ModeError::from_job(&job).code, "out_of_memory");
        job.status = JobStatus::Cancelled;
        assert_eq!(ModeError::from_job(&job).code, "interrupted");
    }
}

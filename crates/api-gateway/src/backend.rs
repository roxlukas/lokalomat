//! [`Backend`] on top of the app's modes: requests from the phone become
//! ordinary jobs (queue, VRAM scheduler, Gallery entries tagged `phone`).
//! Each request runs in its own task with a cancel token, so a phone that
//! disconnects cancels its job instead of leaving it half-done. Jobs started
//! with `/v1/jobs/*` (ADR 0027) run on their own instead; the phone polls them
//! and only the phone that started one can see or cancel it.

use std::path::PathBuf;
use std::sync::Arc;

use lokalsi_core::catalog::{Modality, ModelRef};
use lokalsi_core::jobs::{Job, JobKind, JobStatus, Jobs};
use lokalsi_engines::llama::{ChatMessage, ChatParams};
use lokalsi_engines::modes::{ModeError, ModeRunner, ModelResolver};
use lokalsi_engines::queue::output_files;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::api::{
    DecisionAnswer, DecisionJobRequest, JobFile, JobView, ModelOption, Turn, content_type_of,
    model_id, split_model_id,
};
use crate::server::{ApiError, Backend, BoxFut};

/// Answers "is the computer in Expert mode?": decisions are Expert-only (ADR 0013, 0024).
pub type ExpertFlag = Box<dyn Fn() -> bool + Send + Sync>;

pub struct ModesBackend {
    modes: Arc<ModeRunner>,
    resolver: Arc<dyn ModelResolver>,
    expert: ExpertFlag,
}

impl ModesBackend {
    /// A backend that offers every mode, decisions included (examples, tests).
    pub fn new(modes: Arc<ModeRunner>, resolver: Arc<dyn ModelResolver>) -> Arc<Self> {
        Self::with_expert(modes, resolver, Box::new(|| true))
    }

    /// `expert`: decisions are offered to the phone only while it answers `true`.
    pub fn with_expert(
        modes: Arc<ModeRunner>,
        resolver: Arc<dyn ModelResolver>,
        expert: ExpertFlag,
    ) -> Arc<Self> {
        Arc::new(ModesBackend {
            modes,
            resolver,
            expert,
        })
    }

    /// Modes of the free edition the phone may use, with their model kind.
    fn mode_list(&self) -> Vec<(Modality, &'static str)> {
        let mut list = vec![(Modality::Llm, "chat")];
        if (self.expert)() {
            list.push((Modality::Decision, "decision"));
        }
        list
    }

    /// The stored job when `device` started it (image, song or decision).
    fn own_job(&self, device: &str, job_id: i64) -> Option<Job> {
        let job = Jobs::new(self.modes.db()).get(job_id).ok()??;
        let ours = job.inputs["source"]["device"].as_str() == Some(device)
            && matches!(
                job.kind,
                JobKind::Image | JobKind::Music | JobKind::Decision
            );
        ours.then_some(job)
    }
}

/// Names of the modes of `list` whose model resolves.
pub fn available(resolver: &dyn ModelResolver, list: Vec<(Modality, &'static str)>) -> Vec<String> {
    list.into_iter()
        .filter(|(m, _)| resolver.resolve(*m, None, false).is_ok())
        .map(|(_, name)| name.to_owned())
        .collect()
}

/// Installed models for `mode` of `list`; `None` when `list` has no such mode.
pub fn models_in(
    resolver: &dyn ModelResolver,
    list: Vec<(Modality, &'static str)>,
    mode: &str,
) -> Option<Vec<ModelOption>> {
    let modality = list
        .into_iter()
        .find(|(_, name)| *name == mode)
        .map(|(m, _)| m)?;
    let default = resolver
        .resolve(modality, None, false)
        .ok()
        .map(|r| model_id(&r.model.id, &r.variant.id));
    Some(
        resolver
            .installed(modality)
            .into_iter()
            .map(|r| {
                let id = model_id(&r.model.id, &r.variant.id);
                ModelOption {
                    default: default.as_deref() == Some(id.as_str()),
                    name: r.display_name(),
                    id,
                }
            })
            .collect(),
    )
}

/// A job as the phone sees it: status, then files or answers once done.
pub fn job_view(job: &Job) -> JobView {
    let done = job.status == JobStatus::Done;
    let files = if done {
        gallery_files(job)
            .iter()
            .enumerate()
            .map(|(index, p)| JobFile {
                index,
                content_type: content_type_of(
                    p.extension().and_then(|e| e.to_str()).unwrap_or_default(),
                )
                .to_owned(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let answers: Vec<DecisionAnswer> = if done && job.kind == JobKind::Decision {
        serde_json::from_value(job.outputs["answers"].clone()).unwrap_or_default()
    } else {
        Vec::new()
    };
    JobView {
        job_id: job.id,
        kind: job.kind.as_str().to_owned(),
        status: job.status.as_str().to_owned(),
        progress: job.progress,
        error: job
            .error
            .as_deref()
            .filter(|_| job.status == JobStatus::Failed)
            .map(|e| e.split_once(": ").map_or(e, |(code, _)| code).to_owned()),
        model: job.model_name.clone(),
        files,
        answers,
    }
}

pub fn api_err(e: ModeError) -> ApiError {
    let status = match e.code.as_str() {
        "empty" | "text_too_long" | "audio_unsupported" | "decision_invalid" => 400,
        "no_model"
        | "no_media_model"
        | "model_not_installed"
        | "no_cloning_model"
        | "voice_unsupported"
        | "not_found" => 409,
        "interrupted" => 499,
        _ => 500,
    };
    ApiError::new(status, &e.code, e.detail)
}

/// The phone's choice as a model reference. An unknown id is not an error
/// here: the mode answers `no_model` / `model_not_installed` for it.
pub fn model_ref(id: Option<String>) -> Result<Option<ModelRef>, ApiError> {
    match id.filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(id) => split_model_id(&id)
            .map(|(model_id, variant_id)| {
                Some(ModelRef {
                    model_id,
                    variant_id,
                })
            })
            .ok_or_else(|| ApiError::new(400, "bad_request", "model")),
    }
}

pub fn source() -> serde_json::Value {
    json!({ "phone": true })
}

/// Source of a job a phone started: [`Backend::job`] shows it only to that phone.
pub fn device_source(device: &str) -> serde_json::Value {
    json!({ "phone": true, "device": device })
}

/// Runs `work` in its own task; dropping the returned future cancels it.
pub async fn detached<T: Send + 'static>(
    work: impl FnOnce(CancellationToken) -> BoxFut<'static, Result<T, ModeError>> + Send + 'static,
) -> Result<T, ApiError> {
    let token = CancellationToken::new();
    let _guard = token.clone().drop_guard();
    tokio::spawn(work(token))
        .await
        .map_err(|e| ApiError::new(500, "error", e.to_string()))?
        .map_err(api_err)
}

impl Backend for ModesBackend {
    fn modes(&self) -> Vec<String> {
        available(self.resolver.as_ref(), self.mode_list())
    }

    fn models(&self, mode: &str) -> Vec<ModelOption> {
        models_in(self.resolver.as_ref(), self.mode_list(), mode).unwrap_or_default()
    }

    fn expert(&self) -> bool {
        (self.expert)()
    }

    fn pro(&self) -> bool {
        lokalsi_core::edition::Edition::current().is_pro()
    }

    fn chat(
        &self,
        messages: Vec<Turn>,
        model: Option<String>,
    ) -> BoxFut<'_, Result<(String, String), ApiError>> {
        let modes = Arc::clone(&self.modes);
        let model = match model_ref(model) {
            Ok(m) => m,
            Err(e) => return Box::pin(async { Err(e) }),
        };
        Box::pin(detached(move |cancel| {
            Box::pin(async move {
                let msgs: Vec<ChatMessage> = messages
                    .into_iter()
                    .filter(|t| matches!(t.role.as_str(), "system" | "user" | "assistant"))
                    .map(|t| ChatMessage::text(&t.role, t.content))
                    .collect();
                modes
                    .chat(
                        &msgs,
                        model.as_ref(),
                        &ChatParams::default(),
                        &mut (),
                        &cancel,
                    )
                    .await
            })
        }))
    }

    fn start_decision(&self, device: &str, r: DecisionJobRequest) -> Result<JobView, ApiError> {
        if !(self.expert)() {
            return Err(ApiError::new(403, "expert_only", ""));
        }
        let src = device_source(device);
        let model = model_ref(r.model)?;
        // The wire type has the same JSON as the core type.
        let questions = serde_json::to_value(&r.questions)
            .and_then(serde_json::from_value)
            .map_err(|e| ApiError::new(400, "bad_request", e.to_string()))?;
        let (job, req) = self
            .modes
            .create_decision(r.state.trim(), questions, model.as_ref())
            .map_err(api_err)?;
        if let Err(e) = Jobs::new(self.modes.db()).set_source(job.id, &src) {
            tracing::warn!(job_id = job.id, error = %e, "cannot record the job source");
        }
        let id = job.id;
        let decider = Arc::clone(self.modes.decider());
        let queue = Arc::clone(self.modes.queue());
        tokio::spawn(async move { queue.run_decision(id, decider, req).await });
        Ok(job_view(&job))
    }

    fn job(&self, device: &str, job_id: i64) -> Option<JobView> {
        self.own_job(device, job_id).map(|j| job_view(&j))
    }

    fn cancel_job(&self, device: &str, job_id: i64) -> bool {
        self.own_job(device, job_id).is_some() && self.modes.queue().cancel(job_id)
    }
}

/// Files the phone may download: images, speech and songs (WAV and, when
/// encoding worked, MP3), not the copied recordings.
pub fn gallery_files(job: &Job) -> Vec<PathBuf> {
    output_files(job)
        .into_iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "png" | "wav" | "mp3"))
        })
        .collect()
}

// --- local OpenAI-compatible server (M5c, ADR 0025) ------------------------------------

/// [`crate::openai::ServerBackend`] on top of the app's modes: keys from the
/// database, models from the library, the chat and decision slots of llama.cpp.
pub struct ModesServer {
    modes: Arc<ModeRunner>,
    resolver: Arc<dyn ModelResolver>,
}

/// The models of `modality` as the server lists them.
pub fn served(
    resolver: &dyn ModelResolver,
    modality: Modality,
    slot: crate::openai::Slot,
) -> Vec<crate::openai::ServedModel> {
    let default = resolver
        .resolve(modality, None, false)
        .ok()
        .map(|r| model_id(&r.model.id, &r.variant.id));
    resolver
        .installed(modality)
        .into_iter()
        .map(|r| {
            let id = model_id(&r.model.id, &r.variant.id);
            crate::openai::ServedModel {
                default: default.as_deref() == Some(id.as_str()),
                name: r.display_name(),
                id,
                slot,
            }
        })
        .collect()
}

impl ModesServer {
    pub fn new(modes: Arc<ModeRunner>, resolver: Arc<dyn ModelResolver>) -> Arc<Self> {
        Arc::new(ModesServer { modes, resolver })
    }
}

pub fn server_err(e: ModeError) -> crate::openai::OaError {
    use crate::openai::OaError;
    match e.code.as_str() {
        "no_model" | "no_media_model" | "model_not_installed" => OaError::new(
            404,
            "invalid_request_error",
            "model_not_found",
            "No such model on this computer. See GET /v1/models.",
        ),
        "engine_install_failed"
        | "engine_start_failed"
        | "engine_missing"
        | "engine_missing_library"
        | "engine_crashed"
        | "model_files_missing" => OaError::new(
            503,
            "api_error",
            &e.code,
            "The model is not ready yet. Try again in a moment.",
        ),
        "interrupted" => OaError::new(499, "api_error", "cancelled", "Cancelled."),
        "decision_invalid" => OaError::new(
            400,
            "invalid_request_error",
            "invalid_questions",
            format!("The questions are not valid: {}.", e.detail),
        ),
        "empty" | "text_too_long" | "voice_unsupported" => OaError::new(
            400,
            "invalid_request_error",
            &e.code,
            "The text is empty or too long, or the model cannot do that.",
        ),
        _ => OaError::new(500, "api_error", &e.code, "The engine reported an error."),
    }
}

impl crate::openai::ServerBackend for ModesServer {
    fn verify_key(&self, token: &str) -> Option<crate::openai::Caller> {
        let keys = lokalsi_core::api_keys::ApiKeys::new(self.modes.db());
        match keys.verify(token) {
            Ok(Some(key)) => {
                if let Err(e) = keys.touch(&key.id) {
                    tracing::warn!(error = %e, "cannot count a server request");
                }
                Some(crate::openai::Caller {
                    key_id: key.id,
                    name: key.name,
                })
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(error = %e, "cannot check a server key");
                None
            }
        }
    }

    fn models(&self) -> Vec<crate::openai::ServedModel> {
        let r = self.resolver.as_ref();
        let mut all = served(r, Modality::Llm, crate::openai::Slot::Chat);
        all.extend(served(r, Modality::Decision, crate::openai::Slot::Decision));
        all
    }

    fn upstream(
        &self,
        slot: crate::openai::Slot,
        model: Option<String>,
        cancel: CancellationToken,
    ) -> BoxFut<'_, Result<crate::openai::UpstreamHandle, crate::openai::OaError>> {
        Box::pin(async move {
            let model = match model {
                None => None,
                Some(id) => {
                    let known = self
                        .models()
                        .into_iter()
                        .any(|m| m.id == id && m.slot == slot);
                    let parsed = split_model_id(&id);
                    match parsed {
                        Some((model_id, variant_id)) if known => Some(ModelRef {
                            model_id,
                            variant_id,
                        }),
                        _ => {
                            return Err(crate::openai::OaError::new(
                                404,
                                "invalid_request_error",
                                "model_not_found",
                                format!("No model \"{id}\" here. See GET /v1/models."),
                            ));
                        }
                    }
                }
            };
            let up = match slot {
                crate::openai::Slot::Chat => {
                    self.modes.chat_upstream(model.as_ref(), &cancel).await
                }
                crate::openai::Slot::Decision => {
                    self.modes.decision_upstream(model.as_ref(), &cancel).await
                }
                // Media models are not behind llama-server; they have their own endpoints.
                _ => {
                    return Err(crate::openai::OaError::new(
                        400,
                        "invalid_request_error",
                        "not_a_chat_model",
                        "This model is not used through chat.",
                    ));
                }
            }
            .map_err(server_err)?;
            Ok(crate::openai::UpstreamHandle {
                base_url: up.base_url.clone(),
                token: up.token.clone(),
                model_name: up.model_name.clone(),
                hold: Box::new(up),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use lokalsi_core::db::Db;
    use lokalsi_core::jobs::NewJob;
    use lokalsi_engines::manifest::EngineManifest;
    use lokalsi_engines::modes::Resolved;
    use lokalsi_engines::queue::{JobQueue, MediaEvents};
    use lokalsi_engines::scheduler::Scheduler;
    use lokalsi_engines::service::{EngineEvents, EngineService};

    use super::*;
    use crate::api::{DecisionKind, DecisionQuestion};

    struct NoEvents;
    impl EngineEvents for NoEvents {}
    impl MediaEvents for NoEvents {}

    /// Nothing is installed.
    struct Empty;
    impl ModelResolver for Empty {
        fn resolve(
            &self,
            _: Modality,
            _: Option<&ModelRef>,
            _: bool,
        ) -> Result<Resolved, ModeError> {
            Err(ModeError::new("no_media_model", "none"))
        }
    }

    /// Chat and decisions on a job queue; no media engines (free edition).
    fn runner(dir: &Path) -> Arc<ModeRunner> {
        let db = Arc::new(Db::open(&dir.join("app.db")).unwrap());
        let queue = JobQueue::new(
            db.clone(),
            Scheduler::new(),
            Arc::new(NoEvents),
            dir.to_path_buf(),
            dir.join("logs"),
        );
        let llm = EngineService::new(
            EngineManifest::llama_cpp(),
            db.clone(),
            reqwest::Client::new(),
            "windows-x64".into(),
            dir.join("engines"),
            &dir.join("logs"),
            Arc::new(NoEvents),
        );
        ModeRunner::new(db, queue, llm, Arc::new(Empty))
    }

    fn backend(dir: &Path, expert: bool) -> Arc<ModesBackend> {
        ModesBackend::with_expert(runner(dir), Arc::new(Empty), Box::new(move || expert))
    }

    fn spam_question() -> DecisionJobRequest {
        DecisionJobRequest {
            state: "Kup tanie zegarki!".into(),
            questions: vec![DecisionQuestion {
                id: "q1".into(),
                instructions: "Czy to spam?".into(),
                kind: DecisionKind::YesNo,
            }],
            model: None,
        }
    }

    #[tokio::test]
    async fn decisions_are_expert_only() {
        let dir = tempfile::tempdir().unwrap();
        let basic = backend(dir.path(), false);
        assert!(Backend::models(&*basic, "decision").is_empty());
        let e = basic.start_decision("phone", spam_question()).unwrap_err();
        assert_eq!((e.status, e.code.as_str()), (403, "expert_only"));

        let expert = backend(dir.path(), true);
        // Past the gate the usual checks answer: no model here.
        let e = expert.start_decision("phone", spam_question()).unwrap_err();
        assert_eq!(e.code, "no_media_model");
        let mut bad = spam_question();
        bad.questions[0].kind = DecisionKind::Choice {
            options: vec!["tylko jedna".into()],
        };
        let e = expert.start_decision("phone", bad).unwrap_err();
        assert_eq!((e.status, e.code.as_str()), (400, "decision_invalid"));
    }

    #[tokio::test]
    async fn a_phone_sees_only_its_own_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(dir.path(), true);
        let jobs = Jobs::new(b.modes.db());
        let new = NewJob {
            model_name: "ACE-Step",
            inputs: json!({}),
            ..Default::default()
        };
        let song = jobs.create(JobKind::Music, &new).unwrap();
        jobs.set_source(song.id, &device_source("a")).unwrap();
        let stt = jobs.create(JobKind::Stt, &new).unwrap();
        jobs.set_source(stt.id, &device_source("a")).unwrap();

        let v = b.job("a", song.id).unwrap();
        assert_eq!((v.kind.as_str(), v.status.as_str()), ("music", "queued"));
        assert!(v.files.is_empty());
        assert!(b.job("b", song.id).is_none(), "another phone");
        assert!(b.job("a", stt.id).is_none(), "not a job kind of the phone");
        assert!(b.job("a", 999).is_none());
        assert!(!b.cancel_job("b", song.id));

        // Done: the WAV and the MP3, with their types.
        let out = b.modes.queue().outputs_root();
        let files = [out.join("music-1.wav"), out.join("music-1.mp3")];
        jobs.save_outputs(song.id, &json!({ "files": files }), None, None)
            .unwrap();
        jobs.set_status(song.id, JobStatus::Done, None).unwrap();
        let v = b.job("a", song.id).unwrap();
        let types: Vec<_> = v.files.iter().map(|f| f.content_type.as_str()).collect();
        assert_eq!(types, ["audio/wav", "audio/mpeg"]);
        assert_eq!(v.progress, Some(1.0));

        // Failed: only the error code reaches the phone, not paths.
        let img = jobs.create(JobKind::Image, &new).unwrap();
        jobs.set_source(img.id, &device_source("a")).unwrap();
        jobs.set_status(
            img.id,
            JobStatus::Failed,
            Some("out_of_memory: CUDA at C:/x"),
        )
        .unwrap();
        assert_eq!(
            b.job("a", img.id).unwrap().error.as_deref(),
            Some("out_of_memory")
        );
    }

    #[tokio::test]
    async fn decision_answers_reach_the_phone() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(dir.path(), true);
        let jobs = Jobs::new(b.modes.db());
        let job = jobs
            .create(
                JobKind::Decision,
                &NewJob {
                    model_name: "Basal",
                    inputs: json!({}),
                    ..Default::default()
                },
            )
            .unwrap();
        jobs.set_source(job.id, &device_source("a")).unwrap();
        let outputs = json!({ "answers": [{
            "id": "q1", "label": "yes", "confidence": 0.8,
            "probabilities": [{ "label": "yes", "p": 0.9 }, { "label": "no", "p": 0.1 }]
        }], "input_tokens": 12 });
        jobs.save_outputs(job.id, &outputs, None, None).unwrap();
        jobs.set_status(job.id, JobStatus::Done, None).unwrap();
        let v = b.job("a", job.id).unwrap();
        assert_eq!(v.answers.len(), 1);
        assert_eq!(v.answers[0].label, "yes");
        assert_eq!(v.answers[0].probabilities[0].p, 0.9);
        assert!(v.finished());
    }
}

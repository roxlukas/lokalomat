//! The generic job queue shared by every kind of job (ADR 0035): bookkeeping in
//! the `jobs` table, cancelling, progress events, deleting results, and
//! decision jobs. Image, speech, transcription and song jobs (`media.rs`) are
//! built on it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lokalsi_core::db::Db;
use lokalsi_core::jobs::{Job, JobKind, JobStatus, Jobs, NewJob};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::cli::CliError;
use crate::scheduler::Scheduler;
use crate::service::{EngineError, EngineService, error_codes, job_errors};

pub trait MediaEvents: Send + Sync + 'static {
    fn job_updated(&self, _job: &Job) {}
    fn job_progress(&self, _job_id: i64, _progress: f64) {}
}

/// A typed decision (ADR 0024) run in the decision slot of llama.cpp.
#[derive(Debug, Clone)]
pub struct DecisionRequest {
    pub load: crate::service::LoadRequest,
    pub state: String,
    pub questions: Vec<lokalsi_core::decisions::Question>,
}

/// Result of running one job: an error code (`job_errors`) and a detail.
pub type JobResult = Result<(), (&'static str, String)>;

pub struct JobQueue {
    pub db: Arc<Db>,
    pub scheduler: Arc<Scheduler>,
    pub events: Arc<dyn MediaEvents>,
    pub logs_dir: PathBuf,
    data_dir: Mutex<PathBuf>,
    jobs: Mutex<HashMap<i64, CancellationToken>>,
}

impl JobQueue {
    pub fn new(
        db: Arc<Db>,
        scheduler: Arc<Scheduler>,
        events: Arc<dyn MediaEvents>,
        data_dir: PathBuf,
        logs_dir: PathBuf,
    ) -> Arc<Self> {
        match Jobs::new(&db).fail_unfinished(error_codes::INTERRUPTED) {
            Ok(n) if n > 0 => tracing::info!(
                count = n,
                "jobs interrupted by the previous run marked as failed"
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "cannot clean up unfinished jobs"),
        }
        Arc::new(JobQueue {
            db,
            scheduler,
            events,
            logs_dir,
            data_dir: Mutex::new(data_dir),
            jobs: Mutex::new(HashMap::new()),
        })
    }

    pub fn set_data_dir(&self, dir: PathBuf) {
        *self.data_dir.lock().unwrap_or_else(|p| p.into_inner()) = dir;
    }

    pub fn data_dir(&self) -> PathBuf {
        self.data_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// `<data_dir>/outputs`: everything the app writes as results.
    pub fn outputs_root(&self) -> PathBuf {
        self.data_dir().join("outputs")
    }

    /// `<data_dir>/outputs/YYYY-MM` (PRD §4.5).
    pub fn outputs_dir(&self) -> std::io::Result<PathBuf> {
        let dir = self
            .outputs_root()
            .join(year_month(lokalsi_core::db::now_secs()));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    // --- job bookkeeping ------------------------------------------------------------

    pub fn update(&self, id: i64, status: JobStatus, error: Option<&str>) {
        let jobs = Jobs::new(&self.db);
        if let Err(e) = jobs.set_status(id, status, error) {
            tracing::warn!(job_id = id, error = %e, "cannot save job status");
        }
        if let Ok(Some(job)) = jobs.get(id) {
            self.events.job_updated(&job);
        }
    }

    pub fn cancel(&self, job_id: i64) -> bool {
        match self
            .jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&job_id)
        {
            Some(token) => {
                tracing::info!(job_id, "job cancelled by the user");
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Deletes a finished job and the files it produced (only inside `outputs`).
    pub fn delete(&self, job_id: i64) -> Result<bool, lokalsi_core::db::DbError> {
        self.cancel(job_id);
        let Some(job) = Jobs::new(&self.db).delete(job_id)? else {
            return Ok(false);
        };
        let root = self.outputs_root();
        for path in output_files(&job) {
            if path.starts_with(&root) {
                let _ = std::fs::remove_file(&path);
            }
        }
        tracing::info!(job_id, kind = job.kind.as_str(), "job deleted");
        Ok(true)
    }

    pub fn progress_reporter(&self, job_id: i64) -> impl FnMut(f64) + Send + '_ {
        let mut last = Instant::now() - Duration::from_secs(1);
        let mut last_value = -1.0f64;
        move |p: f64| {
            if (p - last_value).abs() >= 0.01 && last.elapsed() >= Duration::from_millis(200)
                || p >= 1.0
            {
                last = Instant::now();
                last_value = p;
                let _ = Jobs::new(&self.db).set_progress(job_id, Some(p));
                self.events.job_progress(job_id, p);
            }
        }
    }

    // --- decisions (ADR 0024) -------------------------------------------------------------

    /// Stores a decision job: `params.questions`, the text to judge as the
    /// searchable text; the answers come in `outputs` when it is done.
    pub fn create_decision(&self, req: &DecisionRequest) -> Result<Job, lokalsi_core::db::DbError> {
        let job = Jobs::new(&self.db).create(
            JobKind::Decision,
            &NewJob {
                model_id: Some(&req.load.model_id),
                variant_id: Some(&req.load.variant_id),
                model_name: &req.load.name,
                engine: "llama-cpp",
                params: json!({ "questions": req.questions }),
                inputs: json!({ "chars": req.state.chars().count() }),
                text: &req.state,
            },
        )?;
        tracing::info!(job_id = job.id, model = %req.load.model_id, questions = req.questions.len(), chars = req.state.chars().count(), "decision job created");
        Ok(job)
    }

    /// Runs a decision job in `decider` (the second llama.cpp slot).
    pub async fn run_decision(
        self: Arc<Self>,
        job_id: i64,
        decider: Arc<EngineService>,
        req: DecisionRequest,
    ) {
        let cancel = self.register(job_id);
        let started = Instant::now();
        let result = async {
            let decision = decider
                .decide(&req.load, &req.state, &req.questions, &cancel, &mut |s| {
                    self.update(job_id, s, None)
                })
                .await?;
            let outputs = serde_json::to_value(&decision)
                .map_err(|e| (job_errors::ENGINE_ERROR, e.to_string()))?;
            Jobs::new(&self.db)
                .save_outputs(job_id, &outputs, None, None)
                .map_err(|e| (job_errors::ENGINE_ERROR, e.to_string()))
        }
        .await;
        self.finish(job_id, result, &cancel, started);
    }

    // --- shared steps ------------------------------------------------------------------

    pub fn register(&self, job_id: i64) -> CancellationToken {
        let token = CancellationToken::new();
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(job_id, token.clone());
        token
    }

    pub fn finish(
        &self,
        job_id: i64,
        result: JobResult,
        cancel: &CancellationToken,
        started: Instant,
    ) {
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&job_id);
        let elapsed = started.elapsed().as_millis() as i64;
        let jobs = Jobs::new(&self.db);
        if let Ok(Some(job)) = jobs.get(job_id) {
            let _ = jobs.save_outputs(job_id, &job.outputs, None, Some(elapsed));
        }
        match result {
            Ok(()) => {
                tracing::info!(job_id, elapsed_ms = elapsed, "job finished");
                self.update(job_id, JobStatus::Done, None);
            }
            Err(_) if cancel.is_cancelled() => {
                self.update(job_id, JobStatus::Cancelled, None);
            }
            Err((code, detail)) => {
                tracing::error!(job_id, code, detail = %detail, "job failed");
                self.update(
                    job_id,
                    JobStatus::Failed,
                    Some(&format!("{code}: {detail}")),
                );
            }
        }
    }

    /// Plug and play: installs the engine on first use.
    pub async fn ensure_engine(
        &self,
        job_id: i64,
        engine: &Arc<EngineService>,
        cancel: &CancellationToken,
    ) -> JobResult {
        if engine.installed().ok().flatten().is_some() {
            return Ok(());
        }
        self.update(job_id, JobStatus::Preparing, None);
        let installed = tokio::select! {
            r = engine.ensure_installed() => r,
            _ = cancel.cancelled() => return Err((error_codes::INTERRUPTED, "cancelled".into())),
        };
        match installed {
            Ok(()) => Ok(()),
            Err(EngineError::NoBuild(name)) => Err((job_errors::ENGINE_UNAVAILABLE, name)),
            Err(e) => Err((job_errors::ENGINE_INSTALL, e.to_string())),
        }
    }
}

pub async fn wait_turn<'a>(
    lock: &'a tokio::sync::Mutex<()>,
    cancel: &CancellationToken,
) -> Result<tokio::sync::MutexGuard<'a, ()>, (&'static str, String)> {
    tokio::select! {
        g = lock.lock() => Ok(g),
        _ = cancel.cancelled() => Err((error_codes::INTERRUPTED, "cancelled".into())),
    }
}

pub fn io_err(e: std::io::Error) -> (&'static str, String) {
    (job_errors::ENGINE_ERROR, e.to_string())
}

pub fn cli_err(e: CliError) -> (&'static str, String) {
    match e {
        CliError::Spawn(e) => (job_errors::ENGINE_START, e.to_string()),
        CliError::Cancelled => (error_codes::INTERRUPTED, "cancelled".into()),
        // Detail = the package to install (shown in the message, ADR 0017).
        CliError::MissingLibrary(m) => (job_errors::MISSING_LIBRARY, m.package),
        CliError::Exited { code, detail } => {
            let lower = detail.to_lowercase();
            if lower.contains("out of memory")
                || lower.contains("cudamalloc")
                || lower.contains("failed to allocate")
            {
                (job_errors::OUT_OF_MEMORY, detail)
            } else {
                (
                    job_errors::ENGINE_CRASHED,
                    format!("exit {code:?}: {detail}"),
                )
            }
        }
    }
}

/// Files a job produced (images, the copied recording).
pub fn output_files(job: &Job) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = job.outputs["files"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default();
    if let Some(audio) = job.outputs["audio"].as_str() {
        out.push(PathBuf::from(audio));
    }
    out
}

/// `YYYY-MM` (UTC) for a unix time.
pub fn year_month(secs: i64) -> String {
    // Civil-from-days (Howard Hinnant), enough for a folder name.
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}")
}

/// True when `path` lies inside `root` (after resolving `..`).
pub fn is_inside(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(p), Ok(r)) => p.starts_with(r),
        _ => false,
    }
}

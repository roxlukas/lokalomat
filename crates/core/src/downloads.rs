//! Download manager (PRD §7): FIFO queue with reordering, pause, resume and
//! cancel; HTTP Range resume into `*.part` files; sha256 verification with one
//! automatic retry; free-space check (size + 10%); reuse of files already in
//! the Hugging Face cache; optional bandwidth limit.
//!
//! State lives in SQLite, so an interrupted download (crash, kill, power
//! loss) is re-queued on the next start and resumes from the `.part` file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::db::{Db, DbError, FileSource, InstalledFile, now_secs};
use crate::hf;

/// Automatic attempts after a checksum mismatch (PRD: retry once, then error).
const MAX_CHECKSUM_ATTEMPTS: i64 = 2;
/// Network failures tolerated in a row before the item is marked failed.
const MAX_NETWORK_RETRIES: u32 = 3;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const PERSIST_INTERVAL: Duration = Duration::from_secs(1);
const WRITE_BUFFER: usize = 256 * 1024;

/// Error keys shown to the user through i18n (`downloads.errors.*`).
pub mod error_keys {
    pub const CHECKSUM: &str = "checksum_mismatch";
    pub const AUTH: &str = "auth_required";
    pub const NOT_FOUND: &str = "not_found";
    pub const NETWORK: &str = "network";
    pub const DISK: &str = "disk";
    pub const HTTP: &str = "http";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Verifying,
    Paused,
    Completed,
    Failed,
}

impl DownloadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DownloadStatus::Queued => "queued",
            DownloadStatus::Downloading => "downloading",
            DownloadStatus::Verifying => "verifying",
            DownloadStatus::Paused => "paused",
            DownloadStatus::Completed => "completed",
            DownloadStatus::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "downloading" => DownloadStatus::Downloading,
            "verifying" => DownloadStatus::Verifying,
            "paused" => DownloadStatus::Paused,
            "completed" => DownloadStatus::Completed,
            "failed" => DownloadStatus::Failed,
            _ => DownloadStatus::Queued,
        }
    }

    pub fn is_pending(self) -> bool {
        !matches!(self, DownloadStatus::Completed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadItem {
    pub id: i64,
    pub model_id: String,
    pub variant_id: String,
    pub repo: String,
    pub file_path: String,
    pub url: String,
    pub dest: PathBuf,
    pub size: u64,
    pub sha256: Option<String>,
    pub gated: bool,
    pub downloaded: u64,
    pub status: DownloadStatus,
    /// One of [`error_keys`], optionally followed by `: details`.
    pub error: Option<String>,
    pub attempts: i64,
    pub position: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub id: i64,
    pub model_id: String,
    pub variant_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub bytes_per_sec: u64,
    pub status: DownloadStatus,
}

/// Receives state changes, e.g. to forward them to the UI as Tauri events.
pub trait DownloadEvents: Send + Sync + 'static {
    fn progress(&self, _progress: &Progress) {}
    /// The queue changed (status, order, items added or removed).
    fn changed(&self) {}
    fn variant_installed(&self, _model_id: &str, _variant_id: &str) {}
}

pub struct NoEvents;
impl DownloadEvents for NoEvents {}

#[derive(Debug, Clone, Default)]
pub struct DownloadConfig {
    /// KiB/s; `None` = unlimited.
    pub bandwidth_limit_kbps: Option<u32>,
    pub hf_token: Option<String>,
    /// Hugging Face cache to reuse files from (`None` disables the lookup).
    pub hf_cache: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedFile {
    pub repo: String,
    pub path: String,
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub gated: bool,
    /// Path inside the variant folder (`ModelFile::local_path`).
    #[serde(default)]
    pub local: String,
}

impl PlannedFile {
    /// Where the file is stored; an empty `local` falls back to the file name.
    pub fn dest_in(&self, dir: &Path) -> PathBuf {
        let rel = if self.local.is_empty() {
            file_name(&self.path)
        } else {
            self.local.as_str()
        };
        rel.split('/').fold(dir.to_path_buf(), |p, seg| p.join(seg))
    }

    /// Stored in a sub-folder (engines that need a folder tree).
    fn nested(&self) -> bool {
        self.local.contains('/')
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantPlan {
    pub model_id: String,
    pub variant_id: String,
    pub files: Vec<PlannedFile>,
    /// `<data_dir>/models/<model>/<variant>`.
    pub dest_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnqueueOutcome {
    Queued {
        files: usize,
        bytes: u64,
        reused: usize,
    },
    AlreadyInstalled {
        reused: usize,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("not enough free space: {needed} bytes needed, {available} available")]
    InsufficientSpace { needed: u64, available: u64 },
    #[error("download {0} not found")]
    NotFound(i64),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<rusqlite::Error> for DownloadError {
    fn from(e: rusqlite::Error) -> Self {
        DownloadError::Db(DbError::Sql(e))
    }
}

/// PRD §7: free space must cover the remaining bytes plus 10%.
pub fn check_space(needed: u64, available: u64) -> Result<(), DownloadError> {
    let required = needed + needed / 10;
    if required > available {
        Err(DownloadError::InsufficientSpace {
            needed: required,
            available,
        })
    } else {
        Ok(())
    }
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    Pause,
    Cancel,
    Shutdown,
}

struct Active {
    id: i64,
    token: CancellationToken,
    reason: Arc<Mutex<Option<StopReason>>>,
}

struct Inner {
    db: Arc<Db>,
    client: reqwest::Client,
    events: Arc<dyn DownloadEvents>,
    config: RwLock<DownloadConfig>,
    wake: Notify,
    shutdown: AtomicBool,
    active: Mutex<Option<Active>>,
}

#[derive(Clone)]
pub struct DownloadManager {
    inner: Arc<Inner>,
}

enum TransferError {
    Stopped,
    Retryable(String),
    Fatal(String),
}

const SELECT_COLUMNS: &str = "id, model_id, variant_id, repo, file_path, url, dest, size, sha256, gated, downloaded, status, error, attempts, position";

fn row_to_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<DownloadItem> {
    Ok(DownloadItem {
        id: r.get(0)?,
        model_id: r.get(1)?,
        variant_id: r.get(2)?,
        repo: r.get(3)?,
        file_path: r.get(4)?,
        url: r.get(5)?,
        dest: PathBuf::from(r.get::<_, String>(6)?),
        size: r.get::<_, i64>(7)? as u64,
        sha256: r.get(8)?,
        gated: r.get::<_, i64>(9)? != 0,
        downloaded: r.get::<_, i64>(10)? as u64,
        status: DownloadStatus::parse(&r.get::<_, String>(11)?),
        error: r.get(12)?,
        attempts: r.get(13)?,
        position: r.get(14)?,
    })
}

impl DownloadManager {
    /// Creates the manager and re-queues downloads interrupted by a previous
    /// run. Call [`run`](Self::run) on an async runtime to start the worker.
    pub fn new(
        db: Arc<Db>,
        events: Arc<dyn DownloadEvents>,
        config: DownloadConfig,
    ) -> Result<Self, DownloadError> {
        let recovered = db.conn().execute(
            "UPDATE downloads SET status = 'queued', updated_at = ?1
             WHERE status IN ('downloading', 'verifying')",
            params![now_secs()],
        )?;
        if recovered > 0 {
            tracing::info!(
                count = recovered,
                "resuming downloads interrupted by the previous run"
            );
        }
        Ok(DownloadManager {
            inner: Arc::new(Inner {
                db,
                client: hf::client(),
                events,
                config: RwLock::new(config),
                wake: Notify::new(),
                shutdown: AtomicBool::new(false),
                active: Mutex::new(None),
            }),
        })
    }

    pub fn set_config(&self, config: DownloadConfig) {
        tracing::debug!(
            bandwidth_limit_kbps = ?config.bandwidth_limit_kbps,
            hf_token = config.hf_token.is_some(),
            "download configuration updated"
        );
        *self.inner.config.write().unwrap_or_else(|p| p.into_inner()) = config;
    }

    fn config(&self) -> DownloadConfig {
        self.inner
            .config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    // --- queue API ------------------------------------------------------------

    /// Queues every file of a variant that is not installed yet. Files found
    /// in the Hugging Face cache are registered without downloading.
    pub fn enqueue_variant(&self, plan: &VariantPlan) -> Result<EnqueueOutcome, DownloadError> {
        let installed = self
            .inner
            .db
            .installed_files_for(&plan.model_id, &plan.variant_id)?;
        let cache = self.config().hf_cache;
        let paths: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
        self.inner
            .db
            .set_variant_files(&plan.model_id, &plan.variant_id, &paths)?;
        let mut reused = 0usize;
        let mut to_download: Vec<&PlannedFile> = Vec::new();

        for file in &plan.files {
            if installed.iter().any(|f| f.file_path == file.path) {
                continue;
            }
            // A folder tree must stay together, so the HF cache is not used for it.
            let cached = match (&cache, &file.sha256) {
                (Some(dir), Some(sha)) if !file.nested() => {
                    hf::find_in_cache(dir, &file.repo, sha, file.size)
                }
                _ => None,
            };
            if let Some(path) = cached {
                tracing::info!(model = %plan.model_id, file = %file.path, path = %path.display(), "reusing file from the Hugging Face cache");
                self.inner.db.insert_installed_file(&InstalledFile {
                    model_id: plan.model_id.clone(),
                    variant_id: plan.variant_id.clone(),
                    file_path: file.path.clone(),
                    repo: file.repo.clone(),
                    local_path: path.to_string_lossy().into_owned(),
                    size: file.size,
                    sha256: file.sha256.clone(),
                    source: FileSource::HfCache,
                })?;
                reused += 1;
            } else {
                to_download.push(file);
            }
        }

        if to_download.is_empty() {
            self.finish_variant_if_complete(&plan.model_id, &plan.variant_id)?;
            self.inner.events.changed();
            return Ok(EnqueueOutcome::AlreadyInstalled { reused });
        }

        let remaining: u64 = to_download
            .iter()
            .map(|f| {
                let dest = f.dest_in(&plan.dest_dir);
                let have = std::fs::metadata(part_path(&dest))
                    .map(|m| m.len())
                    .unwrap_or(0);
                f.size.saturating_sub(have)
            })
            .sum();
        std::fs::create_dir_all(&plan.dest_dir)?;
        if let Some(available) = lokalsi_hwprobe::available_space(&plan.dest_dir) {
            check_space(remaining, available)?;
        }

        let mut queued = 0usize;
        {
            let conn = self.inner.db.conn();
            let now = now_secs();
            for f in &to_download {
                let dest = f.dest_in(&plan.dest_dir);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let existing: Option<i64> = conn
                    .query_row(
                        "SELECT id FROM downloads WHERE model_id = ?1 AND variant_id = ?2 AND file_path = ?3 AND status != 'completed'",
                        params![plan.model_id, plan.variant_id, f.path],
                        |r| r.get(0),
                    )
                    .optional()?;
                let position: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(position), 0) + 1 FROM downloads",
                    [],
                    |r| r.get(0),
                )?;
                match existing {
                    Some(id) => {
                        conn.execute(
                            "UPDATE downloads SET status = 'queued', error = NULL, attempts = 0, updated_at = ?2
                             WHERE id = ?1 AND status IN ('paused', 'failed')",
                            params![id, now],
                        )?;
                    }
                    None => {
                        conn.execute(
                            "INSERT INTO downloads (model_id, variant_id, repo, file_path, revision, url, dest, size, sha256, gated,
                                downloaded, status, attempts, position, created_at, updated_at)
                             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, 0, 'queued', 0, ?10, ?11, ?11)",
                            params![
                                plan.model_id,
                                plan.variant_id,
                                f.repo,
                                f.path,
                                f.url,
                                dest.to_string_lossy(),
                                f.size as i64,
                                f.sha256,
                                f.gated as i64,
                                position,
                                now
                            ],
                        )?;
                    }
                }
                queued += 1;
            }
        }
        tracing::info!(model = %plan.model_id, variant = %plan.variant_id, files = queued, bytes = remaining, reused, "variant queued for download");
        self.inner.events.changed();
        self.inner.wake.notify_one();
        Ok(EnqueueOutcome::Queued {
            files: queued,
            bytes: remaining,
            reused,
        })
    }

    pub fn list(&self) -> Result<Vec<DownloadItem>, DownloadError> {
        let conn = self.inner.db.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM downloads ORDER BY status = 'completed', position, id"
        ))?;
        let items = stmt
            .query_map([], row_to_item)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn get(&self, id: i64) -> Result<DownloadItem, DownloadError> {
        self.inner
            .db
            .conn()
            .query_row(
                &format!("SELECT {SELECT_COLUMNS} FROM downloads WHERE id = ?1"),
                params![id],
                row_to_item,
            )
            .optional()?
            .ok_or(DownloadError::NotFound(id))
    }

    pub fn pause(&self, id: i64) -> Result<(), DownloadError> {
        let item = self.get(id)?;
        tracing::info!(id, file = %item.file_path, "pause download");
        if !self.stop_active(id, StopReason::Pause) && item.status == DownloadStatus::Queued {
            self.set_status(id, DownloadStatus::Paused, None)?;
        }
        Ok(())
    }

    /// Paused or failed → queued (a failed item gets a fresh attempt counter).
    pub fn resume(&self, id: i64) -> Result<(), DownloadError> {
        let item = self.get(id)?;
        if matches!(item.status, DownloadStatus::Paused | DownloadStatus::Failed) {
            tracing::info!(id, file = %item.file_path, "resume download");
            self.inner.db.conn().execute(
                "UPDATE downloads SET status = 'queued', error = NULL, attempts = 0, updated_at = ?2 WHERE id = ?1",
                params![id, now_secs()],
            )?;
            self.inner.events.changed();
            self.inner.wake.notify_one();
        }
        Ok(())
    }

    /// Stops and removes the item together with its `.part` file.
    pub fn cancel(&self, id: i64) -> Result<(), DownloadError> {
        let item = self.get(id)?;
        tracing::info!(id, file = %item.file_path, "cancel download");
        if !self.stop_active(id, StopReason::Cancel) {
            self.remove_item(&item)?;
        }
        Ok(())
    }

    /// Cancels every pending file of a variant (used when deleting a model).
    pub fn cancel_variant(&self, model_id: &str, variant_id: &str) -> Result<(), DownloadError> {
        for item in self.list()? {
            if item.model_id == model_id
                && item.variant_id == variant_id
                && item.status.is_pending()
            {
                self.cancel(item.id)?;
            }
        }
        Ok(())
    }

    /// Moves a pending item one place up or down in the queue.
    pub fn move_item(&self, id: i64, up: bool) -> Result<(), DownloadError> {
        let item = self.get(id)?;
        let conn = self.inner.db.conn();
        let neighbour: Option<(i64, i64)> = conn
            .query_row(
                if up {
                    "SELECT id, position FROM downloads WHERE status != 'completed' AND position < ?1 ORDER BY position DESC LIMIT 1"
                } else {
                    "SELECT id, position FROM downloads WHERE status != 'completed' AND position > ?1 ORDER BY position ASC LIMIT 1"
                },
                params![item.position],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((other_id, other_pos)) = neighbour {
            conn.execute(
                "UPDATE downloads SET position = ?2 WHERE id = ?1",
                params![id, other_pos],
            )?;
            conn.execute(
                "UPDATE downloads SET position = ?2 WHERE id = ?1",
                params![other_id, item.position],
            )?;
            drop(conn);
            tracing::debug!(id, up, "download moved in the queue");
            self.inner.events.changed();
        }
        Ok(())
    }

    /// Removes completed items from the list.
    pub fn clear_finished(&self) -> Result<usize, DownloadError> {
        let n = self
            .inner
            .db
            .conn()
            .execute("DELETE FROM downloads WHERE status = 'completed'", [])?;
        self.inner.events.changed();
        Ok(n)
    }

    /// Stops the worker after the current chunk; `.part` files are kept.
    pub fn shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        self.stop_active_any(StopReason::Shutdown);
        self.inner.wake.notify_waiters();
        self.inner.wake.notify_one();
    }

    // --- worker -------------------------------------------------------------

    /// Worker loop: downloads queued items one at a time until [`shutdown`](Self::shutdown).
    pub async fn run(self) {
        tracing::debug!("download worker started");
        loop {
            if self.inner.shutdown.load(Ordering::SeqCst) {
                break;
            }
            let notified = self.inner.wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.next_queued() {
                Ok(Some(item)) => self.process(item).await,
                Ok(None) => notified.await,
                Err(e) => {
                    tracing::error!(error = %e, "download queue unavailable");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
        tracing::debug!("download worker stopped");
    }

    fn next_queued(&self) -> Result<Option<DownloadItem>, DownloadError> {
        Ok(self
            .inner
            .db
            .conn()
            .query_row(
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM downloads WHERE status = 'queued' ORDER BY position, id LIMIT 1"
                ),
                [],
                row_to_item,
            )
            .optional()?)
    }

    async fn process(&self, item: DownloadItem) {
        let token = CancellationToken::new();
        let reason = Arc::new(Mutex::new(None));
        *self.inner.active.lock().unwrap_or_else(|p| p.into_inner()) = Some(Active {
            id: item.id,
            token: token.clone(),
            reason: reason.clone(),
        });
        let _ = self.set_status(item.id, DownloadStatus::Downloading, None);
        tracing::info!(id = item.id, model = %item.model_id, file = %item.file_path, size = item.size, "download started");
        tracing::debug!(id = item.id, url = %item.url, dest = %item.dest.display(), "download details");

        let mut network_failures = 0u32;
        let result = loop {
            match self.transfer(&item, &token).await {
                Err(TransferError::Retryable(msg))
                    if network_failures + 1 < MAX_NETWORK_RETRIES =>
                {
                    network_failures += 1;
                    let wait = Duration::from_secs(2u64.pow(network_failures));
                    tracing::warn!(id = item.id, error = %msg, retry_in_s = wait.as_secs(), "download interrupted, retrying");
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = token.cancelled() => break Err(TransferError::Stopped),
                    }
                }
                other => break other,
            }
        };

        let stop = *reason.lock().unwrap_or_else(|p| p.into_inner());
        self.inner
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();

        let outcome = match result {
            Ok(hash) => self.finish(&item, &hash),
            Err(TransferError::Stopped) => {
                self.apply_stop(&item, stop.unwrap_or(StopReason::Shutdown))
            }
            Err(TransferError::Retryable(msg)) => {
                tracing::error!(id = item.id, error = %msg, "download failed after retries");
                self.fail(&item, &format!("{}: {msg}", error_keys::NETWORK))
            }
            Err(TransferError::Fatal(msg)) => {
                tracing::error!(id = item.id, error = %msg, "download failed");
                self.fail(&item, &msg)
            }
        };
        if let Err(e) = outcome {
            tracing::error!(id = item.id, error = %e, "cannot record download result");
        }
    }

    async fn transfer(
        &self,
        item: &DownloadItem,
        token: &CancellationToken,
    ) -> Result<String, TransferError> {
        let io = |e: std::io::Error| TransferError::Fatal(format!("{}: {e}", error_keys::DISK));
        let part = part_path(&item.dest);
        if let Some(parent) = item.dest.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io)?;
        }
        // A complete file left at the destination (e.g. database lost) is re-verified, not re-downloaded.
        if !part.exists()
            && item.size > 0
            && tokio::fs::metadata(&item.dest).await.map(|m| m.len()).ok() == Some(item.size)
        {
            tokio::fs::rename(&item.dest, &part).await.map_err(io)?;
        }

        let mut hasher = Sha256::new();
        let mut existing = tokio::fs::metadata(&part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if item.size > 0 && existing > item.size {
            tracing::warn!(
                id = item.id,
                existing,
                size = item.size,
                "partial file larger than expected, restarting"
            );
            tokio::fs::remove_file(&part).await.map_err(io)?;
            existing = 0;
        }
        if existing > 0 {
            self.report(item, existing, 0, DownloadStatus::Verifying);
            hash_existing(&part, existing, &mut hasher, token).await?;
            tracing::debug!(id = item.id, bytes = existing, "partial file re-hashed");
        }
        if item.size > 0 && existing == item.size {
            return Ok(hex::encode(hasher.finalize()));
        }

        let config = self.config();
        let mut req = self.inner.client.get(&item.url);
        if existing > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={existing}-"));
        }
        if let Some(t) = config
            .hf_token
            .as_deref()
            .filter(|_| hf::is_hf_url(&item.url))
        {
            req = req.bearer_auth(t);
        }
        let resp = tokio::select! {
            r = req.send() => r.map_err(|e| TransferError::Retryable(e.to_string()))?,
            _ = token.cancelled() => return Err(TransferError::Stopped),
        };
        let status = resp.status().as_u16();
        tracing::debug!(
            id = item.id,
            status,
            resume_from = existing,
            "download response"
        );
        let append = match status {
            206 => true,
            200 => {
                if existing > 0 {
                    tracing::info!(
                        id = item.id,
                        "server ignored the Range request, restarting from zero"
                    );
                    hasher = Sha256::new();
                    existing = 0;
                }
                false
            }
            401 | 403 => {
                return Err(TransferError::Fatal(format!(
                    "{}: HTTP {status}",
                    error_keys::AUTH
                )));
            }
            404 => return Err(TransferError::Fatal(error_keys::NOT_FOUND.to_owned())),
            416 => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(TransferError::Retryable("HTTP 416, restarting".into()));
            }
            408 | 429 | 500..=599 => {
                return Err(TransferError::Retryable(format!("HTTP {status}")));
            }
            _ => {
                return Err(TransferError::Fatal(format!(
                    "{}: HTTP {status}",
                    error_keys::HTTP
                )));
            }
        };

        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&part)
            .await
            .map_err(io)?;
        let mut writer = BufWriter::with_capacity(WRITE_BUFFER, file);
        let mut stream = resp.bytes_stream();
        let mut downloaded = existing;
        let mut last_report = Instant::now();
        let mut last_persist = Instant::now();
        let mut report_bytes = downloaded;
        let mut throttle = Throttle::new();
        self.report(item, downloaded, 0, DownloadStatus::Downloading);

        loop {
            let next = tokio::select! {
                c = stream.next() => c,
                _ = token.cancelled() => {
                    writer.flush().await.map_err(io)?;
                    self.persist_downloaded(item.id, downloaded);
                    return Err(TransferError::Stopped);
                }
            };
            match next {
                None => break,
                Some(Err(e)) => {
                    writer.flush().await.map_err(io)?;
                    self.persist_downloaded(item.id, downloaded);
                    return Err(TransferError::Retryable(e.to_string()));
                }
                Some(Ok(bytes)) => {
                    writer.write_all(&bytes).await.map_err(io)?;
                    hasher.update(&bytes);
                    downloaded += bytes.len() as u64;
                    let limit = self.config().bandwidth_limit_kbps;
                    if let Some(wait) = throttle.after(bytes.len() as u64, limit) {
                        tokio::select! {
                            _ = tokio::time::sleep(wait) => {}
                            _ = token.cancelled() => {}
                        }
                    }
                }
            }
            let elapsed = last_report.elapsed();
            if elapsed >= PROGRESS_INTERVAL {
                let speed = ((downloaded - report_bytes) as f64 / elapsed.as_secs_f64()) as u64;
                self.report(item, downloaded, speed, DownloadStatus::Downloading);
                last_report = Instant::now();
                report_bytes = downloaded;
            }
            if last_persist.elapsed() >= PERSIST_INTERVAL {
                writer.flush().await.map_err(io)?;
                self.persist_downloaded(item.id, downloaded);
                last_persist = Instant::now();
            }
        }
        writer.flush().await.map_err(io)?;
        writer.get_ref().sync_all().await.map_err(io)?;
        drop(writer);
        self.persist_downloaded(item.id, downloaded);

        if item.size > 0 && downloaded != item.size {
            if downloaded > item.size {
                let _ = tokio::fs::remove_file(&part).await;
            }
            return Err(TransferError::Retryable(format!(
                "incomplete transfer: {downloaded} of {} bytes",
                item.size
            )));
        }
        Ok(hex::encode(hasher.finalize()))
    }

    fn finish(&self, item: &DownloadItem, hash: &str) -> Result<(), DownloadError> {
        self.set_status(item.id, DownloadStatus::Verifying, None)?;
        let part = part_path(&item.dest);
        if let Some(expected) = item.sha256.as_deref().filter(|s| !s.is_empty()) {
            if !hash.eq_ignore_ascii_case(expected) {
                let _ = std::fs::remove_file(&part);
                let attempts = item.attempts + 1;
                tracing::warn!(id = item.id, file = %item.file_path, expected, actual = hash, attempts, "checksum mismatch, file discarded");
                let conn = self.inner.db.conn();
                if attempts < MAX_CHECKSUM_ATTEMPTS {
                    conn.execute(
                        "UPDATE downloads SET status = 'queued', downloaded = 0, attempts = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
                        params![item.id, attempts, error_keys::CHECKSUM, now_secs()],
                    )?;
                    drop(conn);
                    self.inner.events.changed();
                    self.inner.wake.notify_one();
                } else {
                    conn.execute(
                        "UPDATE downloads SET status = 'failed', downloaded = 0, attempts = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
                        params![item.id, attempts, error_keys::CHECKSUM, now_secs()],
                    )?;
                    drop(conn);
                    tracing::error!(id = item.id, file = %item.file_path, "checksum mismatch again, download failed");
                    self.inner.events.changed();
                }
                return Ok(());
            }
        } else {
            tracing::warn!(id = item.id, file = %item.file_path, "no checksum to verify against");
        }

        if item.dest.exists() {
            std::fs::remove_file(&item.dest)?;
        }
        std::fs::rename(&part, &item.dest)?;
        let size = std::fs::metadata(&item.dest)?.len();
        self.inner.db.insert_installed_file(&InstalledFile {
            model_id: item.model_id.clone(),
            variant_id: item.variant_id.clone(),
            file_path: item.file_path.clone(),
            repo: item.repo.clone(),
            local_path: item.dest.to_string_lossy().into_owned(),
            size,
            sha256: Some(hash.to_owned()),
            source: FileSource::Download,
        })?;
        self.inner.db.conn().execute(
            "UPDATE downloads SET status = 'completed', downloaded = ?2, error = NULL, updated_at = ?3 WHERE id = ?1",
            params![item.id, size as i64, now_secs()],
        )?;
        tracing::info!(id = item.id, model = %item.model_id, file = %item.file_path, bytes = size, "download completed and verified");
        self.report(item, size, 0, DownloadStatus::Completed);
        self.finish_variant_if_complete(&item.model_id, &item.variant_id)?;
        self.inner.events.changed();
        Ok(())
    }

    fn finish_variant_if_complete(
        &self,
        model_id: &str,
        variant_id: &str,
    ) -> Result<(), DownloadError> {
        let pending: i64 = self.inner.db.conn().query_row(
            "SELECT COUNT(*) FROM downloads WHERE model_id = ?1 AND variant_id = ?2 AND status != 'completed'",
            params![model_id, variant_id],
            |r| r.get(0),
        )?;
        if pending > 0 {
            return Ok(());
        }
        let installed = self.inner.db.installed_files_for(model_id, variant_id)?;
        let expected = self.inner.db.variant_files(model_id, variant_id)?;
        let missing: Vec<&String> = expected
            .iter()
            .filter(|p| !installed.iter().any(|i| &i.file_path == *p))
            .collect();
        // Variants queued before migration 5 have no file list: any file counts.
        let complete = if expected.is_empty() {
            !installed.is_empty()
        } else {
            missing.is_empty()
        };
        if complete {
            tracing::info!(model = %model_id, variant = %variant_id, "model variant installed");
            self.inner.events.variant_installed(model_id, variant_id);
        } else if !installed.is_empty() {
            tracing::warn!(model = %model_id, variant = %variant_id, missing = missing.len(), "model variant incomplete: some files were cancelled or are missing");
        }
        Ok(())
    }

    fn apply_stop(&self, item: &DownloadItem, reason: StopReason) -> Result<(), DownloadError> {
        match reason {
            StopReason::Pause => {
                tracing::info!(id = item.id, "download paused");
                self.set_status(item.id, DownloadStatus::Paused, None)
            }
            StopReason::Cancel => self.remove_item(item),
            // Left as `downloading`; re-queued by `new()` on the next start.
            StopReason::Shutdown => {
                tracing::info!(id = item.id, "download suspended for shutdown");
                Ok(())
            }
        }
    }

    fn fail(&self, item: &DownloadItem, error: &str) -> Result<(), DownloadError> {
        self.set_status(item.id, DownloadStatus::Failed, Some(error))
    }

    fn remove_item(&self, item: &DownloadItem) -> Result<(), DownloadError> {
        let part = part_path(&item.dest);
        if part.exists() {
            std::fs::remove_file(&part)?;
        }
        self.inner
            .db
            .conn()
            .execute("DELETE FROM downloads WHERE id = ?1", params![item.id])?;
        tracing::info!(id = item.id, file = %item.file_path, "download cancelled");
        self.inner.events.changed();
        Ok(())
    }

    fn set_status(
        &self,
        id: i64,
        status: DownloadStatus,
        error: Option<&str>,
    ) -> Result<(), DownloadError> {
        self.inner.db.conn().execute(
            "UPDATE downloads SET status = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, status.as_str(), error, now_secs()],
        )?;
        self.inner.events.changed();
        Ok(())
    }

    fn persist_downloaded(&self, id: i64, downloaded: u64) {
        if let Err(e) = self.inner.db.conn().execute(
            "UPDATE downloads SET downloaded = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, downloaded as i64, now_secs()],
        ) {
            tracing::warn!(id, error = %e, "cannot save download progress");
        }
    }

    fn report(
        &self,
        item: &DownloadItem,
        downloaded: u64,
        bytes_per_sec: u64,
        status: DownloadStatus,
    ) {
        let p = Progress {
            id: item.id,
            model_id: item.model_id.clone(),
            variant_id: item.variant_id.clone(),
            downloaded,
            total: item.size,
            bytes_per_sec,
            status,
        };
        tracing::trace!(
            id = item.id,
            downloaded,
            total = item.size,
            bytes_per_sec,
            "download progress"
        );
        self.inner.events.progress(&p);
    }

    /// Signals the active transfer to stop if it is `id`. Returns false when
    /// `id` is not the active item.
    fn stop_active(&self, id: i64, reason: StopReason) -> bool {
        let active = self.inner.active.lock().unwrap_or_else(|p| p.into_inner());
        match active.as_ref() {
            Some(a) if a.id == id => {
                *a.reason.lock().unwrap_or_else(|p| p.into_inner()) = Some(reason);
                a.token.cancel();
                true
            }
            _ => false,
        }
    }

    fn stop_active_any(&self, reason: StopReason) {
        let active = self.inner.active.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(a) = active.as_ref() {
            *a.reason.lock().unwrap_or_else(|p| p.into_inner()) = Some(reason);
            a.token.cancel();
        }
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

async fn hash_existing(
    path: &Path,
    len: u64,
    hasher: &mut Sha256,
    token: &CancellationToken,
) -> Result<(), TransferError> {
    let io = |e: std::io::Error| TransferError::Fatal(format!("{}: {e}", error_keys::DISK));
    let mut file = tokio::fs::File::open(path).await.map_err(io)?;
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut remaining = len;
    while remaining > 0 {
        if token.is_cancelled() {
            return Err(TransferError::Stopped);
        }
        let want = buf.len().min(remaining as usize);
        let n = file.read(&mut buf[..want]).await.map_err(io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        remaining -= n as u64;
    }
    Ok(())
}

/// Simple rate limiter: sleeps so that the average rate over a few seconds
/// stays at or below the configured limit.
struct Throttle {
    window_start: Instant,
    window_bytes: u64,
}

impl Throttle {
    fn new() -> Self {
        Throttle {
            window_start: Instant::now(),
            window_bytes: 0,
        }
    }

    fn after(&mut self, bytes: u64, limit_kbps: Option<u32>) -> Option<Duration> {
        let limit = u64::from(limit_kbps.filter(|l| *l > 0)?) * 1024;
        if self.window_start.elapsed() > Duration::from_secs(5) {
            self.window_start = Instant::now();
            self.window_bytes = 0;
        }
        self.window_bytes += bytes;
        let expected = Duration::from_secs_f64(self.window_bytes as f64 / limit as f64);
        expected.checked_sub(self.window_start.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planned_files_keep_their_folder() {
        let mut f = PlannedFile {
            repo: "o/r".into(),
            path: "a/b/model.safetensors".into(),
            url: String::new(),
            size: 1,
            sha256: None,
            gated: false,
            local: String::new(),
        };
        let dir = Path::new("models");
        assert_eq!(f.dest_in(dir), dir.join("model.safetensors"));
        assert!(!f.nested());
        f.local = "lm/model.safetensors".into();
        assert_eq!(f.dest_in(dir), dir.join("lm").join("model.safetensors"));
        assert!(f.nested());
    }

    #[test]
    fn space_check_includes_ten_percent() {
        assert!(check_space(1000, 1100).is_ok());
        assert!(matches!(
            check_space(1000, 1099),
            Err(DownloadError::InsufficientSpace { needed: 1100, .. })
        ));
    }

    #[test]
    fn part_file_names() {
        assert_eq!(
            part_path(Path::new("/m/model.gguf")),
            PathBuf::from("/m/model.gguf.part")
        );
    }

    #[test]
    fn throttle_waits_when_over_limit() {
        let mut t = Throttle::new();
        assert!(t.after(1024, None).is_none());
        let wait = t.after(100 * 1024, Some(10)).expect("10 KiB/s limit");
        assert!(wait > Duration::from_secs(9));
    }
}

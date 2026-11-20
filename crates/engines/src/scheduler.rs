//! VRAM scheduler (PRD §4.4, ADR 0015).
//!
//! Knows the memory budget, the models kept loaded by server engines
//! ("residents", e.g. `llama-server`) and the memory leased by running jobs
//! (image, transcription). Before a load or a job it makes room by unloading
//! the least recently used resident that may go; jobs that still do not fit
//! wait for other jobs, and when nothing can be freed they run anyway with a
//! "tight" flag (the engine then offloads to RAM).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use lokalsi_core::db::now_secs;

/// Unloads the resident model of an engine. Returns false when it cannot go
/// right now (e.g. it is generating).
pub type Evictor = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// Called with the new status whenever memory use changes (memory meter).
pub type OnChange = Arc<dyn Fn(&MemoryStatus) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// A model kept loaded by a server engine.
    Resident,
    /// Memory used by a running job.
    Job,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryItem {
    /// `resident:<engine>` or `job:<id>`.
    pub key: String,
    pub kind: ItemKind,
    pub engine_id: String,
    pub name: String,
    pub memory_mb: u64,
    pub since: i64,
    pub last_used: i64,
    /// Protected from automatic unloading ("keep the chat model in memory").
    pub keep: bool,
    /// Job id for job items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStatus {
    /// `None` until the hardware is known.
    pub budget_mb: Option<u64>,
    pub used_mb: u64,
    pub keep_llm: bool,
    pub items: Vec<MemoryItem>,
}

/// Where a job landed: `tight` = it did not fit even after unloading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub tight: bool,
}

struct State {
    budget_mb: Option<u64>,
    keep_llm: bool,
    residents: HashMap<String, MemoryItem>,
    leases: HashMap<i64, MemoryItem>,
}

impl State {
    fn used(&self, except_engine: Option<&str>) -> u64 {
        self.residents
            .values()
            .filter(|r| Some(r.engine_id.as_str()) != except_engine)
            .chain(self.leases.values())
            .map(|i| i.memory_mb)
            .sum()
    }

    fn status(&self) -> MemoryStatus {
        let mut items: Vec<MemoryItem> = self
            .residents
            .values()
            .chain(self.leases.values())
            .cloned()
            .collect();
        items.sort_by_key(|i| (i.since, i.key.clone()));
        MemoryStatus {
            budget_mb: self.budget_mb,
            used_mb: self.used(None),
            keep_llm: self.keep_llm,
            items,
        }
    }
}

/// Engines whose resident model counts as "the chat model".
const LLM_ENGINES: &[&str] = &["llama-cpp"];

pub struct Scheduler {
    state: Mutex<State>,
    evictors: Mutex<HashMap<String, Evictor>>,
    /// Serialises admissions so two jobs never claim the same free memory.
    admission: tokio::sync::Mutex<()>,
    released: tokio::sync::Notify,
    on_change: Mutex<Option<OnChange>>,
}

/// Memory held by a running job; dropping it frees the memory.
pub struct Lease {
    scheduler: Arc<Scheduler>,
    job_id: i64,
    pub placement: Placement,
}

impl Drop for Lease {
    fn drop(&mut self) {
        let removed = self.scheduler.lock().leases.remove(&self.job_id).is_some();
        if removed {
            tracing::debug!(job_id = self.job_id, "job memory released");
            self.scheduler.released.notify_waiters();
            self.scheduler.changed();
        }
    }
}

impl Scheduler {
    pub fn new() -> Arc<Self> {
        Arc::new(Scheduler {
            state: Mutex::new(State {
                budget_mb: None,
                keep_llm: false,
                residents: HashMap::new(),
                leases: HashMap::new(),
            }),
            evictors: Mutex::new(HashMap::new()),
            admission: tokio::sync::Mutex::new(()),
            released: tokio::sync::Notify::new(),
            on_change: Mutex::new(None),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn changed(&self) {
        let status = self.status();
        let cb = self
            .on_change
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(cb) = cb {
            cb(&status);
        }
    }

    pub fn set_on_change(&self, cb: OnChange) {
        *self.on_change.lock().unwrap_or_else(|p| p.into_inner()) = Some(cb);
    }

    pub fn status(&self) -> MemoryStatus {
        self.lock().status()
    }

    pub fn set_budget(&self, budget_mb: Option<u64>) {
        let changed = {
            let mut s = self.lock();
            let changed = s.budget_mb != budget_mb;
            s.budget_mb = budget_mb;
            changed
        };
        if changed {
            tracing::info!(?budget_mb, "memory budget set");
            self.changed();
        }
    }

    /// "Keep the chat model in memory" (Settings → Hardware).
    pub fn set_keep_llm(&self, keep: bool) {
        {
            let mut s = self.lock();
            s.keep_llm = keep;
            for r in s.residents.values_mut() {
                if LLM_ENGINES.contains(&r.engine_id.as_str()) {
                    r.keep = keep;
                }
            }
        }
        self.changed();
    }

    pub fn register_evictor(&self, engine_id: &str, evictor: Evictor) {
        self.evictors
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(engine_id.to_owned(), evictor);
    }

    // --- residents ----------------------------------------------------------------

    pub fn resident_loaded(&self, engine_id: &str, name: &str, memory_mb: u64) {
        {
            let mut s = self.lock();
            let keep = s.keep_llm && LLM_ENGINES.contains(&engine_id);
            let now = now_secs();
            s.residents.insert(
                engine_id.to_owned(),
                MemoryItem {
                    key: format!("resident:{engine_id}"),
                    kind: ItemKind::Resident,
                    engine_id: engine_id.to_owned(),
                    name: name.to_owned(),
                    memory_mb,
                    since: now,
                    last_used: now,
                    keep,
                    job_id: None,
                },
            );
        }
        tracing::info!(
            engine = engine_id,
            model = name,
            memory_mb,
            "model resident in memory"
        );
        self.changed();
    }

    pub fn resident_unloaded(&self, engine_id: &str) {
        let removed = self.lock().residents.remove(engine_id).is_some();
        if removed {
            tracing::info!(engine = engine_id, "model unloaded from memory");
            self.released.notify_waiters();
            self.changed();
        }
    }

    /// Marks the resident model of `engine_id` as just used (LRU).
    pub fn touch(&self, engine_id: &str) {
        if let Some(r) = self.lock().residents.get_mut(engine_id) {
            r.last_used = now_secs();
        }
    }

    // --- making room --------------------------------------------------------------

    /// Frees memory for `need_mb` (not counting `for_engine`'s own resident).
    /// Unloads least recently used residents that may go; with `wait_for_jobs`
    /// it then waits for running jobs to finish. Returns `None` when cancelled.
    pub async fn make_room(
        &self,
        need_mb: u64,
        for_engine: &str,
        wait_for_jobs: bool,
        cancel: &CancellationToken,
    ) -> Option<Placement> {
        let mut refused: Vec<String> = Vec::new();
        loop {
            if cancel.is_cancelled() {
                return None;
            }
            // Register interest before checking, so a release in between is not missed.
            let released = self.released.notified();
            let (free, victim, jobs_running) = {
                let s = self.lock();
                let Some(budget) = s.budget_mb else {
                    return Some(Placement { tight: false });
                };
                let free = budget.saturating_sub(s.used(Some(for_engine)));
                let victim = s
                    .residents
                    .values()
                    .filter(|r| r.engine_id != for_engine && !r.keep)
                    .filter(|r| !refused.contains(&r.engine_id))
                    .min_by_key(|r| r.last_used)
                    .map(|r| r.engine_id.clone());
                (free, victim, !s.leases.is_empty())
            };
            if need_mb <= free {
                return Some(Placement { tight: false });
            }
            if let Some(engine) = victim {
                let evictor = self
                    .evictors
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(&engine)
                    .cloned();
                tracing::info!(engine = %engine, need_mb, free_mb = free, "unloading least recently used model");
                let evicted = match evictor {
                    Some(evict) => evict().await,
                    None => false,
                };
                if evicted {
                    // The engine reports `resident_unloaded`; make sure it is gone.
                    self.resident_unloaded(&engine);
                } else {
                    refused.push(engine);
                }
                continue;
            }
            if wait_for_jobs && jobs_running {
                tracing::debug!(
                    need_mb,
                    free_mb = free,
                    "waiting for running jobs to free memory"
                );
                tokio::select! {
                    _ = released => {}
                    _ = cancel.cancelled() => return None,
                }
                refused.clear();
                continue;
            }
            tracing::warn!(
                need_mb,
                free_mb = free,
                "not enough GPU memory, running with offload"
            );
            return Some(Placement { tight: true });
        }
    }

    /// Makes room and leases `memory_mb` for job `job_id` until the lease is
    /// dropped. `None` when cancelled while waiting.
    pub async fn acquire(
        self: &Arc<Self>,
        job_id: i64,
        engine_id: &str,
        name: &str,
        memory_mb: u64,
        cancel: &CancellationToken,
    ) -> Option<Lease> {
        let _admission = tokio::select! {
            g = self.admission.lock() => g,
            _ = cancel.cancelled() => return None,
        };
        let placement = if memory_mb == 0 {
            Placement { tight: false }
        } else {
            self.make_room(memory_mb, engine_id, true, cancel).await?
        };
        {
            let now = now_secs();
            self.lock().leases.insert(
                job_id,
                MemoryItem {
                    key: format!("job:{job_id}"),
                    kind: ItemKind::Job,
                    engine_id: engine_id.to_owned(),
                    name: name.to_owned(),
                    memory_mb,
                    since: now,
                    last_used: now,
                    keep: false,
                    job_id: Some(job_id),
                },
            );
        }
        tracing::debug!(
            job_id,
            engine = engine_id,
            memory_mb,
            tight = placement.tight,
            "job memory leased"
        );
        self.changed();
        Some(Lease {
            scheduler: Arc::clone(self),
            job_id,
            placement,
        })
    }

    /// "Release" in the memory meter: unloads a resident model even when it
    /// is kept. Jobs are cancelled by their owner instead.
    pub async fn release(&self, engine_id: &str) -> bool {
        let evictor = self
            .evictors
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(engine_id)
            .cloned();
        let ok = match evictor {
            Some(evict) => evict().await,
            None => false,
        };
        if ok {
            self.resident_unloaded(engine_id);
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn evictor(
        s: &Arc<Scheduler>,
        engine: &'static str,
        calls: Arc<AtomicUsize>,
        ok: bool,
    ) -> Evictor {
        let weak = Arc::downgrade(s);
        Arc::new(move || {
            let weak = weak.clone();
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                if ok && let Some(s) = weak.upgrade() {
                    s.resident_unloaded(engine);
                }
                ok
            })
        })
    }

    #[tokio::test]
    async fn unloads_the_chat_model_for_an_image_job() {
        let s = Scheduler::new();
        s.set_budget(Some(24_000));
        let calls = Arc::new(AtomicUsize::new(0));
        s.register_evictor(
            "llama-cpp",
            evictor(&s, "llama-cpp", Arc::clone(&calls), true),
        );
        s.resident_loaded("llama-cpp", "Bielik 11B", 13_000);
        let cancel = CancellationToken::new();

        // A small transcription fits next to the chat model.
        let stt = s
            .acquire(1, "whisper-cpp", "Whisper", 1_200, &cancel)
            .await
            .unwrap();
        assert!(!stt.placement.tight);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(stt);

        // The image job does not fit: the chat model goes.
        let img = s
            .acquire(2, "sd-cpp", "Z-Image", 13_800, &cancel)
            .await
            .unwrap();
        assert!(!img.placement.tight);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let st = s.status();
        assert_eq!(st.used_mb, 13_800);
        assert_eq!(st.items.len(), 1);
        drop(img);
        assert_eq!(s.status().used_mb, 0, "memory is free again after the job");
    }

    #[tokio::test]
    async fn keep_llm_protects_the_chat_model() {
        let s = Scheduler::new();
        s.set_budget(Some(24_000));
        let calls = Arc::new(AtomicUsize::new(0));
        s.register_evictor(
            "llama-cpp",
            evictor(&s, "llama-cpp", Arc::clone(&calls), true),
        );
        s.set_keep_llm(true);
        s.resident_loaded("llama-cpp", "Bielik 11B", 13_000);
        let lease = s
            .acquire(1, "sd-cpp", "Z-Image", 13_800, &CancellationToken::new())
            .await
            .unwrap();
        assert!(lease.placement.tight, "runs with offload instead");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(s.status().items.iter().any(|i| i.keep));
        // "Release" in the meter still works.
        assert!(s.release("llama-cpp").await);
        assert_eq!(s.status().used_mb, 13_800);
    }

    #[tokio::test]
    async fn busy_resident_is_skipped_and_jobs_wait_for_each_other() {
        let s = Scheduler::new();
        s.set_budget(Some(10_000));
        let calls = Arc::new(AtomicUsize::new(0));
        s.register_evictor(
            "llama-cpp",
            evictor(&s, "llama-cpp", Arc::clone(&calls), false),
        );
        s.resident_loaded("llama-cpp", "LLM", 3_000);
        let cancel = CancellationToken::new();
        let first = s.acquire(1, "sd-cpp", "A", 6_000, &cancel).await.unwrap();
        assert!(!first.placement.tight);

        let s2 = Arc::clone(&s);
        let waiting = tokio::spawn(async move {
            s2.acquire(2, "whisper-cpp", "B", 5_000, &CancellationToken::new())
                .await
                .map(|l| l.placement)
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!waiting.is_finished(), "waits for the running job");
        drop(first);
        let placement = waiting.await.unwrap().unwrap();
        assert!(!placement.tight);
        assert!(
            calls.load(Ordering::SeqCst) >= 1,
            "the busy LLM was asked and refused"
        );
    }

    #[tokio::test]
    async fn unknown_budget_and_cancel() {
        let s = Scheduler::new();
        let lease = s
            .acquire(1, "sd-cpp", "A", 50_000, &CancellationToken::new())
            .await
            .unwrap();
        assert!(!lease.placement.tight, "no budget known: no limits");
        s.set_budget(Some(1_000));
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(s.acquire(2, "sd-cpp", "B", 500, &cancel).await.is_none());
    }
}

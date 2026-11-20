//! Memory meter and the job list: what every edition needs (decisions are jobs
//! too). Starting jobs of the media modes belongs to the Pro edition.

use lokalsi_core::jobs::{Job, JobFilter, Jobs};
use lokalsi_engines::scheduler::MemoryStatus;
use tauri::State;

use crate::AppState;
use crate::models::CmdError;

type Res<T> = Result<T, CmdError>;

fn err(e: impl std::fmt::Display) -> CmdError {
    CmdError::new("error", e.to_string())
}

// --- memory -----------------------------------------------------------------------

#[tauri::command]
pub fn get_memory(state: State<'_, AppState>) -> MemoryStatus {
    tracing::trace!(command = "get_memory");
    state.scheduler.status()
}

/// "Release" in the memory meter: unloads a model or cancels a job.
#[tauri::command]
pub async fn release_memory(key: String, state: State<'_, AppState>) -> Res<bool> {
    tracing::debug!(command = "release_memory", %key);
    if let Some(engine) = key.strip_prefix("resident:") {
        return Ok(state.scheduler.release(engine).await);
    }
    if let Some(id) = key.strip_prefix("job:").and_then(|i| i.parse::<i64>().ok()) {
        return Ok(state.queue.cancel(id));
    }
    Err(CmdError::new("not_found", key))
}

// --- jobs -------------------------------------------------------------------------

#[tauri::command]
pub fn list_jobs(filter: JobFilter, state: State<'_, AppState>) -> Res<Vec<Job>> {
    tracing::trace!(command = "list_jobs", ?filter);
    Jobs::new(&state.db).list(&filter).map_err(err)
}

#[tauri::command]
pub fn cancel_job(id: i64, state: State<'_, AppState>) -> bool {
    tracing::debug!(command = "cancel_job", id);
    state.queue.cancel(id)
}

#[tauri::command]
pub fn delete_jobs(ids: Vec<i64>, state: State<'_, AppState>) -> Res<usize> {
    tracing::debug!(command = "delete_jobs", count = ids.len());
    let mut n = 0;
    for id in ids {
        if state.queue.delete(id).map_err(err)? {
            n += 1;
        }
    }
    Ok(n)
}

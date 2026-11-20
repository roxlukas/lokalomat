//! Workshop mode "Decyzje" (ADR 0024): typed questions about a text answered
//! by a decision model (Basal) with probabilities. A thin IPC layer over
//! `ModeRunner` (second llama.cpp slot) and the job queue.

use std::sync::Arc;

use lokalsi_core::decisions::Question;
use lokalsi_core::jobs::Job;
use lokalsi_core::pipelines::ModelRef;
use serde::Deserialize;
use tauri::State;

use crate::AppState;
use crate::models::CmdError;

type Res<T> = Result<T, CmdError>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionForm {
    /// The text to judge.
    state: String,
    questions: Vec<Question>,
    model_id: Option<String>,
    variant_id: Option<String>,
}

#[tauri::command]
pub fn decide(form: DecisionForm, state: State<'_, AppState>) -> Res<Job> {
    let text = form.state.trim().to_owned();
    tracing::debug!(command = "decide", chars = text.chars().count(), questions = form.questions.len(), model = ?form.model_id);
    let model = form
        .model_id
        .zip(form.variant_id)
        .map(|(model_id, variant_id)| ModelRef {
            model_id,
            variant_id,
        });
    let (job, req) = state
        .modes
        .create_decision(&text, form.questions, model.as_ref())
        .map_err(|e| CmdError::new(e.code, e.detail))?;
    let queue = Arc::clone(&state.queue);
    let decider = Arc::clone(state.modes.decider());
    let id = job.id;
    tauri::async_runtime::spawn(async move { queue.run_decision(id, decider, req).await });
    Ok(job)
}

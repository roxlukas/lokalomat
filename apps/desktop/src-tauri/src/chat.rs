//! IPC for M2: engines (V5), chat sessions and chat jobs (Workshop, Chat mode).

use std::path::PathBuf;
use std::sync::Arc;

use lokalsi_core::catalog::Modality;
use lokalsi_core::chats::{Chats, Message, MessageStatus, Role, Session, title_from};
use lokalsi_core::library::Library;
use lokalsi_core::model_params::{ModelParams, ModelParamsError, ModelSettings};
use lokalsi_engines::llama::{self, ChatParams};
use lokalsi_engines::service::{EngineService, EngineStatus, LoadRequest};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::AppState;
use crate::models::CmdError;

type Res<T> = Result<T, CmdError>;

fn db_err(e: impl std::fmt::Display) -> CmdError {
    CmdError::new("error", e.to_string())
}

/// Engines directory: `<data_dir>/engines`, or the app home before a data folder is chosen.
pub fn engines_dir(state: &AppState) -> PathBuf {
    state
        .settings()
        .data_dir
        .clone()
        .unwrap_or_else(|| state.home.clone())
        .join("engines")
}

fn llm(state: &AppState) -> Arc<EngineService> {
    Arc::clone(&state.llm)
}

/// The engine `id` (default: llama.cpp); the edition's own engines are handled by its extension.
fn engine(state: &AppState, id: Option<&str>) -> Res<Arc<EngineService>> {
    let id = id.unwrap_or("llama-cpp");
    if id == "llama-cpp" {
        return Ok(llm(state));
    }
    Err(CmdError::new("not_found", id.to_owned()))
}

// --- engines -----------------------------------------------------------------------

#[tauri::command]
pub fn get_engines(state: State<'_, AppState>) -> Vec<EngineStatus> {
    tracing::trace!(command = "get_engines");
    let profile = state.hardware();
    let mut list = vec![state.llm.status(profile.as_ref())];
    list.extend(state.ext.engine_statuses(profile.as_ref()));
    list
}

/// "Napraw środowisko" (Expert): rebuilds a Python environment from scratch.
#[tauri::command]
pub fn repair_engine(engine_id: String, app: AppHandle) -> Res<()> {
    tracing::debug!(command = "repair_engine", %engine_id);
    let state = app.state::<AppState>();
    if state
        .ext
        .install_engine(&engine_id, None, engines_dir(&state), None, true)
    {
        Ok(())
    } else {
        Err(CmdError::new("not_found", engine_id))
    }
}

/// Starts the installation in the background; progress arrives as `engines://changed`.
#[tauri::command]
pub fn install_engine(
    engine_id: Option<String>,
    build_id: Option<String>,
    app: AppHandle,
) -> Res<()> {
    tracing::debug!(command = "install_engine", ?engine_id, ?build_id);
    let state = app.state::<AppState>();
    let profile = state.hardware();
    if let Some(id) = engine_id.as_deref()
        && state.ext.install_engine(
            id,
            build_id.clone(),
            engines_dir(&state),
            profile.clone(),
            false,
        )
    {
        return Ok(());
    }
    let service = engine(&state, engine_id.as_deref())?;
    service.set_engines_dir(engines_dir(&state));
    tauri::async_runtime::spawn(async move {
        if let Err(e) = service.install(build_id.as_deref(), profile.as_ref()).await {
            tracing::warn!(error = %e, "engine install did not complete");
        }
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_engine_install(engine_id: Option<String>, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "cancel_engine_install", ?engine_id);
    if let Some(id) = engine_id.as_deref()
        && state.ext.cancel_engine_install(id)
    {
        return Ok(());
    }
    engine(&state, engine_id.as_deref())?.cancel_install();
    Ok(())
}

#[tauri::command]
pub async fn uninstall_engine(engine_id: Option<String>, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "uninstall_engine", ?engine_id);
    if let Some(id) = engine_id.as_deref()
        && let Some(work) = state.ext.uninstall_engine(id)
    {
        return work.await.map_err(db_err);
    }
    engine(&state, engine_id.as_deref())?
        .uninstall()
        .await
        .map_err(db_err)
}

/// Unloads the model ("Zwolnij" in the memory meter).
#[tauri::command]
pub async fn stop_engine(state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "stop_engine");
    llm(&state).stop_server().await;
    Ok(())
}

#[tauri::command]
pub async fn restart_engine(engine_id: Option<String>, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "restart_engine", ?engine_id);
    if let Some(id) = engine_id.as_deref()
        && let Some(work) = state.ext.restart_engine(id)
    {
        return work
            .await
            .map_err(|(code, detail)| CmdError::new(code, detail));
    }
    engine(&state, engine_id.as_deref())?
        .restart()
        .await
        .map_err(|(code, detail)| CmdError::new(code, detail))
}

#[tauri::command]
pub fn get_engine_log(
    engine_id: Option<String>,
    lines: usize,
    state: State<'_, AppState>,
) -> Res<String> {
    tracing::trace!(command = "get_engine_log", ?engine_id, lines);
    let path = match engine_id
        .as_deref()
        .and_then(|id| state.ext.engine_log_path(id))
    {
        Some(path) => path,
        None => engine(&state, engine_id.as_deref())?.status(None).log_path,
    };
    Ok(lokalsi_core::redact::redact(&llama::log_tail(
        &path,
        lines.clamp(1, 2000),
    )))
}

// --- sessions --------------------------------------------------------------------------

#[tauri::command]
pub fn list_sessions(query: Option<String>, state: State<'_, AppState>) -> Res<Vec<Session>> {
    tracing::trace!(command = "list_sessions");
    Chats::new(&state.db)
        .list_sessions(query.as_deref())
        .map_err(db_err)
}

#[tauri::command]
pub fn get_messages(session_id: i64, state: State<'_, AppState>) -> Res<Vec<Message>> {
    tracing::trace!(command = "get_messages", session_id);
    Chats::new(&state.db).messages(session_id).map_err(db_err)
}

#[tauri::command]
pub fn rename_session(id: i64, title: String, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "rename_session", id);
    Chats::new(&state.db)
        .rename_session(id, &title)
        .map_err(db_err)
}

#[tauri::command]
pub fn pin_session(id: i64, pinned: bool, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "pin_session", id, pinned);
    Chats::new(&state.db)
        .pin_session(id, pinned)
        .map_err(db_err)
}

#[tauri::command]
pub fn delete_session(id: i64, state: State<'_, AppState>) -> Res<()> {
    tracing::debug!(command = "delete_session", id);
    Chats::new(&state.db).delete_session(id).map_err(db_err)
}

// --- chat ------------------------------------------------------------------------------------

/// Chooses the model for a chat: the requested one, the default chat model,
/// or the first installed chat model.
fn resolve_model(state: &AppState, requested: Option<(String, String)>) -> Res<LoadRequest> {
    let lib = Library::new(&state.catalog, &state.db);
    let defaults = state.db.default_models().map_err(db_err)?;
    let installed = state.db.installed_files().map_err(db_err)?;
    let (model_id, variant_id) = match requested {
        Some(m) => m,
        None => defaults
            .into_iter()
            .find(|(modality, _, _)| modality == Modality::Llm.as_str())
            .map(|(_, m, v)| (m, v))
            .or_else(|| {
                lib.all_models().ok()?.into_iter().find_map(|(m, _)| {
                    (m.modality == Modality::Llm).then_some(())?;
                    m.variants.iter().find_map(|v| {
                        let all = v.files.iter().all(|f| {
                            installed.iter().any(|i| {
                                i.model_id == m.id && i.variant_id == v.id && i.file_path == f.path
                            })
                        });
                        all.then(|| (m.id.clone(), v.id.clone()))
                    })
                })
            })
            .ok_or_else(|| CmdError::new("no_model", "no chat model installed"))?,
    };
    let (model, _) = lib
        .find(&model_id)
        .map_err(|_| CmdError::new("no_model", model_id.clone()))?;
    if model.modality != Modality::Llm {
        return Err(CmdError::new("no_model", "not a chat model"));
    }
    let variant = model
        .variant(&variant_id)
        .ok_or_else(|| CmdError::new("no_model", variant_id.clone()))?;
    let file = variant
        .files
        .iter()
        .find(|f| f.role.as_deref().is_none_or(|r| r == "model"))
        .ok_or_else(|| CmdError::new("no_model", "variant without a model file"))?;
    let local = installed
        .iter()
        .find(|i| i.model_id == model.id && i.variant_id == variant.id && i.file_path == file.path)
        .ok_or_else(|| {
            CmdError::new(
                "model_not_installed",
                format!("{} {}", model.name, variant.label),
            )
        })?;
    // Read separately: the settings guard must be dropped before `budget_of`
    // locks the settings again (a temporary in the struct literal would deadlock).
    let ctx_size = state.settings().context_size;
    let budget_mb = crate::models::budget_of(state).map(|b| b.main_mb);
    // Automatic windows grow only as far as free GPU memory allows; tight
    // cards get the compact KV cache (ADR 0021).
    let plan = lokalsi_engines::service::ctx_plan(budget_mb, variant.memory_estimate_mb);
    Ok(LoadRequest {
        model_id: model.id.clone(),
        variant_id: variant.id.clone(),
        name: format!("{} {}", model.name, variant.label),
        path: PathBuf::from(&local.local_path),
        memory_estimate_mb: variant.memory_estimate_mb,
        supports_tools: model.features.iter().any(|f| f == "tools"),
        ctx_size,
        ctx_limit: plan.ctx_limit,
        kv_compact: plan.kv_compact,
    })
}

#[derive(Debug, Serialize)]
pub struct ChatSent {
    session: Session,
    user: Message,
    assistant: Message,
}

/// Saves the user's message, creates the assistant message (the job) and
/// starts generation in the background; output arrives as `chat://delta`.
#[tauri::command]
pub fn chat_send(
    session_id: Option<i64>,
    text: String,
    model_id: Option<String>,
    variant_id: Option<String>,
    state: State<'_, AppState>,
) -> Res<ChatSent> {
    let text = text.trim().to_owned();
    tracing::debug!(
        command = "chat_send",
        ?session_id,
        chars = text.chars().count(),
        ?model_id
    );
    if text.is_empty() {
        return Err(CmdError::new("empty", "empty message"));
    }
    if state.llm.installed().ok().flatten().is_none() {
        return Err(CmdError::new(
            "engine_missing",
            "llama.cpp is not installed",
        ));
    }
    let model = resolve_model(&state, model_id.zip(variant_id))?;
    let chats = Chats::new(&state.db);
    let session = match session_id {
        Some(id) => chats
            .session(id)
            .map_err(db_err)?
            .ok_or_else(|| CmdError::new("not_found", "session"))?,
        None => chats.create_session(&title_from(&text)).map_err(db_err)?,
    };
    let user = chats
        .add_message(session.id, Role::User, &text, None, MessageStatus::Done)
        .map_err(db_err)?;
    let assistant = chats
        .add_message(
            session.id,
            Role::Assistant,
            "",
            Some((&model.model_id, &model.variant_id)),
            MessageStatus::Queued,
        )
        .map_err(db_err)?;
    let service = llm(&state);
    let id = assistant.id;
    tauri::async_runtime::spawn(
        async move { service.run_chat(id, model, ChatParams::default()).await },
    );
    let session = chats
        .session(session.id)
        .map_err(db_err)?
        .unwrap_or(session);
    Ok(ChatSent {
        session,
        user,
        assistant,
    })
}

/// Inspector defaults of a chat model variant (ADR 0036); Auto when none are saved.
#[tauri::command]
pub fn get_model_params(
    model_id: String,
    variant_id: String,
    state: State<'_, AppState>,
) -> Res<ModelParams> {
    tracing::debug!(command = "get_model_params", %model_id, %variant_id);
    ModelSettings::new(&state.db)
        .get(&model_id, &variant_id)
        .map_err(db_err)
}

/// Saves the Inspector defaults of a chat model variant; all Auto clears them.
#[tauri::command]
pub fn set_model_params(
    model_id: String,
    variant_id: String,
    params: ModelParams,
    state: State<'_, AppState>,
) -> Res<ModelParams> {
    tracing::debug!(command = "set_model_params", %model_id, %variant_id, ?params);
    let store = ModelSettings::new(&state.db);
    store
        .set(&model_id, &variant_id, &params)
        .map_err(|e| match e {
            ModelParamsError::OutOfRange(_) => CmdError::new("invalid_params", e.to_string()),
            ModelParamsError::Db(e) => db_err(e),
        })?;
    store.get(&model_id, &variant_id).map_err(db_err)
}

/// Number of layers of an installed chat model (GGUF `block_count`), the top
/// of the GPU offload slider (ADR 0036); `None` when the file does not say.
#[tauri::command]
pub fn get_model_layers(
    model_id: String,
    variant_id: String,
    state: State<'_, AppState>,
) -> Res<Option<u32>> {
    let model = resolve_model(&state, Some((model_id, variant_id)))?;
    let layers = lokalsi_core::gguf::block_count(&model.path)
        .inspect_err(
            |e| tracing::warn!(error = %e, path = %model.path.display(), "cannot read layer count"),
        )
        .ok()
        .flatten();
    tracing::debug!(command = "get_model_layers", model = %model.model_id, ?layers);
    Ok(layers)
}

#[tauri::command]
pub fn chat_cancel(message_id: i64, state: State<'_, AppState>) -> bool {
    tracing::debug!(command = "chat_cancel", message_id);
    state.llm.cancel_job(message_id)
}

/// Installs the chat engine in the background when it is missing
/// (plug and play, PRD §15). Safe to call often: it returns at once when the
/// engine is installed and waits for an installation already running.
pub fn ensure_chat_engine(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.llm.installed().ok().flatten().is_some() {
        return;
    }
    let service = llm(&state);
    service.set_engines_dir(engines_dir(&state));
    tauri::async_runtime::spawn(async move {
        if let Err(e) = service.ensure_installed().await {
            tracing::warn!(error = %e, "automatic engine installation failed");
        }
    });
}

/// Installs the engine for `modality` in the background when a model of that
/// kind is present (PRD §15). Chat and decision models use llama.cpp; the
/// edition's extension handles the other kinds.
pub fn ensure_engine_for(app: &AppHandle, modality: Modality) {
    match modality {
        Modality::Llm | Modality::Decision => ensure_chat_engine(app),
        other => app.state::<AppState>().ext.ensure_engine_for(app, other),
    }
}

/// True when at least one chat model is fully downloaded.
pub fn has_chat_model(state: &AppState) -> bool {
    resolve_model(state, None).is_ok()
}

/// Used by the first-run wizard: prepare the chat engine together with the models.
#[tauri::command]
pub fn ensure_engines(app: AppHandle) {
    tracing::debug!(command = "ensure_engines");
    ensure_chat_engine(&app);
}

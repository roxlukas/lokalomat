//! Lokalomat for Android (V9, phase 1 – client): pairs with a computer by QR
//! code and runs the computer's modes over the local network. Thin IPC layer;
//! pairing, TLS pinning and the API calls live in `lokalsi_api_gateway::client`.

mod device;
mod launcher;
mod phone_llm;
mod store;

use std::sync::Mutex;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use lokalsi_api_gateway::api::{
    DecisionJobRequest, DecisionQuestion, GalleryItem, ImageRequest, Info, JobView, ModelOption,
    MusicJobRequest, SpeechRequest, Transcription, Turn, Voice,
};
use lokalsi_api_gateway::client::{self, ClientError, Connection, Credentials};
use lokalsi_api_gateway::pairing::QrPayload;
use serde::Serialize;
use tauri::{Manager, State};

use store::Store;

/// Error for the UI: `code` selects the message (`mobile.errors.*`).
#[derive(Debug, Serialize)]
pub struct MobileError {
    code: String,
    message: String,
}

impl From<ClientError> for MobileError {
    fn from(e: ClientError) -> Self {
        MobileError {
            code: e.code(),
            message: e.to_string(),
        }
    }
}

fn err(code: &str, message: impl Into<String>) -> MobileError {
    MobileError {
        code: code.into(),
        message: message.into(),
    }
}

type Res<T> = Result<T, MobileError>;

struct AppState {
    store: Store,
    pairing: tokio::sync::Mutex<Option<tokio::task::JoinHandle<Result<Credentials, ClientError>>>>,
    /// The pairing `pair_wait` is waiting for: `pair_wait` takes the handle out of `pairing`,
    /// so cancelling needs this to reach the running task.
    pairing_abort: Mutex<Option<tokio::task::AbortHandle>>,
    creds: Mutex<Option<Credentials>>,
    /// Where models for the phone itself are kept (`lokalsi-phone-models`).
    phone_root: std::path::PathBuf,
}

impl AppState {
    fn connection(&self) -> Res<Connection> {
        let creds = self
            .creds
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or_else(|| err("not_paired", "not paired"))?;
        Ok(Connection::from_credentials(&creds)?)
    }
}

#[derive(Debug, Serialize)]
pub struct Status {
    paired: bool,
    /// The computer in use (its certificate fingerprint).
    id: Option<String>,
    computer_name: Option<String>,
    address: Option<String>,
}

/// A paired computer for the settings list and the source dropdown.
#[derive(Debug, Serialize)]
pub struct Computer {
    id: String,
    name: String,
    address: String,
}

#[derive(Debug, Serialize)]
pub struct Computers {
    /// The computer in use.
    active: Option<String>,
    list: Vec<Computer>,
}

fn status_of(state: &AppState) -> Status {
    let creds = state.creds.lock().unwrap_or_else(|p| p.into_inner());
    Status {
        paired: creds.is_some(),
        id: creds.as_ref().map(|c| c.fingerprint.clone()),
        computer_name: creds.as_ref().map(|c| c.computer_name.clone()),
        address: creds.as_ref().map(|c| format!("{}:{}", c.host, c.port)),
    }
}

/// Suggested phone name for pairing (owner-set name and model), if readable.
#[tauri::command]
fn device_name() -> Option<String> {
    device::read()
}

/// Launcher icon: gold when the paired computer runs Lokalomat Pro, else purple.
#[tauri::command]
fn launcher_icon(pro: bool) {
    launcher::set(pro);
}

#[tauri::command]
fn status(state: State<'_, AppState>) -> Status {
    status_of(&state)
}

/// Every paired computer and which one is in use.
#[tauri::command]
fn computers(state: State<'_, AppState>) -> Computers {
    let saved = state.store.load();
    Computers {
        active: saved.active_credentials().map(|c| c.fingerprint),
        list: saved
            .computers
            .iter()
            .map(|c| Computer {
                id: c.fingerprint.clone(),
                name: c.computer_name.clone(),
                address: format!("{}:{}", c.host, c.port),
            })
            .collect(),
    }
}

/// Switches to another paired computer.
#[tauri::command]
fn computer_select(id: String, state: State<'_, AppState>) -> Res<Status> {
    let creds = state
        .store
        .select(&id)
        .map_err(|e| err("storage", e))?
        .ok_or_else(|| err("not_paired", "unknown computer"))?;
    *state.creds.lock().unwrap_or_else(|p| p.into_inner()) = Some(creds);
    Ok(status_of(&state))
}

/// Forgets a computer on this phone ("Odłącz" on that computer revokes the key).
#[tauri::command]
fn computer_remove(id: String, state: State<'_, AppState>) -> Res<Status> {
    let active = state.store.remove(&id).map_err(|e| err("storage", e))?;
    *state.creds.lock().unwrap_or_else(|p| p.into_inner()) = active;
    tracing::info!("a computer was forgotten");
    Ok(status_of(&state))
}

#[derive(Debug, Serialize)]
pub struct PairStart {
    /// The code to compare with the computer's screen.
    code: String,
    computer_name: String,
}

/// After scanning: shows the comparison code and starts waiting for the
/// user's answer on the computer (finish with `pair_wait`).
#[tauri::command]
async fn pair_begin(
    link: String,
    device_name: String,
    state: State<'_, AppState>,
) -> Res<PairStart> {
    let payload = QrPayload::parse(&link).map_err(|e| err("bad_link", e.to_string()))?;
    let identity = state.store.identity().map_err(|e| err("storage", e))?;
    let code = client::code_for(&payload, &identity);
    tracing::info!(host = %payload.host, port = payload.port, "pairing started");
    let name = device_name.trim().chars().take(60).collect::<String>();
    let seed = *identity.seed();
    let task = tokio::spawn(async move {
        let identity = lokalsi_api_gateway::pairing::DeviceIdentity::from_seed(seed);
        client::pair(&payload, &identity, &name).await
    });
    abort_pairing(&state);
    let mut slot = state.pairing.lock().await;
    if let Some(old) = slot.replace(task) {
        old.abort();
    }
    Ok(PairStart {
        code,
        computer_name: QrPayload::parse(&link).map(|p| p.name).unwrap_or_default(),
    })
}

/// Waits until the computer confirms (or rejects) the pairing.
#[tauri::command]
async fn pair_wait(state: State<'_, AppState>) -> Res<Status> {
    let task = state
        .pairing
        .lock()
        .await
        .take()
        .ok_or_else(|| err("no_pairing", "no pairing in progress"))?;
    *state
        .pairing_abort
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(task.abort_handle());
    let result = task.await;
    *state
        .pairing_abort
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = None;
    let creds = result.map_err(|_| err("cancelled", "pairing cancelled"))??;
    state
        .store
        .add(creds.clone())
        .map_err(|e| err("storage", e))?;
    tracing::info!("paired with the computer");
    *state.creds.lock().unwrap_or_else(|p| p.into_inner()) = Some(creds);
    Ok(status_of(&state))
}

/// Stops the pairing that is waiting for the computer's answer, if any.
fn abort_pairing(state: &AppState) {
    if let Some(abort) = state
        .pairing_abort
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        abort.abort();
    }
}

#[tauri::command]
async fn pair_cancel(state: State<'_, AppState>) -> Res<()> {
    if let Some(t) = state.pairing.lock().await.take() {
        t.abort();
    }
    abort_pairing(&state);
    Ok(())
}

#[tauri::command]
async fn info(state: State<'_, AppState>) -> Res<Info> {
    Ok(state.connection()?.info().await?)
}

/// Models installed on the computer for a mode (`chat`, `stt`, `tts`, `image`).
#[tauri::command]
async fn models(mode: String, state: State<'_, AppState>) -> Res<Vec<ModelOption>> {
    Ok(state.connection()?.models(&mode).await?)
}

/// `model`: a [`ModelOption::id`] chosen on the phone; `None` = the computer's default.
#[tauri::command]
async fn chat(
    messages: Vec<Turn>,
    model: Option<String>,
    state: State<'_, AppState>,
) -> Res<String> {
    tracing::debug!(messages = messages.len(), model = ?model, "chat");
    Ok(state.connection()?.chat(messages, model.as_deref()).await?)
}

/// `audio`: the recording as base64; `ext`: its file type (`webm`, `m4a`, `wav`).
#[tauri::command]
async fn transcribe(
    audio: String,
    ext: String,
    language: Option<String>,
    model: Option<String>,
    state: State<'_, AppState>,
) -> Res<Transcription> {
    let bytes = STANDARD
        .decode(audio)
        .map_err(|e| err("bad_request", e.to_string()))?;
    tracing::debug!(bytes = bytes.len(), %ext, model = ?model, "transcribe");
    Ok(state
        .connection()?
        .transcribe(bytes, &ext, language.as_deref(), model.as_deref())
        .await?)
}

/// WAV as base64.
#[tauri::command]
async fn speak(
    text: String,
    voice_id: Option<i64>,
    language: Option<String>,
    model: Option<String>,
    state: State<'_, AppState>,
) -> Res<String> {
    let wav = state
        .connection()?
        .speak(&SpeechRequest {
            input: text,
            voice_id,
            language,
            model,
            ..Default::default()
        })
        .await?;
    Ok(STANDARD.encode(wav))
}

/// PNG images as base64.
#[tauri::command]
async fn image(
    prompt: String,
    aspect: Option<String>,
    model: Option<String>,
    state: State<'_, AppState>,
) -> Res<Vec<String>> {
    Ok(state
        .connection()?
        .image(&ImageRequest {
            prompt,
            aspect,
            model,
            ..Default::default()
        })
        .await?)
}

/// Starts an image on the computer as a job (ADR 0027, settings ADR 0037); poll it with `job`.
#[tauri::command]
async fn start_image(req: ImageRequest, state: State<'_, AppState>) -> Res<JobView> {
    tracing::debug!(chars = req.prompt.chars().count(), count = ?req.count, model = ?req.model, "start image");
    Ok(state.connection()?.start_image(&req).await?)
}

/// Starts a song (style, lyrics, language, length in seconds, seed).
#[tauri::command]
async fn start_music(req: MusicJobRequest, state: State<'_, AppState>) -> Res<JobView> {
    tracing::debug!(caption_chars = req.caption.chars().count(), lyrics_chars = req.lyrics.chars().count(), duration = ?req.duration, model = ?req.model, "start music");
    Ok(state.connection()?.start_music(&req).await?)
}

/// Starts speech as a job (ADR 0037); the WAV comes with `gallery_file`.
#[tauri::command]
async fn start_speech(req: SpeechRequest, state: State<'_, AppState>) -> Res<JobView> {
    tracing::debug!(chars = req.input.chars().count(), voice = ?req.voice_id, model = ?req.model, "start speech");
    Ok(state.connection()?.start_speech(&req).await?)
}

/// Deletes a result and its files on the computer (ADR 0037).
#[tauri::command]
async fn delete_gallery_item(job_id: i64, state: State<'_, AppState>) -> Res<()> {
    tracing::info!(job_id, "delete gallery item");
    Ok(state.connection()?.delete_gallery_item(job_id).await?)
}

/// Asks typed questions about a text (Expert mode on the computer).
#[tauri::command]
async fn start_decision(
    text: String,
    questions: Vec<DecisionQuestion>,
    model: Option<String>,
    state: State<'_, AppState>,
) -> Res<JobView> {
    tracing::debug!(chars = text.chars().count(), questions = questions.len(), model = ?model, "start decision");
    Ok(state
        .connection()?
        .start_decision(&DecisionJobRequest {
            state: text,
            questions,
            model,
        })
        .await?)
}

/// The state of a job this phone started.
#[tauri::command]
async fn job(job_id: i64, state: State<'_, AppState>) -> Res<JobView> {
    Ok(state.connection()?.job(job_id).await?)
}

#[tauri::command]
async fn cancel_job(job_id: i64, state: State<'_, AppState>) -> Res<JobView> {
    tracing::info!(job_id, "cancel job");
    Ok(state.connection()?.cancel_job(job_id).await?)
}

#[tauri::command]
async fn voices(state: State<'_, AppState>) -> Res<Vec<Voice>> {
    Ok(state.connection()?.voices().await?)
}

/// Recent results, newest first (`limit` up to 100, default 60).
#[tauri::command]
async fn gallery(limit: Option<u32>, state: State<'_, AppState>) -> Res<Vec<GalleryItem>> {
    let limit = limit.unwrap_or(60).clamp(1, 100);
    Ok(state.connection()?.gallery(limit).await?)
}

#[derive(Debug, Serialize)]
pub struct FileData {
    content_type: String,
    data: String,
}

/// Small JPEG of an image result. Fetched here, not by the web view, so the
/// device key never reaches page code and no file URL exists to leak.
#[tauri::command]
async fn gallery_thumb(job_id: i64, index: usize, state: State<'_, AppState>) -> Res<FileData> {
    let reply = state.connection()?.gallery_thumb(job_id, index).await?;
    Ok(FileData {
        content_type: reply.content_type,
        data: STANDARD.encode(reply.body),
    })
}

#[tauri::command]
async fn gallery_file(job_id: i64, index: usize, state: State<'_, AppState>) -> Res<FileData> {
    let reply = state.connection()?.gallery_file(job_id, index).await?;
    Ok(FileData {
        content_type: reply.content_type,
        data: STANDARD.encode(reply.body),
    })
}

/// "Zapisz": downloads a result file and writes it where the user picks in
/// the system save dialog. `false` when the user closed the dialog.
#[tauri::command]
async fn save_file(
    job_id: i64,
    index: usize,
    file_name: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Res<bool> {
    use std::io::Write as _;
    use tauri_plugin_dialog::DialogExt as _;
    use tauri_plugin_fs::{FsExt as _, OpenOptions};

    let reply = state.connection()?.gallery_file(job_id, index).await?;
    let name: String = file_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(80)
        .collect();
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_owned());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file().set_file_name(&name);
    if let Some(ext) = &ext {
        dialog = dialog.add_filter(ext, &[ext.as_str()]);
    }
    dialog.save_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(path) = rx.await.ok().flatten() else {
        tracing::debug!(job_id, "save cancelled");
        return Ok(false);
    };
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    let written = app
        .fs()
        .open(path, opts)
        .and_then(|mut f| f.write_all(&reply.body));
    if let Err(e) = written {
        tracing::warn!(job_id, error = %e, "cannot save a result");
        return Err(err("save_failed", e.to_string()));
    }
    tracing::info!(
        job_id,
        bytes = reply.body.len(),
        "result saved on the phone"
    );
    Ok(true)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,lokalomat_mobile_lib=debug".into()),
        )
        .try_init();
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init());
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(tauri_plugin_localllm::init());
    let result = builder
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let store = Store::new(&dir);
            let creds = store.active_credentials();
            app.manage(AppState {
                store,
                pairing: tokio::sync::Mutex::new(None),
                pairing_abort: Mutex::new(None),
                creds: Mutex::new(creds),
                phone_root: dir.join("phone-models"),
            });
            app.manage(phone_llm::Downloads::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            phone_llm::phone_models,
            phone_llm::phone_model_download,
            phone_llm::phone_model_cancel,
            phone_llm::phone_model_delete,
            phone_llm::phone_model_path,
            device_name,
            launcher_icon,
            computers,
            computer_select,
            computer_remove,
            status,
            pair_begin,
            pair_wait,
            pair_cancel,
            info,
            chat,
            transcribe,
            speak,
            image,
            start_image,
            start_music,
            start_speech,
            delete_gallery_item,
            start_decision,
            job,
            cancel_job,
            save_file,
            voices,
            gallery,
            gallery_thumb,
            gallery_file,
            models,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!(error = %e, "Lokalomat mobile failed");
    }
}

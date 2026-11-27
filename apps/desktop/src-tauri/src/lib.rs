//! Tauri shell: wires `lokalsi-core` into IPC commands. Keep logic out of here.

pub mod chat;
pub mod commands;
mod decision;
pub mod events;
pub mod ext;
pub mod jobs;
pub mod models;
mod network;
pub mod resolve;
pub mod server;
mod updates;
pub mod vcrt;

pub use ext::{ExtContext, Extension, ExtensionFactory, NoExtension};

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use lokalsi_core::benchmark::Benchmark;
use lokalsi_core::catalog::{Catalog, Modality};
use lokalsi_core::db::Db;
use lokalsi_core::downloads::{DownloadConfig, DownloadManager};
use lokalsi_core::logging::{self, LogHandle, LogMode};
use lokalsi_core::settings::{self, AppSettings};
use lokalsi_core::{hf, paths, secrets};
use lokalsi_engines::manifest::EngineManifest;
use lokalsi_engines::modes::ModeRunner;
use lokalsi_engines::queue::JobQueue;
use lokalsi_engines::scheduler::Scheduler;
use lokalsi_engines::service::EngineService;
use lokalsi_hwprobe::HardwareProfile;
use tauri::{Manager, RunEvent};

pub struct AppState {
    pub home: PathBuf,
    pub settings_path: PathBuf,
    pub settings: Mutex<AppSettings>,
    pub log: LogHandle,
    /// Log mode forced by `--verbose` / `STUDIO_LOKALSI_LOG` for this run.
    pub log_mode_overridden: bool,
    pub db: Arc<Db>,
    pub catalog: Catalog,
    pub downloads: DownloadManager,
    pub hardware: Mutex<Option<HardwareProfile>>,
    /// Serialises hardware probes so concurrent requests share one result.
    pub probe_lock: tauri::async_runtime::Mutex<()>,
    pub http: hf::Client,
    /// AI Transformation benchmark; `None` until loaded or when the site did not respond.
    pub benchmark: Mutex<Option<Benchmark>>,
    /// llama.cpp: installation, `llama-server` supervision, chat jobs.
    pub llm: Arc<EngineService>,
    /// VRAM scheduler: memory budget, resident models, running jobs (M3).
    pub scheduler: Arc<Scheduler>,
    /// The job queue: decisions run on it, and so do the Pro media jobs.
    pub queue: Arc<JobQueue>,
    pub events: Arc<events::TauriEvents>,
    /// Model choice for pipelines and the phone (same rules as the Workshop).
    pub resolver: Arc<resolve::DesktopResolver>,
    /// Runs one mode to completion (chat and decisions; the Pro edition adds the media modes).
    pub modes: Arc<ModeRunner>,
    /// What the edition adds (ADR 0035).
    pub ext: Arc<dyn Extension>,
    /// LAN gateway for paired phones; `None` while sharing is off.
    pub gateway: tauri::async_runtime::Mutex<Option<lokalsi_api_gateway::server::Gateway>>,
    pub gateway_error: Mutex<Option<String>>,
    /// Local OpenAI-compatible server for agents (ADR 0025); `None` while off.
    pub server: tauri::async_runtime::Mutex<Option<lokalsi_api_gateway::openai::OpenAiServer>>,
    pub server_error: Mutex<Option<String>>,
}

impl AppState {
    pub fn settings(&self) -> MutexGuard<'_, AppSettings> {
        self.settings.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn benchmark(&self) -> MutexGuard<'_, Option<Benchmark>> {
        self.benchmark.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn hardware(&self) -> Option<HardwareProfile> {
        self.hardware
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Download settings derived from app settings and the credential store.
    pub fn download_config(&self) -> DownloadConfig {
        download_config(&self.settings())
    }
}

fn download_config(settings: &AppSettings) -> DownloadConfig {
    let hf_token = secrets::hf_token().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "cannot read the Hugging Face token");
        None
    });
    DownloadConfig {
        bandwidth_limit_kbps: settings.bandwidth_limit_kbps,
        hf_token,
        hf_cache: hf::cache_dir(),
    }
}

fn fatal(what: &str, error: &dyn std::fmt::Display) -> ! {
    tracing::error!(error = %error, "{what}");
    eprintln!("Lokalomat AI Studio: {what}: {error}");
    std::process::exit(1);
}

/// The IPC commands of the free app.
fn free_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::get_settings,
        commands::update_settings,
        commands::get_app_info,
        commands::get_edition,
        updates::check_update,
        updates::install_update,
        commands::log_from_ui,
        commands::get_log_tail,
        commands::open_log_dir,
        commands::export_diagnostics,
        models::get_hardware,
        models::get_storage_options,
        models::set_data_dir,
        models::open_data_dir,
        models::list_models,
        models::get_starter_pack,
        models::download_variant,
        models::delete_variant,
        models::set_default_model,
        models::add_custom_model_file,
        models::add_custom_model_hf,
        models::remove_custom_model,
        models::list_downloads,
        models::pause_download,
        models::resume_download,
        models::cancel_download,
        models::cancel_variant_download,
        models::move_download,
        models::clear_finished_downloads,
        models::get_hf_token_status,
        models::set_hf_token,
        models::get_benchmark_meta,
        chat::get_engines,
        chat::install_engine,
        chat::cancel_engine_install,
        chat::uninstall_engine,
        chat::stop_engine,
        chat::restart_engine,
        chat::get_engine_log,
        chat::list_sessions,
        chat::get_messages,
        chat::rename_session,
        chat::pin_session,
        chat::delete_session,
        chat::chat_send,
        chat::chat_cancel,
        chat::get_model_params,
        chat::set_model_params,
        chat::get_model_layers,
        chat::ensure_engines,
        decision::decide,
        network::get_network,
        network::set_lan_sharing,
        network::start_pairing,
        network::confirm_pairing,
        network::cancel_pairing,
        network::unpair_device,
        server::get_server,
        server::set_server,
        server::create_api_key,
        server::api_key_secret,
        server::revoke_api_key,
        server::server_log,
        jobs::get_memory,
        jobs::release_memory,
        jobs::list_jobs,
        jobs::cancel_job,
        jobs::delete_jobs,
        chat::repair_engine,
    ]
}

/// Runs the app with the Tauri configuration and the extension of an edition
/// (ADR 0035): the free app passes its own context and no extension, the Pro app
/// its context and the Pro extension.
pub fn run_with(context: tauri::Context<tauri::Wry>, factory: Option<ExtensionFactory>) {
    let home = paths::app_home();
    // First, while only this thread runs: the C++ runtime engines need on
    // Windows (ADR 0043).
    let vc_runtime = vcrt::prepare(&home);
    let settings_path = paths::settings_path(&home);
    let loaded = settings::load(&settings_path);
    let env_mode = std::env::var(logging::ENV_LOG_MODE).ok();
    let (mode, overridden) = logging::effective_mode(
        loaded.settings.log_mode,
        std::env::args(),
        env_mode.as_deref(),
    );

    let log = match logging::init(&paths::logs_dir(&home), mode) {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("Lokalomat AI Studio: cannot initialise logging: {e}");
            std::process::exit(1);
        }
    };
    logging::install_panic_hook();
    vcrt::log(&vc_runtime);
    logging::set_log_prompts(loaded.settings.log_prompts);

    let sys = lokalsi_hwprobe::basic_info();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = sys.os,
        arch = sys.arch,
        log_mode = ?mode,
        log_mode_overridden = overridden,
        "Lokalomat AI Studio starting"
    );
    if let Some(warning) = &loaded.warning {
        tracing::warn!("{warning}");
    }
    tracing::debug!(home = %home.display(), settings = ?loaded.settings, "startup state");
    if mode == LogMode::Verbose && !loaded.settings.log_prompts {
        tracing::debug!("verbose mode: prompt contents stay hidden (log_prompts = false)");
    }

    let db = match Db::open(&paths::db_path(&home)) {
        Ok(db) => Arc::new(db),
        Err(e) => fatal("cannot open the database", &e),
    };
    let mut catalog = match Catalog::embedded() {
        Ok(c) => c,
        Err(e) => fatal("the built-in model catalog is invalid", &e),
    };
    let hardware = db.load_hardware().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "cannot read the saved hardware profile");
        None
    });
    let events = Arc::new(events::TauriEvents::default());
    let downloads = match DownloadManager::new(
        db.clone(),
        events.clone(),
        download_config(&loaded.settings),
    ) {
        Ok(d) => d,
        Err(e) => fatal("cannot start the download manager", &e),
    };

    let data_root = loaded
        .settings
        .data_dir
        .clone()
        .unwrap_or_else(|| home.clone());
    let engine = |manifest: EngineManifest| {
        EngineService::new(
            manifest,
            db.clone(),
            hf::client(),
            lokalsi_core::library::platform_key(),
            data_root.join("engines"),
            &paths::logs_dir(&home),
            events.clone(),
        )
    };
    let llm = engine(EngineManifest::llama_cpp());
    let scheduler = Scheduler::new();
    scheduler.set_keep_llm(loaded.settings.keep_llm_loaded);
    llm.set_scheduler(Arc::clone(&scheduler));
    let queue = JobQueue::new(
        db.clone(),
        Arc::clone(&scheduler),
        events.clone(),
        data_root.clone(),
        paths::logs_dir(&home),
    );
    {
        let events = events.clone();
        scheduler.set_on_change(Arc::new(move |status| events.memory_changed(status)));
    }

    let resolver = Arc::new(resolve::DesktopResolver::default());
    let modes = ModeRunner::new(
        db.clone(),
        Arc::clone(&queue),
        Arc::clone(&llm),
        resolver.clone(),
    );
    let ext: Arc<dyn Extension> = match factory {
        Some(build) => build(&ExtContext {
            db: db.clone(),
            events: events.clone(),
            llm: Arc::clone(&llm),
            scheduler: Arc::clone(&scheduler),
            queue: Arc::clone(&queue),
            modes: Arc::clone(&modes),
            resolver: resolver.clone(),
            home: home.clone(),
            data_root: data_root.clone(),
            logs_dir: paths::logs_dir(&home),
        }),
        None => Arc::new(NoExtension),
    };

    // The shipped catalog is the large list; each edition offers the kinds of models of its modes.
    let listed = catalog.models.len();
    catalog.retain_modalities(ext.modalities());
    tracing::info!(
        models = catalog.models.len(),
        of = listed,
        edition = ?ext.edition(),
        "model catalog loaded"
    );

    let state = AppState {
        home,
        settings_path,
        settings: Mutex::new(loaded.settings),
        log,
        log_mode_overridden: overridden,
        db,
        catalog,
        downloads: downloads.clone(),
        hardware: Mutex::new(hardware),
        probe_lock: tauri::async_runtime::Mutex::new(()),
        http: hf::client(),
        benchmark: Mutex::new(None),
        llm,
        scheduler,
        queue,
        events: events.clone(),
        resolver: resolver.clone(),
        modes,
        ext: Arc::clone(&ext),
        gateway: tauri::async_runtime::Mutex::new(None),
        gateway_error: Mutex::new(None),
        server: tauri::async_runtime::Mutex::new(None),
        server_error: Mutex::new(None),
    };

    let ext_for_handler = Arc::clone(&ext);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .manage(updates::PendingUpdate::default())
        .setup(move |app| {
            events.attach(app.handle().clone());
            resolver.attach(app.handle().clone());
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { network::start_gateway(&handle).await });
            }
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { server::start_server(&handle).await });
            }
            updates::spawn_startup_check(app.handle());
            tauri::async_runtime::spawn(downloads.clone().run());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { models::load_benchmark(&handle).await });
            let state = app.state::<AppState>();
            models::apply_hardware(&state);
            if chat::has_chat_model(&state) {
                chat::ensure_chat_engine(app.handle());
            }
            // A decision model also needs llama.cpp (its second slot).
            if resolve::has_model(&state, Modality::Decision) {
                chat::ensure_chat_engine(app.handle());
            }
            // The edition's own start-up work (Pro: result folders, engines, MCP).
            state.ext.setup(app.handle());
            if state.hardware().is_none() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = models::probe_hardware(&handle, false).await {
                        tracing::warn!(error = %e, "initial hardware probe failed");
                    }
                });
            }
            Ok(())
        })
        .invoke_handler({
            let free = free_handler();
            move |invoke| {
                // Commands of the edition (Pro) go to its extension.
                if ext_for_handler.handles(invoke.message.command()) {
                    ext_for_handler.invoke(invoke)
                } else {
                    free(invoke)
                }
            }
        })
        .build(context);

    let app = match app {
        Ok(app) => app,
        Err(e) => fatal("Tauri runtime failed", &e),
    };
    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            let state = handle.state::<AppState>();
            state.downloads.shutdown();
            // Never leave a llama-server process behind; job processes die
            // with their tasks (`kill_on_drop`).
            tauri::async_runtime::block_on(state.llm.stop_server());
            tauri::async_runtime::block_on(state.modes.decider().stop_server());
            tauri::async_runtime::block_on(network::stop_gateway(&state));
            tauri::async_runtime::block_on(server::stop_server(&state));
            // What the edition started (Pro: speech workers, MCP servers, agent helpers).
            state.ext.exit();
            tracing::info!("Lokalomat AI Studio stopped");
        }
    });
}

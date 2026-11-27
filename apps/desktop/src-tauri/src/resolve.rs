//! Which downloaded model a mode uses: the requested one, the default, or the
//! first fully installed (shared by the Workshop commands, the local server and
//! the phone). Free edition code; the Pro edition adds the media modes on top.

use std::path::PathBuf;
use std::sync::OnceLock;

use lokalsi_core::catalog::{Modality, Model, Variant};
use lokalsi_core::library::Library;
use lokalsi_core::pipelines;
use lokalsi_engines::modes::{ModeError, ModelResolver, Resolved as ResolvedModel, has_cloning};
use tauri::{AppHandle, Manager, State};

use crate::AppState;
use crate::models::CmdError;

type Res<T> = Result<T, CmdError>;

fn err(e: impl std::fmt::Display) -> CmdError {
    CmdError::new("error", e.to_string())
}

/// Model, variant and `(role, local path)` of every file.
pub type Resolved = (Model, Variant, Vec<(String, PathBuf)>);

/// A downloaded model of `modality`: the requested one, the default, or the
/// first fully installed. Returns the model, variant and `(role, local path)`.
pub fn resolve(
    state: &AppState,
    modality: Modality,
    requested: Option<(String, String)>,
) -> Res<Resolved> {
    let lib = Library::new(&state.catalog, &state.db);
    let installed = state.db.installed_files().map_err(err)?;
    let local = |m: &Model, v: &Variant| -> Option<Vec<(String, PathBuf)>> {
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
    };
    let choice = match requested {
        Some(m) => Some(m),
        None => state
            .db
            .default_models()
            .map_err(err)?
            .into_iter()
            .find(|(modality_key, _, _)| modality_key == modality.as_str())
            .map(|(_, m, v)| (m, v)),
    };
    if let Some((model_id, variant_id)) = choice {
        let (model, _) = lib
            .find(&model_id)
            .map_err(|_| CmdError::new("no_model", model_id.clone()))?;
        let variant = model
            .variant(&variant_id)
            .cloned()
            .ok_or_else(|| CmdError::new("no_model", variant_id.clone()))?;
        if model.modality != modality {
            return Err(CmdError::new("no_media_model", "wrong kind of model"));
        }
        let files = local(&model, &variant).ok_or_else(|| {
            CmdError::new(
                "model_not_installed",
                format!("{} {}", model.name, variant.label),
            )
        })?;
        return Ok((model, variant, files));
    }
    lib.all_models()
        .map_err(err)?
        .into_iter()
        .filter(|(m, _)| m.modality == modality)
        .find_map(|(m, _)| {
            m.variants.iter().find_map(|v| {
                let files = local(&m, v)?;
                Some((m.clone(), v.clone(), files))
            })
        })
        .ok_or_else(|| CmdError::new("no_media_model", "no model installed"))
}

/// A downloaded TTS model: the requested one, else (for a cloned voice) the
/// first that can clone, else the default/first TTS model.
pub fn resolve_tts(
    state: &AppState,
    requested: Option<(String, String)>,
    cloning: bool,
) -> Res<Resolved> {
    let picked = resolve(state, Modality::Tts, requested.clone())?;
    if !cloning || has_cloning(&picked.0) {
        return Ok(picked);
    }
    if requested.is_some() {
        return Err(CmdError::new("voice_unsupported", picked.0.name));
    }
    let lib = Library::new(&state.catalog, &state.db);
    let cloners: Vec<(String, String)> = lib
        .all_models()
        .map_err(err)?
        .into_iter()
        .filter(|(m, _)| m.modality == Modality::Tts && has_cloning(m))
        .flat_map(|(m, _)| {
            m.variants
                .iter()
                .map(|v| (m.id.clone(), v.id.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    cloners
        .into_iter()
        .find_map(|choice| resolve(state, Modality::Tts, Some(choice)).ok())
        .ok_or_else(|| CmdError::new("no_cloning_model", "no voice cloning model installed"))
}

/// Resolves models like the Workshop commands do (default model, else the
/// first installed one), for pipelines and the phone.
#[derive(Default)]
pub struct DesktopResolver {
    app: OnceLock<AppHandle>,
}

impl DesktopResolver {
    pub fn attach(&self, app: AppHandle) {
        let _ = self.app.set(app);
    }

    fn state(&self) -> Option<State<'_, AppState>> {
        self.app.get().map(|a| a.state::<AppState>())
    }
}

impl ModelResolver for DesktopResolver {
    fn resolve(
        &self,
        modality: Modality,
        requested: Option<&pipelines::ModelRef>,
        cloning: bool,
    ) -> Result<ResolvedModel, ModeError> {
        let state = self
            .state()
            .ok_or_else(|| ModeError::new("error", "app not ready"))?;
        let req = requested.map(|r| (r.model_id.clone(), r.variant_id.clone()));
        let picked = if modality == Modality::Tts {
            resolve_tts(&state, req, cloning)
        } else {
            resolve(&state, modality, req)
        };
        picked
            .map(|(model, variant, files)| ResolvedModel {
                model,
                variant,
                files,
            })
            .map_err(|e| ModeError::new(e.code(), e.message()))
    }

    fn installed(&self, modality: Modality) -> Vec<ResolvedModel> {
        let Some(state) = self.state() else {
            return Vec::new();
        };
        let Ok(all) = lokalsi_core::library::Library::new(&state.catalog, &state.db).all_models()
        else {
            return Vec::new();
        };
        all.iter()
            .filter(|(m, _)| m.modality == modality)
            .flat_map(|(m, _)| {
                m.variants.iter().filter_map(|v| {
                    resolve(&state, modality, Some((m.id.clone(), v.id.clone())))
                        .ok()
                        .map(|(model, variant, files)| ResolvedModel {
                            model,
                            variant,
                            files,
                        })
                })
            })
            .collect()
    }

    fn context_size(&self) -> Option<u32> {
        self.state().and_then(|s| s.settings().context_size)
    }

    fn server_context_size(&self) -> Option<u32> {
        self.state().and_then(|s| s.settings().server_context_size)
    }

    fn budget_mb(&self) -> Option<u64> {
        self.state()
            .and_then(|s| crate::models::budget_of(&s).map(|b| b.main_mb))
    }
}

/// True when a model of `modality` is fully downloaded.
pub fn has_model(state: &AppState, modality: Modality) -> bool {
    resolve(state, modality, None).is_ok()
}

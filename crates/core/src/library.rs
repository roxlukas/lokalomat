//! The user's model library: catalog entries joined with what is installed,
//! what is downloading and how well each variant fits the hardware. Also the
//! starter pack for the first-run wizard and custom GGUF models (PRD §6.2).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use lokalsi_hwprobe::{FitReport, MemoryBudget, Speed, SpeedReport, fit, llm_fit, music_fit};
use serde::{Deserialize, Serialize};

use crate::catalog::{
    Catalog, Commercial, License, Modality, Model, ModelFile, Variant, estimate_memory_mb,
};
use crate::db::{CustomModelRow, Db, DbError, FileSource, InstalledFile, now_secs};
use crate::downloads::{DownloadError, DownloadManager, DownloadStatus, PlannedFile, VariantPlan};
use crate::hf;

pub const CUSTOM_PREFIX: &str = "custom-";
pub const CUSTOM_VARIANT: &str = "file";

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Hf(#[from] hf::HfError),
    #[error("unknown model `{0}`")]
    UnknownModel(String),
    #[error("unknown variant `{1}` of `{0}`")]
    UnknownVariant(String, String),
    #[error("choose a data folder first")]
    NoDataDir,
    #[error("not a GGUF file: {0}")]
    NotGguf(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// `windows-x64`, `macos-arm64`, … as used in the catalog `platforms` field.
pub fn platform_key() -> String {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

pub fn models_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("models")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariantState {
    NotInstalled,
    Queued,
    Downloading,
    Paused,
    Failed,
    /// Some files are on disk, others are missing (e.g. cancelled downloads).
    Partial,
    Installed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantView {
    pub id: String,
    pub label: String,
    pub recommended: bool,
    pub size: u64,
    pub memory_estimate_mb: u64,
    pub files: usize,
    /// `None` until the hardware has been probed.
    pub fit: Option<FitReport>,
    /// Estimated speed on this computer (ADR 0016); `None` until probed.
    #[serde(default)]
    pub speed: Option<SpeedReport>,
    pub supported_here: bool,
    pub state: VariantState,
    /// Bytes on disk/downloaded so far for this variant.
    pub downloaded: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelView {
    #[serde(flatten)]
    pub model: Model,
    pub custom: bool,
    pub variants: Vec<VariantView>,
    /// Variant set as default for this modality, if it belongs to this model.
    pub default_variant: Option<String>,
    /// AI Transformation benchmark scores, when the model was tested.
    pub scores: Option<crate::benchmark::ModelScores>,
}

impl ModelView {
    pub fn variant(&self, id: &str) -> Option<&VariantView> {
        self.variants.iter().find(|v| v.id == id)
    }
}

pub struct Library<'a> {
    pub catalog: &'a Catalog,
    pub db: &'a Db,
}

impl<'a> Library<'a> {
    pub fn new(catalog: &'a Catalog, db: &'a Db) -> Self {
        Library { catalog, db }
    }

    /// Catalog plus custom models.
    pub fn all_models(&self) -> Result<Vec<(Model, bool)>, LibraryError> {
        let mut out: Vec<(Model, bool)> = self
            .catalog
            .models
            .iter()
            .cloned()
            .map(|m| (m, false))
            .collect();
        for row in self.db.custom_models()? {
            out.push((custom_model(&row), true));
        }
        Ok(out)
    }

    pub fn find(&self, model_id: &str) -> Result<(Model, bool), LibraryError> {
        self.all_models()?
            .into_iter()
            .find(|(m, _)| m.id == model_id)
            .ok_or_else(|| LibraryError::UnknownModel(model_id.to_owned()))
    }

    pub fn list(
        &self,
        budget: Option<&MemoryBudget>,
        downloads: &[crate::downloads::DownloadItem],
        benchmark: Option<&crate::benchmark::Benchmark>,
    ) -> Result<Vec<ModelView>, LibraryError> {
        let installed = self.db.installed_files()?;
        let defaults = self.db.default_models()?;
        let platform = platform_key();

        let mut by_variant: HashMap<(&str, &str), Vec<&InstalledFile>> = HashMap::new();
        for f in &installed {
            by_variant
                .entry((f.model_id.as_str(), f.variant_id.as_str()))
                .or_default()
                .push(f);
        }

        let mut views = Vec::new();
        for (model, custom) in self.all_models()? {
            let default_variant = defaults
                .iter()
                .find(|(modality, m, _)| modality == model.modality.as_str() && *m == model.id)
                .map(|(_, _, v)| v.clone());
            let variants = model
                .variants
                .iter()
                .map(|v| {
                    let files = by_variant
                        .get(&(model.id.as_str(), v.id.as_str()))
                        .cloned()
                        .unwrap_or_default();
                    let pending: Vec<_> = downloads
                        .iter()
                        .filter(|d| {
                            d.model_id == model.id && d.variant_id == v.id && d.status.is_pending()
                        })
                        .collect();
                    let all_installed = v
                        .files
                        .iter()
                        .all(|f| files.iter().any(|i| i.file_path == f.path));
                    let any_installed = v
                        .files
                        .iter()
                        .any(|f| files.iter().any(|i| i.file_path == f.path));
                    let state = variant_state(all_installed, any_installed, &pending);
                    let downloaded = files.iter().map(|f| f.size).sum::<u64>()
                        + pending.iter().map(|d| d.downloaded).sum::<u64>();
                    let fit = budget.map(|b| match model.modality {
                        Modality::Llm | Modality::Decision => llm_fit(v.memory_estimate_mb, b),
                        Modality::Music => music_fit(v.memory_estimate_mb, b),
                        _ => fit(v.memory_estimate_mb, b),
                    });
                    let speed = budget
                        .zip(fit.as_ref())
                        .and_then(|(b, f)| crate::speed::speed_of(&model, v, b, f));
                    VariantView {
                        id: v.id.clone(),
                        label: v.label.clone(),
                        recommended: v.recommended,
                        size: v.total_size(),
                        memory_estimate_mb: v.memory_estimate_mb,
                        files: v.files.len(),
                        fit,
                        speed,
                        supported_here: v.platforms.is_empty() || v.platforms.contains(&platform),
                        state,
                        downloaded,
                    }
                })
                .collect();
            let scores = benchmark.and_then(|b| b.scores_for(&model));
            views.push(ModelView {
                model,
                custom,
                variants,
                default_variant,
                scores,
            });
        }
        Ok(views)
    }

    /// Files to download for `model_id/variant_id` into `data_dir`.
    pub fn plan(
        &self,
        model_id: &str,
        variant_id: &str,
        data_dir: &Path,
    ) -> Result<VariantPlan, LibraryError> {
        let (model, _) = self.find(model_id)?;
        let variant = model
            .variant(variant_id)
            .ok_or_else(|| LibraryError::UnknownVariant(model_id.into(), variant_id.into()))?;
        Ok(VariantPlan {
            model_id: model.id.clone(),
            variant_id: variant.id.clone(),
            dest_dir: models_dir(data_dir)
                .join(safe_segment(&model.id))
                .join(safe_segment(&variant.id)),
            files: variant
                .files
                .iter()
                .map(|f| PlannedFile {
                    repo: f.repo.clone(),
                    path: f.path.clone(),
                    url: f.url(),
                    size: f.size,
                    sha256: f.has_valid_sha256().then(|| f.sha256.clone()),
                    gated: model.license.gated,
                    local: f.local_path().to_owned(),
                })
                .collect(),
        })
    }

    /// Removes an installed variant. Only files the app downloaded into
    /// `data_dir` are deleted; HF-cache and user files are left alone.
    pub fn delete_variant(
        &self,
        downloads: &DownloadManager,
        model_id: &str,
        variant_id: &str,
        data_dir: Option<&Path>,
    ) -> Result<(), LibraryError> {
        downloads.cancel_variant(model_id, variant_id)?;
        let files = self.db.installed_files_for(model_id, variant_id)?;
        let root = data_dir.map(models_dir);
        for f in &files {
            let path = PathBuf::from(&f.local_path);
            let inside_data_dir = root.as_ref().is_some_and(|r| path.starts_with(r));
            if f.source == FileSource::Download && inside_data_dir {
                match std::fs::remove_file(&path) {
                    Ok(()) => tracing::info!(path = %path.display(), "model file deleted"),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                // Empty folders go too, up to the models folder (folder trees, ADR 0022).
                let mut dir = path.parent();
                while let Some(d) = dir {
                    if root.as_ref().is_some_and(|r| d == r.as_path())
                        || std::fs::remove_dir(d).is_err()
                    {
                        break;
                    }
                    dir = d.parent();
                }
            } else {
                tracing::info!(path = %path.display(), source = ?f.source, "model file kept (not owned by the app)");
            }
        }
        // ACE-Step writes its model code next to the weights (ADR 0022): the
        // variant folder belongs to the app, so whatever is left in it goes too.
        if let Some(root) = &root {
            let vdir = root
                .join(safe_segment(model_id))
                .join(safe_segment(variant_id));
            let owned = files.iter().all(|f| {
                f.source == FileSource::Download && Path::new(&f.local_path).starts_with(&vdir)
            });
            if owned && !files.is_empty() && vdir.is_dir() {
                match std::fs::remove_dir_all(&vdir) {
                    Ok(()) => tracing::info!(path = %vdir.display(), "variant folder removed"),
                    Err(e) => {
                        tracing::warn!(path = %vdir.display(), error = %e, "variant folder not removed")
                    }
                }
                if let Some(parent) = vdir.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
            }
        }
        self.db.remove_installed_variant(model_id, variant_id)?;
        tracing::info!(model = %model_id, variant = %variant_id, "model variant removed");
        Ok(())
    }

    pub fn set_default(&self, model_id: &str, variant_id: &str) -> Result<(), LibraryError> {
        let (model, _) = self.find(model_id)?;
        if model.variant(variant_id).is_none() {
            return Err(LibraryError::UnknownVariant(
                model_id.into(),
                variant_id.into(),
            ));
        }
        self.db
            .set_default_model(model.modality.as_str(), model_id, variant_id)?;
        tracing::info!(modality = model.modality.as_str(), model = %model_id, variant = %variant_id, "default model set");
        Ok(())
    }

    /// Registers a GGUF file from disk without copying it.
    pub fn add_custom_from_disk(
        &self,
        path: &Path,
        name: Option<&str>,
    ) -> Result<ModelView, LibraryError> {
        let size = check_gguf(path)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model");
        let id = custom_id(stem, &path.to_string_lossy());
        let file_path = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(stem)
            .to_owned();
        let row = CustomModelRow {
            id: id.clone(),
            name: name.map(str::to_owned).unwrap_or_else(|| stem.to_owned()),
            source: "disk".into(),
            repo: None,
            file_path: file_path.clone(),
            size,
            sha256: None,
            added_at: now_secs(),
        };
        self.db.insert_custom_model(&row)?;
        self.db.insert_installed_file(&InstalledFile {
            model_id: id.clone(),
            variant_id: CUSTOM_VARIANT.into(),
            file_path,
            repo: String::new(),
            local_path: path.to_string_lossy().into_owned(),
            size,
            sha256: None,
            source: FileSource::UserFile,
        })?;
        tracing::info!(id = %id, path = %path.display(), size, "custom GGUF added from disk");
        self.view_of(&id)
    }

    /// Registers a GGUF file from a Hugging Face repository (size and sha256
    /// from the HF API). The caller then queues the download.
    pub async fn add_custom_from_hf(
        db: &Db,
        client: &reqwest::Client,
        token: Option<&str>,
        repo: &str,
        path: &str,
    ) -> Result<CustomModelRow, LibraryError> {
        if !path.to_lowercase().ends_with(".gguf") {
            return Err(LibraryError::NotGguf(path.to_owned()));
        }
        let info = hf::file_info(client, repo, path, None, token).await?;
        let file_name = path.rsplit('/').next().unwrap_or(path);
        let stem = file_name.trim_end_matches(".gguf");
        let row = CustomModelRow {
            id: custom_id(stem, &format!("{repo}/{path}")),
            name: stem.to_owned(),
            source: "hf".into(),
            repo: Some(repo.to_owned()),
            file_path: path.to_owned(),
            size: info.size,
            sha256: info.sha256,
            added_at: now_secs(),
        };
        db.insert_custom_model(&row)?;
        tracing::info!(id = %row.id, repo, path, size = row.size, "custom GGUF added from Hugging Face");
        Ok(row)
    }

    pub fn remove_custom(
        &self,
        downloads: &DownloadManager,
        id: &str,
        data_dir: Option<&Path>,
    ) -> Result<(), LibraryError> {
        self.delete_variant(downloads, id, CUSTOM_VARIANT, data_dir)?;
        self.db.remove_custom_model(id)?;
        Ok(())
    }

    fn view_of(&self, id: &str) -> Result<ModelView, LibraryError> {
        self.list(None, &[], None)?
            .into_iter()
            .find(|m| m.model.id == id)
            .ok_or_else(|| LibraryError::UnknownModel(id.to_owned()))
    }
}

fn variant_state(
    all_installed: bool,
    any_installed: bool,
    pending: &[&crate::downloads::DownloadItem],
) -> VariantState {
    if pending.is_empty() {
        return if all_installed {
            VariantState::Installed
        } else if any_installed {
            VariantState::Partial
        } else {
            VariantState::NotInstalled
        };
    }
    let has = |s: DownloadStatus| pending.iter().any(|d| d.status == s);
    if has(DownloadStatus::Downloading) || has(DownloadStatus::Verifying) {
        VariantState::Downloading
    } else if has(DownloadStatus::Failed) {
        VariantState::Failed
    } else if has(DownloadStatus::Queued) {
        VariantState::Queued
    } else {
        VariantState::Paused
    }
}

fn custom_model(row: &CustomModelRow) -> Model {
    let size = row.size;
    Model {
        id: row.id.clone(),
        name: row.name.clone(),
        publisher: row.repo.clone().unwrap_or_default(),
        modality: Modality::Llm,
        engine: "llama-cpp".into(),
        params_b: None,
        active_params_b: None,
        description_pl: String::new(),
        description_en: String::new(),
        languages: vec![],
        features: vec!["chat".into()],
        license: License {
            name: String::new(),
            url: row
                .repo
                .as_ref()
                .map(|r| format!("{}/{r}", hf::HF_ENDPOINT))
                .unwrap_or_default(),
            commercial: Commercial::Conditions,
            conditions_pl: None,
            conditions_en: None,
            eu_allowed: true,
            gated: false,
        },
        variants: vec![Variant {
            id: CUSTOM_VARIANT.into(),
            label: "GGUF".into(),
            files: vec![ModelFile {
                repo: row.repo.clone().unwrap_or_default(),
                path: row.file_path.clone(),
                revision: None,
                size,
                sha256: row.sha256.clone().unwrap_or_default(),
                role: Some("model".into()),
                local: None,
            }],
            memory_estimate_mb: estimate_memory_mb(Modality::Llm, size),
            platforms: vec![],
            recommended_backends: vec![],
            recommended: true,
        }],
        tags: vec![],
        benchmark: vec![],
    }
}

fn custom_id(stem: &str, unique: &str) -> String {
    let slug: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    // short, stable suffix so two files with the same name do not collide
    let hash = unique.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!(
        "{CUSTOM_PREFIX}{}-{:06x}",
        &slug[..slug.len().min(48)],
        hash & 0xff_ffff
    )
}

/// Directory-safe version of a catalog id.
fn safe_segment(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .to_owned()
}

/// Checks the GGUF magic bytes and returns the file size.
fn check_gguf(path: &Path) -> Result<u64, LibraryError> {
    let meta = std::fs::metadata(path)?;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)?.read_exact(&mut magic)?;
    if &magic != b"GGUF" || !meta.is_file() {
        return Err(LibraryError::NotGguf(path.display().to_string()));
    }
    Ok(meta.len())
}

/// One proposed model per modality for the first-run wizard (V1, step 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StarterPick {
    pub modality: Modality,
    pub model_id: String,
    pub model_name: String,
    pub variant_id: String,
    pub variant_label: String,
    pub size: u64,
    pub fit: Option<FitReport>,
    #[serde(default)]
    pub speed: Option<SpeedReport>,
    pub installed: bool,
}

/// Share of the memory budget a starter model may use, so that a chat model
/// and an image model can coexist with the scheduler's LRU policy (PRD §4.4).
/// Without a GPU transcription speed matters more than memory, so CPU-only
/// machines get a smaller Whisper (PRD §6.2: tiny/base/small for weaker devices).
fn starter_share(modality: Modality, kind: lokalsi_hwprobe::BudgetKind) -> f64 {
    match (modality, kind) {
        (Modality::Llm, _) => 0.60,
        // Diffusion models are too heavy without a graphics card (PRD §15 item 14).
        (Modality::Image, lokalsi_hwprobe::BudgetKind::Cpu) => 0.0,
        (Modality::Image, _) => 0.80,
        (Modality::Stt, lokalsi_hwprobe::BudgetKind::Cpu) => 0.15,
        (Modality::Stt, _) => 0.35,
        (Modality::Tts, _) => 0.35,
        // Songs are optional and large: never part of the starter set.
        (Modality::Music, _) => 0.0,
        // Decisions are an Expert tool (ADR 0024): not in the starter set.
        (Modality::Decision, _) => 0.0,
    }
}

/// Largest number of parameters used per token that still answers at a usable
/// speed without a graphics card (≈4B dense, Bielik 4.5B, or MoE with 2–4B active).
pub const CPU_MAX_COMPUTE_PARAMS_B: f32 = 4.5;

/// Without a graphics card only small LLMs or MoE models with few active
/// parameters are recommended (PRD §15 item 14). Other modalities and
/// machines with a GPU are not restricted here.
fn cpu_friendly(model: &Model, kind: lokalsi_hwprobe::BudgetKind) -> bool {
    if kind != lokalsi_hwprobe::BudgetKind::Cpu || model.modality != Modality::Llm {
        return true;
    }
    model
        .compute_params_b()
        .is_some_and(|p| p <= CPU_MAX_COMPUTE_PARAMS_B)
}

/// 0 = fast or usable (or unknown), 1 = slow; too slow variants are not offered.
fn speed_rank(v: &VariantView) -> Option<u8> {
    match v.speed.as_ref().map(|s| s.speed) {
        Some(Speed::TooSlow) => None,
        Some(Speed::Slow) => Some(1),
        _ => Some(0),
    }
}

/// Picks, per modality, the best model that fits comfortably and is not too
/// slow (ADR 0016): a fast or usable model beats a slow one, so weak computers
/// get smaller, quicker models. Then LLM prefers Polish models when the UI
/// language is Polish, images prefer commercial licences, TTS prefers the light
/// model without voice cloning; otherwise the larger (better) model wins.
/// Within the chosen model the recommended variant is used when it fits, else
/// the largest that does.
pub fn starter_pack(models: &[ModelView], budget: &MemoryBudget, polish: bool) -> Vec<StarterPick> {
    let mut picks = Vec::new();
    for modality in Modality::ALL {
        let limit = (budget.main_mb as f64 * starter_share(modality, budget.kind)) as u64;
        let fits = |v: &VariantView| {
            v.supported_here
                && speed_rank(v).is_some()
                && v.memory_estimate_mb <= limit
                && v.fit
                    .as_ref()
                    .is_some_and(|f| f.fit == lokalsi_hwprobe::Fit::Fits)
        };
        let mut candidates: Vec<(&ModelView, &VariantView)> = models
            .iter()
            .filter(|m| m.model.modality == modality && !m.custom)
            .filter(|m| cpu_friendly(&m.model, budget.kind))
            .filter_map(|m| {
                let fitting: Vec<&VariantView> = m.variants.iter().filter(|v| fits(v)).collect();
                let chosen = fitting
                    .iter()
                    .find(|v| v.recommended)
                    .or_else(|| fitting.iter().max_by_key(|v| v.memory_estimate_mb))?;
                Some((m, *chosen))
            })
            .collect();
        let pref = |m: &ModelView| -> i32 {
            match modality {
                Modality::Llm if polish && m.model.tags.iter().any(|t| t == "polski") => 1,
                Modality::Image if m.model.license.commercial == Commercial::Yes => 1,
                Modality::Tts if !m.model.features.iter().any(|f| f == "voice_cloning") => 1,
                _ => 0,
            }
        };
        candidates.sort_by(|a, b| {
            speed_rank(a.1)
                .cmp(&speed_rank(b.1))
                .then_with(|| pref(b.0).cmp(&pref(a.0)))
                .then_with(|| {
                    if modality == Modality::Tts {
                        a.1.memory_estimate_mb.cmp(&b.1.memory_estimate_mb)
                    } else {
                        b.1.memory_estimate_mb.cmp(&a.1.memory_estimate_mb)
                    }
                })
        });
        if let Some((m, v)) = candidates.first() {
            picks.push(StarterPick {
                modality,
                model_id: m.model.id.clone(),
                model_name: m.model.name.clone(),
                variant_id: v.id.clone(),
                variant_label: v.label.clone(),
                size: v.size,
                fit: v.fit.clone(),
                speed: v.speed.clone(),
                installed: v.state == VariantState::Installed,
            });
        }
    }
    picks
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokalsi_hwprobe::{BudgetKind, MemoryBudget};

    fn budget(main_mb: u64, kind: BudgetKind) -> MemoryBudget {
        MemoryBudget {
            kind,
            main_mb,
            offload_mb: if kind == BudgetKind::Gpu { 60_000 } else { 0 },
            device: None,
            speed: None,
        }
    }

    fn views(b: &MemoryBudget) -> Vec<ModelView> {
        let catalog = Catalog::embedded().unwrap();
        let db = Db::open_in_memory().unwrap();
        Library::new(&catalog, &db)
            .list(Some(b), &[], None)
            .unwrap()
    }

    #[test]
    fn starter_pack_for_rtx3090_prefers_polish_llm() {
        let b = budget(24_064, BudgetKind::Gpu);
        let picks = starter_pack(&views(&b), &b, true);
        assert_eq!(picks.len(), 4, "{picks:#?}");
        let llm = picks.iter().find(|p| p.modality == Modality::Llm).unwrap();
        assert!(llm.model_id.starts_with("bielik-11b"), "{llm:?}");
        let image = picks
            .iter()
            .find(|p| p.modality == Modality::Image)
            .unwrap();
        assert_eq!(
            image.model_id, "z-image-turbo",
            "commercial licence preferred"
        );
        let stt = picks.iter().find(|p| p.modality == Modality::Stt).unwrap();
        assert_eq!(stt.model_id, "whisper-large-v3-turbo");
        let tts = picks.iter().find(|p| p.modality == Modality::Tts).unwrap();
        assert_eq!(tts.model_id, "piper-pl-darkman");
    }

    #[test]
    fn starter_pack_for_small_cpu_machine() {
        let b = budget(4_096, BudgetKind::Cpu);
        let picks = starter_pack(&views(&b), &b, true);
        let llm = picks.iter().find(|p| p.modality == Modality::Llm);
        assert!(llm.is_none_or(|p| p.fit.as_ref().unwrap().estimate_mb <= 2_458));
        let stt = picks.iter().find(|p| p.modality == Modality::Stt).unwrap();
        assert_ne!(stt.model_id, "whisper-large-v3-turbo");
    }

    #[test]
    fn starter_pack_without_gpu_has_small_llm_and_no_image() {
        // 16 GB mini PC with integrated graphics (budget = usable RAM).
        let b = budget(11_640, BudgetKind::Cpu);
        let picks = starter_pack(&views(&b), &b, true);
        assert!(
            picks.iter().all(|p| p.modality != Modality::Image),
            "{picks:#?}"
        );
        let llm = picks.iter().find(|p| p.modality == Modality::Llm).unwrap();
        assert_eq!(llm.model_id, "bielik-4.5b-v3", "Polish and ≤4.5B");

        // 64 GB without GPU: the MoE model with 3B active parameters is allowed.
        let big = budget(60_000, BudgetKind::Cpu);
        let picks = starter_pack(&views(&big), &big, false);
        let llm = picks.iter().find(|p| p.modality == Modality::Llm).unwrap();
        assert_eq!(llm.model_id, "qwen3.6-35b-a3b", "{llm:?}");
    }

    fn real_budget(
        cpu: lokalsi_hwprobe::CpuInfo,
        ram_mb: u64,
        gpus: Vec<lokalsi_hwprobe::GpuInfo>,
    ) -> MemoryBudget {
        let profile = lokalsi_hwprobe::HardwareProfile {
            cpu,
            ram_mb,
            gpus,
            ..Default::default()
        };
        lokalsi_hwprobe::budget(&profile, 70)
    }

    #[test]
    fn starter_pack_prefers_fast_models_on_weak_computers() {
        // Ubuntu mini PC: i5-7300U (2 cores, AVX2), HD Graphics 620, 16 GB.
        let cpu = lokalsi_hwprobe::CpuInfo {
            brand: "Intel Core i5-7300U".into(),
            physical_cores: Some(2),
            logical_cores: 4,
            features: vec!["avx2".into()],
            ..Default::default()
        };
        let hd620 = lokalsi_hwprobe::GpuInfo {
            vendor: lokalsi_hwprobe::GpuVendor::Intel,
            name: "Intel(R) HD Graphics 620".into(),
            vram_mb: 11_802,
            source: "os".into(),
            ..Default::default()
        };
        let b = real_budget(cpu, 15_736, vec![hd620]);
        assert_eq!(b.kind, BudgetKind::Cpu);
        let models = views(&b);
        let picks = starter_pack(&models, &b, true);
        let llm = picks.iter().find(|p| p.modality == Modality::Llm).unwrap();
        // Bielik 4.5B Q8 runs at about 3 tok/s here; Gemma 4 E2B (2B effective)
        // is still usable and, being larger than LFM3, wins the tie.
        assert_eq!(llm.model_id, "gemma4-e2b", "{picks:#?}");
        assert!(llm.speed.as_ref().unwrap().speed >= Speed::Usable);
        let stt = picks.iter().find(|p| p.modality == Modality::Stt).unwrap();
        assert_ne!(stt.model_id, "whisper-large-v3-turbo");
        assert!(picks.iter().all(|p| p.modality != Modality::Image));

        let bielik = models
            .iter()
            .find(|m| m.model.id == "bielik-4.5b-v3")
            .unwrap();
        assert_eq!(
            bielik.variants[0].speed.as_ref().unwrap().speed,
            Speed::Slow
        );
    }

    #[test]
    fn starter_pack_on_rtx3090_keeps_big_models() {
        let cpu = lokalsi_hwprobe::CpuInfo {
            brand: "Intel Core i5-11400F".into(),
            physical_cores: Some(6),
            logical_cores: 12,
            features: vec!["avx2".into(), "avx512f".into()],
            ..Default::default()
        };
        let rtx = lokalsi_hwprobe::GpuInfo {
            vendor: lokalsi_hwprobe::GpuVendor::Nvidia,
            name: "NVIDIA GeForce RTX 3090".into(),
            vram_mb: 24_576,
            memory_bandwidth_gbs: Some(936.0),
            fp32_tflops: Some(35.6),
            source: "nvml".into(),
            ..Default::default()
        };
        let b = real_budget(cpu, 65_536, vec![rtx]);
        let picks = starter_pack(&views(&b), &b, true);
        let llm = picks.iter().find(|p| p.modality == Modality::Llm).unwrap();
        assert!(llm.model_id.starts_with("bielik-11b"), "{llm:?}");
        assert_eq!(llm.speed.as_ref().unwrap().speed, Speed::Fast);
        let stt = picks.iter().find(|p| p.modality == Modality::Stt).unwrap();
        assert_eq!(stt.model_id, "whisper-large-v3-turbo");
        let image = picks
            .iter()
            .find(|p| p.modality == Modality::Image)
            .unwrap();
        assert_eq!(image.speed.as_ref().unwrap().speed, Speed::Fast);
    }

    #[test]
    fn custom_ids_are_safe_and_stable() {
        let a = custom_id("My Model.Q4_K_M", "C:/x/My Model.Q4_K_M.gguf");
        assert!(a.starts_with("custom-my-model-q4-k-m-"), "{a}");
        assert_eq!(a, custom_id("My Model.Q4_K_M", "C:/x/My Model.Q4_K_M.gguf"));
        assert_ne!(a, custom_id("My Model.Q4_K_M", "D:/y/My Model.Q4_K_M.gguf"));
        assert_eq!(safe_segment("../x"), "_x");
    }

    #[test]
    fn custom_gguf_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tiny.gguf");
        std::fs::write(&path, b"GGUF\x03\0\0\0rest").unwrap();
        std::fs::write(dir.path().join("fake.gguf"), b"nope").unwrap();
        let catalog = Catalog::embedded().unwrap();
        let db = Db::open_in_memory().unwrap();
        let lib = Library::new(&catalog, &db);
        let view = lib.add_custom_from_disk(&path, None).unwrap();
        assert!(view.custom);
        assert_eq!(view.variants[0].state, VariantState::Installed);
        assert!(matches!(
            lib.add_custom_from_disk(&dir.path().join("fake.gguf"), None),
            Err(LibraryError::NotGguf(_))
        ));
    }

    #[test]
    fn plans_point_into_data_dir() {
        let catalog = Catalog::embedded().unwrap();
        let db = Db::open_in_memory().unwrap();
        let plan = Library::new(&catalog, &db)
            .plan("whisper-tiny", "q8_0", Path::new("D:/Lokalsi"))
            .unwrap();
        assert_eq!(
            plan.dest_dir,
            Path::new("D:/Lokalsi/models/whisper-tiny/q8_0")
        );
        assert!(plan.files[0].url.contains("/resolve/"));
    }

    #[test]
    fn some_files_without_downloads_is_partial() {
        assert_eq!(variant_state(true, true, &[]), VariantState::Installed);
        assert_eq!(variant_state(false, true, &[]), VariantState::Partial);
        assert_eq!(variant_state(false, false, &[]), VariantState::NotInstalled);
    }
}

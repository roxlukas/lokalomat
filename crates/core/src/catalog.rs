//! Model catalog (`catalog/models.json`, PRD §6). The manifest is embedded in
//! the binary; sizes and sha256 sums are filled by `tools/catalog-sync` from
//! the Hugging Face API, never by hand.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub const CATALOG_JSON: &str = include_str!("../../../catalog/models.json");
pub const SCHEMA_VERSION: u32 = 1;

/// A model and variant picked by id (pipeline steps, requests, agents).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub model_id: String,
    pub variant_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Llm,
    Image,
    Stt,
    Tts,
    /// Songs with vocals or instrumentals (ADR 0022).
    Music,
    /// Typed decisions with probabilities (Basal on llama.cpp, ADR 0024).
    Decision,
}

impl Modality {
    pub const ALL: [Modality; 6] = [
        Modality::Llm,
        Modality::Image,
        Modality::Stt,
        Modality::Tts,
        Modality::Music,
        Modality::Decision,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Modality::Llm => "llm",
            Modality::Image => "image",
            Modality::Stt => "stt",
            Modality::Tts => "tts",
            Modality::Music => "music",
            Modality::Decision => "decision",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Commercial {
    Yes,
    Conditions,
    No,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct License {
    pub name: String,
    pub url: String,
    pub commercial: Commercial,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions_pl: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions_en: Option<String>,
    pub eu_allowed: bool,
    /// Requires a Hugging Face account and accepting the licence on the website.
    pub gated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFile {
    pub repo: String,
    pub path: String,
    /// Commit the size/sha256 were read from; downloads are pinned to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Bytes; 0 = not synced yet.
    pub size: u64,
    /// Lower-case hex; empty or `TODO…` = not synced yet.
    pub sha256: String,
    /// What the engine uses the file for, e.g. `model`, `vae`, `text_encoder`, `config`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Path inside the variant folder when the engine needs a folder tree
    /// (ACE-Step, ADR 0022); `None` = the file name alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
}

impl ModelFile {
    pub fn has_valid_sha256(&self) -> bool {
        is_sha256(&self.sha256)
    }

    /// Download URL, pinned to `revision` when known.
    pub fn url(&self) -> String {
        crate::hf::resolve_url(&self.repo, self.revision.as_deref(), &self.path)
    }

    /// File name used on disk.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Path inside the variant folder: `local`, else the file name.
    pub fn local_path(&self) -> &str {
        self.local.as_deref().unwrap_or_else(|| self.file_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    pub id: String,
    pub label: String,
    pub files: Vec<ModelFile>,
    /// Estimated memory needed to run, see [`estimate_memory_mb`].
    pub memory_estimate_mb: u64,
    pub platforms: Vec<String>,
    pub recommended_backends: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recommended: bool,
}

impl Variant {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub modality: Modality,
    pub engine: String,
    /// Parameters in billions (LLMs), from the model card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_b: Option<f32>,
    /// Active parameters per token for mixture-of-experts models (e.g. 35B-A3B → 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_params_b: Option<f32>,
    pub description_pl: String,
    pub description_en: String,
    pub languages: Vec<String>,
    pub features: Vec<String>,
    pub license: License,
    pub variants: Vec<Variant>,
    pub tags: Vec<String>,
    /// Model names in the AI Transformation LLM benchmark (`catalog/benchmark.json`),
    /// in order of preference. Empty when the model was not tested.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub benchmark: Vec<String>,
}

impl Model {
    pub fn variant(&self, id: &str) -> Option<&Variant> {
        self.variants.iter().find(|v| v.id == id)
    }

    /// Parameters used per token: active ones for MoE, else all.
    pub fn compute_params_b(&self) -> Option<f32> {
        self.active_params_b.or(self.params_b)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub models: Vec<Model>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("catalog schema version {0} is not supported")]
    Version(u32),
    #[error("catalog is invalid: {}", .0.join("; "))]
    Invalid(Vec<String>),
}

impl Catalog {
    /// The catalog shipped with the application.
    pub fn embedded() -> Result<Self, CatalogError> {
        Self::parse(CATALOG_JSON)
    }

    /// Keeps only the models of `allowed` kinds: an edition offers the models of the
    /// modes it has (ADR 0035). The shipped catalog is the large list; the free
    /// edition filters it when the app starts.
    pub fn retain_modalities(&mut self, allowed: &[Modality]) {
        self.models.retain(|m| allowed.contains(&m.modality));
    }

    pub fn parse(json: &str) -> Result<Self, CatalogError> {
        let catalog: Catalog = serde_json::from_str(json)?;
        if catalog.version != SCHEMA_VERSION {
            return Err(CatalogError::Version(catalog.version));
        }
        let problems = catalog.structural_problems();
        if !problems.is_empty() {
            return Err(CatalogError::Invalid(problems));
        }
        Ok(catalog)
    }

    pub fn model(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }

    /// Errors that make the catalog unusable (duplicate ids, empty variants…).
    pub fn structural_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut ids = HashSet::new();
        for m in &self.models {
            if !ids.insert(&m.id) {
                problems.push(format!("duplicate model id `{}`", m.id));
            }
            if m.variants.is_empty() {
                problems.push(format!("`{}` has no variants", m.id));
            }
            let mut vids = HashSet::new();
            for v in &m.variants {
                if !vids.insert(&v.id) {
                    problems.push(format!("`{}` has duplicate variant `{}`", m.id, v.id));
                }
                if v.files.is_empty() {
                    problems.push(format!("`{}/{}` has no files", m.id, v.id));
                }
                for f in &v.files {
                    let bad_local = f.local.as_deref().is_some_and(|l| !is_safe_relative(l));
                    if f.repo.split('/').count() != 2
                        || f.path.is_empty()
                        || f.path.contains("..")
                        || bad_local
                    {
                        problems.push(format!(
                            "`{}/{}` has an invalid file `{}/{}`",
                            m.id, v.id, f.repo, f.path
                        ));
                    }
                }
                let mut locals = HashSet::new();
                for f in &v.files {
                    if !locals.insert(f.local_path()) {
                        problems.push(format!(
                            "`{}/{}` stores two files as `{}`",
                            m.id,
                            v.id,
                            f.local_path()
                        ));
                    }
                }
            }
        }
        problems
    }

    /// Files whose size or sha256 has not been synced from Hugging Face yet.
    pub fn unsynced_files(&self) -> Vec<String> {
        self.models
            .iter()
            .flat_map(|m| m.variants.iter().map(move |v| (m, v)))
            .flat_map(|(m, v)| v.files.iter().map(move |f| (m, v, f)))
            .filter(|(_, _, f)| f.size == 0 || !f.has_valid_sha256())
            .map(|(m, v, f)| format!("{}/{}: {}/{}", m.id, v.id, f.repo, f.path))
            .collect()
    }
}

/// `a/b/c.json`: relative, forward slashes, no `.`/`..`, no drive or root.
pub fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

pub fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Rough memory needed to run a model of `total_bytes` (weights on disk).
///
/// * LLM and decision models: weights + 10% + 1 GiB for KV cache (≈8k context)
///   and compute buffers,
/// * image: weights + 20% + 1 GiB activations at 1024 px,
/// * STT: weights × 1.5 + 300 MiB,
/// * TTS: weights × 1.3 + 500 MiB,
/// * music: weights × 1.35 + 1 GiB (ACE-Step 1.5 turbo + LM 1.7B: 10.1 GB of
///   files peaked at 14.4 GB on an RTX 3090, ADR 0022).
///
/// Used by `catalog-sync` and for custom GGUF files (PRD §6.2).
pub fn estimate_memory_mb(modality: Modality, total_bytes: u64) -> u64 {
    let mb = total_bytes as f64 / (1024.0 * 1024.0);
    let est = match modality {
        Modality::Llm | Modality::Decision => mb * 1.10 + 1024.0,
        Modality::Image => mb * 1.20 + 1024.0,
        Modality::Stt => mb * 1.50 + 300.0,
        Modality::Tts => mb * 1.30 + 500.0,
        Modality::Music => mb * 1.35 + 1024.0,
    };
    est.ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_paths_stay_inside_the_variant_folder() {
        assert!(is_safe_relative("acestep-v15-turbo/config.json"));
        assert!(is_safe_relative("model.gguf"));
        for bad in ["", "/etc/x", "a/../b", "a//b", "./a", "C:/x", "a\\b"] {
            assert!(!is_safe_relative(bad), "{bad}");
        }
        let c = Catalog::embedded().unwrap();
        let ace = c.model("ace-step-1.5").expect("ACE-Step in the catalogue");
        assert_eq!(ace.modality, Modality::Music);
        for v in &ace.variants {
            assert!(
                v.files.iter().any(|f| f.role.as_deref() == Some("dit")),
                "{}",
                v.id
            );
            assert!(
                v.files.iter().all(|f| f.local_path().contains('/')),
                "{}",
                v.id
            );
        }
        // Two files may not land on the same path.
        let mut dup = c.clone();
        let files = &mut dup
            .models
            .iter_mut()
            .find(|m| m.id == "ace-step-1.5")
            .unwrap()
            .variants[0]
            .files;
        files[1].local = files[0].local.clone();
        assert!(
            dup.structural_problems()
                .iter()
                .any(|p| p.contains("stores two files"))
        );
    }

    #[test]
    fn embedded_catalog_is_valid() {
        let c = Catalog::embedded().expect("catalog/models.json must parse");
        assert!(!c.models.is_empty());
        for modality in Modality::ALL {
            assert!(
                c.models.iter().any(|m| m.modality == modality),
                "catalog has no {modality:?} model"
            );
        }
    }

    /// PRD §6.2: every size and sum comes from the HF API (`tools/catalog-sync`).
    #[test]
    fn embedded_catalog_is_fully_synced() {
        let c = Catalog::embedded().unwrap();
        let missing = c.unsynced_files();
        assert!(
            missing.is_empty(),
            "run `cargo run -p catalog-sync` – unsynced files: {missing:#?}"
        );
        for m in &c.models {
            for v in &m.variants {
                assert!(
                    v.memory_estimate_mb > 0,
                    "{}/{} has no memory estimate",
                    m.id,
                    v.id
                );
            }
        }
    }

    #[test]
    fn detects_structural_problems() {
        let json = r#"{"version":1,"models":[
          {"id":"a","name":"A","publisher":"p","modality":"llm","engine":"llama-cpp",
           "description_pl":"","description_en":"","languages":[],"features":[],
           "license":{"name":"MIT","url":"","commercial":"yes","eu_allowed":true,"gated":false},
           "variants":[],"tags":[]},
          {"id":"a","name":"A","publisher":"p","modality":"llm","engine":"llama-cpp",
           "description_pl":"","description_en":"","languages":[],"features":[],
           "license":{"name":"MIT","url":"","commercial":"yes","eu_allowed":true,"gated":false},
           "variants":[{"id":"q","label":"Q","files":[{"repo":"bad","path":"../x","size":0,"sha256":""}],
             "memory_estimate_mb":0,"platforms":[],"recommended_backends":[]}],"tags":[]}]}"#;
        match Catalog::parse(json) {
            Err(CatalogError::Invalid(p)) => assert_eq!(p.len(), 3, "{p:?}"),
            other => panic!("expected invalid catalog, got {other:?}"),
        }
    }

    #[test]
    fn sha256_format() {
        assert!(is_sha256(&"a".repeat(64)));
        assert!(!is_sha256(&"A".repeat(64)));
        assert!(!is_sha256("TODO-z-HF-API"));
    }

    #[test]
    fn memory_estimates() {
        let gib = 1024 * 1024 * 1024;
        assert_eq!(estimate_memory_mb(Modality::Llm, 5 * gib), 6656);
        assert_eq!(estimate_memory_mb(Modality::Stt, gib / 2), 1068);
    }
}

//! Phone model catalog (`catalog/phone-models.json`). Like the desktop catalog,
//! sizes and sha256 sums are filled by `tools/catalog-sync` from the Hugging
//! Face API, never by hand.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub const CATALOG_JSON: &str = include_str!("../../../catalog/phone-models.json");
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("invalid phone catalog: {0}")]
    Json(#[from] serde_json::Error),
    #[error("phone catalog version {0} is not supported (expected {SCHEMA_VERSION})")]
    Version(u32),
    #[error("phone catalog: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneLicense {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneFile {
    pub repo: String,
    pub path: String,
    /// Commit the size/sha256 were read from; downloads are pinned to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Bytes; 0 = not synced yet.
    pub size: u64,
    /// Lower-case hex; empty = not synced yet.
    pub sha256: String,
}

impl PhoneFile {
    pub fn is_synced(&self) -> bool {
        self.size > 0
            && self.revision.is_some()
            && self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    /// `https://huggingface.co/{repo}/resolve/{revision}/{path}`.
    pub fn url(&self) -> String {
        let mut path = String::with_capacity(self.path.len());
        for b in self.path.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                    path.push(b as char)
                }
                _ => path.push_str(&format!("%{b:02X}")),
            }
        }
        format!(
            "https://huggingface.co/{}/resolve/{}/{path}",
            self.repo,
            self.revision.as_deref().unwrap_or("main")
        )
    }

    /// File name on disk.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// Whether the phone has enough memory for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    Ok,
    /// Less memory than the model needs: Android may stop it or it may be very slow.
    Low,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneModel {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub description_pl: String,
    pub description_en: String,
    pub params_b: f32,
    pub languages: Vec<String>,
    pub license: PhoneLicense,
    /// Total phone RAM (MB) this model needs to run comfortably. An estimate
    /// from our own tests (docs/plans/android-local-chat.md), not from the publisher.
    pub min_ram_mb: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recommended: bool,
    pub file: PhoneFile,
}

impl PhoneModel {
    pub fn fit(&self, total_ram_mb: u64) -> Fit {
        if total_ram_mb >= self.min_ram_mb {
            Fit::Ok
        } else {
            Fit::Low
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneCatalog {
    pub version: u32,
    pub models: Vec<PhoneModel>,
}

impl PhoneCatalog {
    pub fn parse(text: &str) -> Result<Self, CatalogError> {
        let catalog: PhoneCatalog = serde_json::from_str(text)?;
        if catalog.version != SCHEMA_VERSION {
            return Err(CatalogError::Version(catalog.version));
        }
        let mut seen = HashSet::new();
        for m in &catalog.models {
            let id_ok = !m.id.is_empty()
                && m.id.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'
                });
            if !id_ok {
                return Err(CatalogError::Invalid(format!("bad model id `{}`", m.id)));
            }
            if !seen.insert(m.id.as_str()) {
                return Err(CatalogError::Invalid(format!("duplicate model `{}`", m.id)));
            }
            if m.file.path.is_empty() || m.file.path.contains("..") {
                return Err(CatalogError::Invalid(format!(
                    "bad file path in `{}`",
                    m.id
                )));
            }
        }
        Ok(catalog)
    }

    /// The catalog built into the app.
    pub fn embedded() -> Self {
        Self::parse(CATALOG_JSON).expect("catalog/phone-models.json is valid")
    }

    pub fn find(&self, id: &str) -> Option<&PhoneModel> {
        self.models.iter().find(|m| m.id == id)
    }

    /// Models whose size or sha256 has not been synced from Hugging Face yet.
    pub fn unsynced(&self) -> Vec<String> {
        self.models
            .iter()
            .filter(|m| !m.file.is_synced())
            .map(|m| m.id.clone())
            .collect()
    }
}

//! Scores from the AI Transformation local-LLM benchmark
//! (<https://aitransformation.pl/llm-benchmark/browse.html>, data CC BY 4.0).
//!
//! The results are fetched once at startup with a short timeout. When the
//! site does not respond, the benchmark is simply unavailable for this run and
//! the UI hides benchmark sorting (docs/adr/0008).

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::catalog::Model;

pub const SOURCE_URL: &str = "https://aitransformation.pl/llm-benchmark/browse.html";
pub const DATA_URL: &str = "https://aitransformation.pl/llm-benchmark/results.json";
/// The whole request (connect + body) must finish within this time.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryScore {
    pub avg: f32,
    /// Number of rated answers in the category.
    pub n: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkEntry {
    /// Mean score (1–5) over all rated answers.
    pub overall: f32,
    #[serde(default)]
    pub rated: u32,
    #[serde(default)]
    pub tokens_per_second: Option<f32>,
    #[serde(default)]
    pub quantization: Option<String>,
    #[serde(default)]
    pub params_b: Option<f32>,
    #[serde(default)]
    pub categories: BTreeMap<String, CategoryScore>,
}

/// What the UI needs for the sort menu and the attribution line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkMeta {
    pub title: String,
    pub source_url: String,
    pub data_url: String,
    pub license: String,
    pub attribution: String,
    pub generated_at: String,
    pub evaluator: String,
    pub scale_max: f32,
    /// Categories with the number of prompts, as published.
    pub categories: Vec<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Benchmark {
    pub meta: BenchmarkMeta,
    pub models: BTreeMap<String, BenchmarkEntry>,
}

/// Scores attached to a catalog model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelScores {
    /// Name of the model in the benchmark (e.g. `qwen3.8:27b`).
    pub name: String,
    #[serde(flatten)]
    pub entry: BenchmarkEntry,
}

#[derive(Debug, thiserror::Error)]
pub enum BenchmarkError {
    #[error("benchmark site unavailable: {0}")]
    Http(#[from] reqwest::Error),
    #[error("benchmark site returned HTTP {0}")]
    Status(u16),
    #[error("benchmark data is too large")]
    TooLarge,
    #[error("benchmark data has an unexpected format: {0}")]
    Format(#[from] serde_json::Error),
}

// --- published format (`results.json`) ----------------------------------------

#[derive(Deserialize)]
struct RawResults {
    #[serde(default)]
    generated_at: String,
    #[serde(default)]
    evaluator: String,
    #[serde(default)]
    categories: Vec<RawCategory>,
    models: Vec<RawModel>,
}

#[derive(Deserialize)]
struct RawCategory {
    name: String,
    #[serde(default)]
    prompts: u32,
}

#[derive(Deserialize)]
struct RawModel {
    name: String,
    #[serde(flatten)]
    entry: BenchmarkEntry,
}

impl Benchmark {
    /// Parses the published `results.json`.
    pub fn parse(json: &str) -> Result<Self, BenchmarkError> {
        let raw: RawResults = serde_json::from_str(json)?;
        Ok(Benchmark {
            meta: BenchmarkMeta {
                title: "Benchmark lokalnych LLM – AI Transformation".into(),
                source_url: SOURCE_URL.into(),
                data_url: DATA_URL.into(),
                license: "CC BY 4.0".into(),
                attribution: "AI Transformation (aitransformation.pl)".into(),
                generated_at: raw.generated_at,
                evaluator: raw.evaluator,
                scale_max: 5.0,
                categories: raw
                    .categories
                    .into_iter()
                    .map(|c| (c.name, c.prompts))
                    .collect(),
            },
            models: raw.models.into_iter().map(|m| (m.name, m.entry)).collect(),
        })
    }

    /// Downloads the current results. Errors (timeout, offline, site down)
    /// mean "no benchmark this run" – callers just log and carry on.
    pub async fn fetch(client: &reqwest::Client) -> Result<Self, BenchmarkError> {
        let started = std::time::Instant::now();
        let resp = client.get(DATA_URL).timeout(FETCH_TIMEOUT).send().await?;
        if !resp.status().is_success() {
            return Err(BenchmarkError::Status(resp.status().as_u16()));
        }
        if resp
            .content_length()
            .is_some_and(|l| l as usize > MAX_BODY_BYTES)
        {
            return Err(BenchmarkError::TooLarge);
        }
        let body = resp.text().await?;
        if body.len() > MAX_BODY_BYTES {
            return Err(BenchmarkError::TooLarge);
        }
        let bench = Self::parse(&body)?;
        tracing::info!(
            models = bench.models.len(),
            generated_at = %bench.meta.generated_at,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "benchmark loaded"
        );
        Ok(bench)
    }

    /// Scores of the first benchmark name of `model` present in the data.
    pub fn scores_for(&self, model: &Model) -> Option<ModelScores> {
        model.benchmark.iter().find_map(|name| {
            self.models.get(name).map(|entry| ModelScores {
                name: name.clone(),
                entry: entry.clone(),
            })
        })
    }

    /// Catalog mappings that point at names missing from the data.
    pub fn missing_mappings<'a>(&self, models: impl IntoIterator<Item = &'a Model>) -> Vec<String> {
        models
            .into_iter()
            .filter(|m| !m.benchmark.is_empty() && self.scores_for(m).is_none())
            .map(|m| format!("{}: {:?}", m.id, m.benchmark))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    const SAMPLE: &str = r#"{
      "generated_at": "2026-11-02T07:13:51Z",
      "evaluator": "judge",
      "dimensions": ["accuracy"],
      "categories": [{"name": "Programming", "prompts": 33}, {"name": "Polski", "prompts": 3}],
      "models": [
        {"name": "qwen3.8:27b", "family": "qwen35", "overall": 4.169, "rated": 108,
         "quantization": "Q4_K_M", "params_b": 27.3, "tokens_per_second": 55.6,
         "categories": {"Programming": {"avg": 3.848, "n": 33}}, "dimensions": {"accuracy": 3.8}},
        {"name": "no-speed", "overall": 2.0, "tokens_per_second": null}
      ]
    }"#;

    #[test]
    fn parses_published_format() {
        let b = Benchmark::parse(SAMPLE).unwrap();
        assert_eq!(b.meta.categories[0], ("Programming".to_owned(), 33));
        assert_eq!(b.models["qwen3.8:27b"].categories["Programming"].n, 33);
        assert_eq!(b.models["no-speed"].tokens_per_second, None);
    }

    #[test]
    fn maps_catalog_models_exactly() {
        let b = Benchmark::parse(SAMPLE).unwrap();
        let catalog = Catalog::embedded().unwrap();
        let qwen = catalog.model("qwen3.8-27b").unwrap();
        assert!((b.scores_for(qwen).unwrap().entry.overall - 4.169).abs() < 1e-6);
        let bielik = catalog.model("bielik-11b-v3.1").unwrap();
        assert!(b.scores_for(bielik).is_none(), "v3.1 is not mapped");
        assert!(
            !b.missing_mappings(&catalog.models).is_empty(),
            "sample lacks the other mapped models"
        );
    }

    #[test]
    fn rejects_unexpected_format() {
        assert!(matches!(
            Benchmark::parse("{\"oops\": 1}"),
            Err(BenchmarkError::Format(_))
        ));
    }
}

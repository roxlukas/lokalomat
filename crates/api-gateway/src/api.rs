//! Wire types of the gateway (JSON). Shapes follow the OpenAI API where one
//! exists (`/v1/chat/completions`, `/v1/audio/*`, `/v1/images/generations`),
//! so the full OpenAI-compatible server in M5c can grow from the same routes.
//! Long work (images, songs, decisions) also runs as jobs the phone starts
//! and polls (`/v1/jobs/*`, ADR 0027), so a locked screen does not cancel it.

use serde::{Deserialize, Serialize};

/// Header with the device's credentials: `Authorization: Bearer <device id>.<access key>`.
pub fn bearer(device_id: &str, access_key: &str) -> String {
    format!("Bearer {device_id}.{access_key}")
}

/// `POST /pair` – the phone after scanning the QR code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRequest {
    pub token: String,
    /// Shown in the paired devices list.
    pub device_name: String,
    /// Ed25519 public key (base64url, 32 bytes).
    pub public_key: String,
    /// Signature of [`crate::pairing::signed_message`] (base64url).
    pub signature: String,
}

/// Answer to `POST /pair` after the user confirmed on the computer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairResponse {
    pub device_id: String,
    pub access_key: String,
    pub computer_name: String,
}

/// Error body of every failed request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    /// `unauthorized`, `forbidden_origin`, `pairing_expired`, `pairing_rejected`,
    /// `bad_token`, `bad_request`, `too_large`, or a job error code.
    pub error: String,
    #[serde(default)]
    pub detail: String,
}

/// `GET /v1/info`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Info {
    pub computer_name: String,
    pub version: String,
    /// Modes the computer can run now: `chat`, `stt`, `tts`, `image`, `music`
    /// and, in Expert mode on the computer, `decision`.
    pub modes: Vec<String>,
    /// The computer is in Expert mode: the phone shows the same extra settings
    /// (ADR 0037). Older computers do not send it (= Basic).
    #[serde(default)]
    pub expert: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    /// `system`, `user` or `assistant`.
    pub role: String,
    pub content: String,
}

/// `GET /v1/models?mode=chat|stt|tts|image|music|decision` – models installed on the computer
/// that the phone may pick for a mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOption {
    /// `<model id>:<variant id>`, sent back as `model` in requests.
    pub id: String,
    pub name: String,
    /// The model the computer uses when the phone does not choose one.
    #[serde(default)]
    pub default: bool,
}

/// `<model id>:<variant id>` → its parts (variant ids never contain `:`).
pub fn split_model_id(id: &str) -> Option<(String, String)> {
    let (model, variant) = id.rsplit_once(':')?;
    (!model.is_empty() && !variant.is_empty()).then(|| (model.to_owned(), variant.to_owned()))
}

pub fn model_id(model: &str, variant: &str) -> String {
    format!("{model}:{variant}")
}

/// `POST /v1/chat/completions` (non-streaming subset).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<Turn>,
    /// A [`ModelOption::id`]; `None` = the computer's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: Turn,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub object: String,
    pub model: String,
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// `POST /v1/audio/transcriptions` (raw audio body, `?language=pl`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcription {
    pub text: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// The job on the computer (Gallery entry).
    #[serde(default)]
    pub job_id: Option<i64>,
}

/// `POST /v1/audio/speech` → `audio/wav`; `POST /v1/jobs/speech` → a job (ADR 0037).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpeechRequest {
    pub input: String,
    #[serde(default)]
    pub voice_id: Option<i64>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Speaking rate 0.5–2 (Piper).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    /// Chatterbox (Expert): emotion intensity 0–2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exaggeration: Option<f32>,
    /// Chatterbox (Expert): CFG weight 0–1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cfg_weight: Option<f32>,
    /// Chatterbox (Expert): temperature 0.05–2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// `POST /v1/images/generations` → `{ data: [{ b64_json }] }`; `POST /v1/jobs/image` → a job.
/// The fields after `model` are the Workshop's (ADR 0037); missing = its defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageRequest {
    pub prompt: String,
    /// `1:1`, `3:4`, `16:9`…
    #[serde(default)]
    pub aspect: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// What to avoid (Expert on the computer and the phone).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub negative_prompt: String,
    /// Images in one job, 1–4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// The same seed and settings give the same image; `None` = random.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cfg_scale: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub b64_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResponse {
    pub data: Vec<ImageData>,
}

// --- jobs the phone starts and polls (ADR 0027) ----------------------------------------

/// `POST /v1/jobs/music` – one song (ADR 0022).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MusicJobRequest {
    /// Style: genre, mood, instruments.
    #[serde(default)]
    pub caption: String,
    /// Lyrics; empty = instrumental.
    #[serde(default)]
    pub lyrics: String,
    /// Language of the lyrics (`pl`, `en`…); `None` = Polish.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Length in seconds (10–600); `None` = the model decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The same seed gives the same song (Expert, ADR 0037); `None` = random.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// Kind of a decision question; the same JSON as `lokalsi_core::decisions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionKind {
    Choice {
        options: Vec<String>,
    },
    YesNo,
    /// Levels, lowest first.
    Score {
        levels: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionQuestion {
    pub id: String,
    pub instructions: String,
    #[serde(flatten)]
    pub kind: DecisionKind,
}

/// `POST /v1/jobs/decision` – typed questions about a text (ADR 0024).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionJobRequest {
    /// The text to judge.
    pub state: String,
    pub questions: Vec<DecisionQuestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionProbability {
    pub label: String,
    pub p: f64,
}

/// One answered question: the top label (`yes`/`no` for yes/no) and every probability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionAnswer {
    pub id: String,
    pub label: String,
    pub probabilities: Vec<DecisionProbability>,
    pub confidence: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// A result file of a job: download it with `GET /v1/gallery/{job_id}/files/{index}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobFile {
    pub index: usize,
    /// `image/png`, `audio/mpeg`, `audio/wav`.
    pub content_type: String,
}

/// `POST /v1/jobs/{image,music,speech,decision}` and `GET /v1/jobs/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobView {
    pub job_id: i64,
    /// `image`, `music`, `tts`, `decision`.
    pub kind: String,
    /// `queued`, `preparing`, `loading_model`, `running`, `done`, `failed`, `cancelled`.
    pub status: String,
    /// 0–1 while running, when known.
    #[serde(default)]
    pub progress: Option<f64>,
    /// Error code of a failed job (`jobs.errors.*` on the computer).
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub model: String,
    /// Files of a finished image, song or speech.
    #[serde(default)]
    pub files: Vec<JobFile>,
    /// Answers of a finished decision, in question order.
    #[serde(default)]
    pub answers: Vec<DecisionAnswer>,
}

impl JobView {
    /// True once the job will not change any more.
    pub fn finished(&self) -> bool {
        matches!(self.status.as_str(), "done" | "failed" | "cancelled")
    }
}

/// Content type of a result file by its extension.
pub fn content_type_of(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

/// `GET /v1/voices`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    pub id: i64,
    pub name: String,
    pub language: String,
}

/// `GET /v1/gallery` – recent results on the computer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryItem {
    pub job_id: i64,
    /// `image`, `stt`, `tts`, `music`.
    pub kind: String,
    pub text: String,
    pub model: String,
    pub created_at: i64,
    /// Number of files (`GET /v1/gallery/{job_id}/files/{index}`; images
    /// also have `…/{index}/thumb`).
    pub files: usize,
    /// The job's settings (as stored on the computer), for "use again" (ADR 0037).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub params: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::{model_id, split_model_id};

    #[test]
    fn model_ids_round_trip() {
        let id = model_id("gemma4-e4b", "q4_k_m");
        assert_eq!(
            split_model_id(&id),
            Some(("gemma4-e4b".into(), "q4_k_m".into()))
        );
        assert_eq!(split_model_id("no-variant"), None);
        assert_eq!(split_model_id(":v"), None);
        assert_eq!(split_model_id("m:"), None);
    }
}

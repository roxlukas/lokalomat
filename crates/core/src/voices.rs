//! Voice library (V6, PRD §9.3 and §11): reference recordings for voice
//! cloning. A voice can only be saved together with the consent statement the
//! user confirmed ("To mój głos albo mam zgodę tej osoby"); the statement and
//! its time are stored with the profile and travel with exported voices.
//!
//! Recordings live in `<data_dir>/voices/` as mono WAV; the database keeps
//! the file name only, so moving the data folder keeps voices working.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError, now_secs};

/// `format` in exported `voice.json`.
pub const FORMAT: &str = "studio-lokalsi-voice";
pub const FORMAT_VERSION: u32 = 1;
/// Largest recording accepted from an archive.
const MAX_SAMPLE_BYTES: u64 = 64 << 20;
const MAX_META_BYTES: u64 = 64 << 10;
const MAX_NAME_CHARS: usize = 80;

#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a voice archive: {0}")]
    Invalid(String),
    /// Saving a voice needs the confirmed consent statement.
    #[error("the consent statement was not confirmed")]
    ConsentRequired,
    #[error("the voice needs a name")]
    NoName,
    #[error("voice not found")]
    NotFound,
}

impl From<rusqlite::Error> for VoiceError {
    fn from(e: rusqlite::Error) -> Self {
        VoiceError::Db(DbError::Sql(e))
    }
}

impl From<zip::result::ZipError> for VoiceError {
    fn from(e: zip::result::ZipError) -> Self {
        VoiceError::Invalid(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    pub id: i64,
    pub name: String,
    /// ISO 639-1 code of the recording.
    pub language: String,
    /// Model the voice was made for (`None` = any cloning model).
    pub model_id: Option<String>,
    /// File name inside the voices folder.
    pub sample_file: String,
    pub sample_ms: u64,
    /// Sample quality at saving time (see `lokalsi_engines_media::audio::SampleQuality`).
    pub quality: serde_json::Value,
    pub consent_text: String,
    pub consent_at: i64,
    /// `recording`, `upload` or `import`.
    pub source: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewVoice<'a> {
    pub name: &'a str,
    pub language: &'a str,
    pub model_id: Option<&'a str>,
    pub sample_file: &'a str,
    pub sample_ms: u64,
    pub quality: serde_json::Value,
    /// Must be `true`: the user ticked the consent statement.
    pub consent: bool,
    pub consent_text: &'a str,
    pub source: &'a str,
}

/// `<data_dir>/voices`.
pub fn voices_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("voices")
}

/// Full path of a voice's recording.
pub fn sample_path(data_dir: &Path, voice: &Voice) -> PathBuf {
    voices_dir(data_dir).join(&voice.sample_file)
}

/// A fresh file name for a recording (`voice-<time>-<random>.wav`).
pub fn new_sample_file() -> String {
    let mut b = [0u8; 4];
    let _ = getrandom::fill(&mut b);
    format!("voice-{}-{}.wav", now_secs(), hex::encode(b))
}

/// Trimmed, single-line name of at most 80 characters.
pub fn clean_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect()
}

fn clean_language(lang: &str) -> String {
    let l = lang.trim().to_ascii_lowercase();
    if (2..=3).contains(&l.len()) && l.chars().all(|c| c.is_ascii_lowercase()) {
        l
    } else {
        "pl".into()
    }
}

/// Only plain file names inside the voices folder.
fn safe_file(name: &str) -> bool {
    !name.is_empty()
        && name.ends_with(".wav")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !name.contains("..")
}

const COLUMNS: &str = "id, name, language, model_id, sample_file, sample_ms, quality, consent_text, consent_at, source, created_at, updated_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Voice> {
    Ok(Voice {
        id: r.get(0)?,
        name: r.get(1)?,
        language: r.get(2)?,
        model_id: r.get(3)?,
        sample_file: r.get(4)?,
        sample_ms: r.get::<_, i64>(5)?.max(0) as u64,
        quality: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or(serde_json::Value::Null),
        consent_text: r.get(7)?,
        consent_at: r.get(8)?,
        source: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

pub struct Voices<'a> {
    db: &'a Db,
}

impl<'a> Voices<'a> {
    pub fn new(db: &'a Db) -> Self {
        Voices { db }
    }

    pub fn create(&self, v: &NewVoice<'_>) -> Result<Voice, VoiceError> {
        if !v.consent || v.consent_text.trim().is_empty() {
            return Err(VoiceError::ConsentRequired);
        }
        let name = clean_name(v.name);
        if name.is_empty() {
            return Err(VoiceError::NoName);
        }
        if !safe_file(v.sample_file) {
            return Err(VoiceError::Invalid("sample file name".into()));
        }
        let now = now_secs();
        let id = {
            let conn = self.db.conn();
            conn.execute(
                "INSERT INTO voices (name, language, model_id, sample_file, sample_ms, quality, consent_text, consent_at, source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?8)",
                params![
                    name,
                    clean_language(v.language),
                    v.model_id,
                    v.sample_file,
                    v.sample_ms as i64,
                    v.quality.to_string(),
                    v.consent_text.trim(),
                    now,
                    v.source,
                ],
            )?;
            conn.last_insert_rowid()
        };
        tracing::info!(
            voice_id = id,
            sample_ms = v.sample_ms,
            source = v.source,
            "voice saved with consent"
        );
        self.get(id)?.ok_or(VoiceError::NotFound)
    }

    pub fn get(&self, id: i64) -> Result<Option<Voice>, VoiceError> {
        Ok(self
            .db
            .conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM voices WHERE id = ?1"),
                params![id],
                row,
            )
            .optional()?)
    }

    pub fn list(&self) -> Result<Vec<Voice>, VoiceError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM voices ORDER BY name COLLATE NOCASE, id"
        ))?;
        let voices = stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(voices)
    }

    /// Renames a voice and/or changes its language.
    pub fn update(&self, id: i64, name: &str, language: &str) -> Result<Voice, VoiceError> {
        let name = clean_name(name);
        if name.is_empty() {
            return Err(VoiceError::NoName);
        }
        let n = self.db.conn().execute(
            "UPDATE voices SET name = ?2, language = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, name, clean_language(language), now_secs()],
        )?;
        if n == 0 {
            return Err(VoiceError::NotFound);
        }
        self.get(id)?.ok_or(VoiceError::NotFound)
    }

    /// Replaces the recording (re-trimmed sample). Returns the old file name.
    pub fn set_sample(
        &self,
        id: i64,
        sample_file: &str,
        sample_ms: u64,
        quality: &serde_json::Value,
    ) -> Result<String, VoiceError> {
        if !safe_file(sample_file) {
            return Err(VoiceError::Invalid("sample file name".into()));
        }
        let old = self.get(id)?.ok_or(VoiceError::NotFound)?;
        self.db.conn().execute(
            "UPDATE voices SET sample_file = ?2, sample_ms = ?3, quality = ?4, updated_at = ?5 WHERE id = ?1",
            params![id, sample_file, sample_ms as i64, quality.to_string(), now_secs()],
        )?;
        Ok(old.sample_file)
    }

    /// Removes the profile and its recording.
    pub fn delete(&self, id: i64, data_dir: &Path) -> Result<bool, VoiceError> {
        let Some(voice) = self.get(id)? else {
            return Ok(false);
        };
        self.db
            .conn()
            .execute("DELETE FROM voices WHERE id = ?1", params![id])?;
        remove_sample(data_dir, &voice.sample_file);
        tracing::info!(voice_id = id, "voice deleted");
        Ok(true)
    }
}

/// Deletes a recording file from the voices folder (never outside it).
pub fn remove_sample(data_dir: &Path, file: &str) {
    if safe_file(file) {
        let _ = std::fs::remove_file(voices_dir(data_dir).join(file));
    }
}

/// Metadata in an exported voice archive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceMeta {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub language: String,
    #[serde(default)]
    pub model_id: Option<String>,
    pub sample_ms: u64,
    #[serde(default)]
    pub quality: serde_json::Value,
    pub consent_text: String,
    pub consent_at: i64,
    pub created_at: i64,
    #[serde(default)]
    pub software: String,
}

/// Writes `voice.json` and `sample.wav` into a ZIP archive at `dest`.
pub fn export_zip(voice: &Voice, sample: &Path, dest: &Path) -> Result<(), VoiceError> {
    let meta = VoiceMeta {
        format: FORMAT.into(),
        version: FORMAT_VERSION,
        name: voice.name.clone(),
        language: voice.language.clone(),
        model_id: voice.model_id.clone(),
        sample_ms: voice.sample_ms,
        quality: voice.quality.clone(),
        consent_text: voice.consent_text.clone(),
        consent_at: voice.consent_at,
        created_at: voice.created_at,
        software: format!("Lokalomat AI Studio {}", env!("CARGO_PKG_VERSION")),
    };
    let wav = std::fs::read(sample)?;
    let tmp = dest.with_extension("partial");
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("voice.json", opts)?;
        zip.write_all(
            serde_json::to_string_pretty(&meta)
                .unwrap_or_default()
                .as_bytes(),
        )?;
        zip.start_file("sample.wav", opts)?;
        zip.write_all(&wav)?;
        zip.finish()?;
    }
    std::fs::rename(&tmp, dest)?;
    tracing::info!(voice_id = voice.id, "voice exported");
    Ok(())
}

fn read_entry(
    zip: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, VoiceError> {
    let entry = zip
        .by_name(name)
        .map_err(|_| VoiceError::Invalid(format!("{name} missing")))?;
    if entry.size() > limit {
        return Err(VoiceError::Invalid(format!("{name} too large")));
    }
    let mut buf = Vec::new();
    entry.take(limit + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(VoiceError::Invalid(format!("{name} too large")));
    }
    Ok(buf)
}

/// Reads an exported voice: its metadata and the WAV bytes. Checks the format
/// and sizes; the caller still asks for consent before saving it.
pub fn read_zip(path: &Path) -> Result<(VoiceMeta, Vec<u8>), VoiceError> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let meta: VoiceMeta =
        serde_json::from_slice(&read_entry(&mut zip, "voice.json", MAX_META_BYTES)?)
            .map_err(|e| VoiceError::Invalid(e.to_string()))?;
    if meta.format != FORMAT || meta.version > FORMAT_VERSION {
        return Err(VoiceError::Invalid(format!(
            "{} v{}",
            meta.format, meta.version
        )));
    }
    let wav = read_entry(&mut zip, "sample.wav", MAX_SAMPLE_BYTES)?;
    if wav.len() < 44 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(VoiceError::Invalid("sample.wav is not a WAV file".into()));
    }
    Ok((meta, wav))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_voice<'a>(file: &'a str) -> NewVoice<'a> {
        NewVoice {
            name: "  Mój   głos ",
            language: "PL",
            model_id: Some("chatterbox-multilingual"),
            sample_file: file,
            sample_ms: 20_000,
            quality: serde_json::json!({"level": "good"}),
            consent: true,
            consent_text: "To mój głos albo mam zgodę tej osoby.",
            source: "recording",
        }
    }

    #[test]
    fn saving_needs_consent_and_a_name() {
        let db = Db::open_in_memory().unwrap();
        let voices = Voices::new(&db);
        let mut v = new_voice("voice-1-ab.wav");
        v.consent = false;
        assert!(matches!(
            voices.create(&v),
            Err(VoiceError::ConsentRequired)
        ));
        v.consent = true;
        v.consent_text = " ";
        assert!(matches!(
            voices.create(&v),
            Err(VoiceError::ConsentRequired)
        ));
        let mut v = new_voice("voice-1-ab.wav");
        v.name = "\n ";
        assert!(matches!(voices.create(&v), Err(VoiceError::NoName)));
        let v = new_voice("../evil.wav");
        assert!(matches!(voices.create(&v), Err(VoiceError::Invalid(_))));
    }

    #[test]
    fn lifecycle_and_archive_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let voices = Voices::new(&db);
        let file = new_sample_file();
        let v = voices.create(&new_voice(&file)).unwrap();
        assert_eq!(v.name, "Mój głos");
        assert_eq!(v.language, "pl");
        assert!(v.consent_at > 0);
        let v = voices.update(v.id, "Lektor", "en").unwrap();
        assert_eq!((v.name.as_str(), v.language.as_str()), ("Lektor", "en"));
        assert_eq!(voices.list().unwrap().len(), 1);

        std::fs::create_dir_all(voices_dir(dir.path())).unwrap();
        let sample = sample_path(dir.path(), &v);
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.resize(64, 0);
        std::fs::write(&sample, &wav).unwrap();
        let archive = dir.path().join("lektor.zip");
        export_zip(&v, &sample, &archive).unwrap();
        let (meta, bytes) = read_zip(&archive).unwrap();
        assert_eq!(meta.name, "Lektor");
        assert_eq!(meta.consent_text, v.consent_text);
        assert_eq!(bytes, wav);

        let old = voices
            .set_sample(v.id, "voice-2-cd.wav", 12_000, &serde_json::json!({}))
            .unwrap();
        assert_eq!(old, file);
        assert!(voices.delete(v.id, dir.path()).unwrap());
        assert!(voices.get(v.id).unwrap().is_none());
        assert!(!voices.delete(v.id, dir.path()).unwrap());
    }

    #[test]
    fn rejects_foreign_archives() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            z.start_file("voice.json", zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(br#"{"format":"other","version":1,"name":"a","language":"pl","sample_ms":1,"consent_text":"x","consent_at":1,"created_at":1}"#)
                .unwrap();
            z.finish().unwrap();
        }
        assert!(matches!(read_zip(&path), Err(VoiceError::Invalid(_))));
        assert_eq!(clean_name("a\u{7}b"), "ab");
    }
}

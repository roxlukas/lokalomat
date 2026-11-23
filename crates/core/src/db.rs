//! SQLite storage (`app.db`, PRD §4.5). Schema changes are append-only
//! migrations tracked with `PRAGMA user_version`.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub use rusqlite::Error as SqlError;

/// Each entry upgrades the schema by one version. Never edit a released entry.
const MIGRATIONS: &[&str] = &[
    // 1: hardware profile, downloads, installed model files, custom models, defaults
    r#"
    CREATE TABLE hardware_profile (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        json TEXT NOT NULL,
        probed_at INTEGER NOT NULL
    );
    CREATE TABLE downloads (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        model_id TEXT NOT NULL,
        variant_id TEXT NOT NULL,
        repo TEXT NOT NULL,
        file_path TEXT NOT NULL,
        revision TEXT,
        url TEXT NOT NULL,
        dest TEXT NOT NULL,
        size INTEGER NOT NULL,
        sha256 TEXT,
        gated INTEGER NOT NULL DEFAULT 0,
        downloaded INTEGER NOT NULL DEFAULT 0,
        status TEXT NOT NULL,
        error TEXT,
        attempts INTEGER NOT NULL DEFAULT 0,
        position INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX downloads_status ON downloads (status, position);
    CREATE TABLE model_files (
        model_id TEXT NOT NULL,
        variant_id TEXT NOT NULL,
        file_path TEXT NOT NULL,
        repo TEXT NOT NULL,
        local_path TEXT NOT NULL,
        size INTEGER NOT NULL,
        sha256 TEXT,
        source TEXT NOT NULL,
        installed_at INTEGER NOT NULL,
        PRIMARY KEY (model_id, variant_id, file_path)
    );
    CREATE TABLE custom_models (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        source TEXT NOT NULL,
        repo TEXT,
        file_path TEXT NOT NULL,
        size INTEGER NOT NULL,
        sha256 TEXT,
        added_at INTEGER NOT NULL
    );
    CREATE TABLE default_models (
        modality TEXT PRIMARY KEY,
        model_id TEXT NOT NULL,
        variant_id TEXT NOT NULL
    );
    "#,
    // 2: installed engines, chat sessions and messages (M2)
    r#"
    CREATE TABLE installed_engines (
        engine_id TEXT PRIMARY KEY,
        build_id TEXT NOT NULL,
        version TEXT NOT NULL,
        backend TEXT NOT NULL,
        dir TEXT NOT NULL,
        installed_at INTEGER NOT NULL
    );
    CREATE TABLE sessions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL,
        pinned INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE messages (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        role TEXT NOT NULL,
        content TEXT NOT NULL DEFAULT '',
        reasoning TEXT NOT NULL DEFAULT '',
        model_id TEXT,
        variant_id TEXT,
        status TEXT NOT NULL,
        error TEXT,
        tokens INTEGER,
        tokens_per_second REAL,
        created_at INTEGER NOT NULL,
        finished_at INTEGER
    );
    CREATE INDEX messages_session ON messages (session_id, id);
    "#,
    // 3: MCP servers (M2b) and tool calls recorded on assistant messages
    r#"
    CREATE TABLE mcp_servers (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        config TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    ALTER TABLE messages ADD COLUMN tool_calls TEXT NOT NULL DEFAULT '[]';
    "#,
    // 4: transcription and image jobs with their results (M3)
    r#"
    CREATE TABLE jobs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        kind TEXT NOT NULL,
        status TEXT NOT NULL,
        progress REAL,
        model_id TEXT,
        variant_id TEXT,
        model_name TEXT NOT NULL DEFAULT '',
        engine TEXT NOT NULL DEFAULT '',
        params TEXT NOT NULL DEFAULT '{}',
        inputs TEXT NOT NULL DEFAULT '{}',
        outputs TEXT NOT NULL DEFAULT '{}',
        text TEXT NOT NULL DEFAULT '',
        error TEXT,
        created_at INTEGER NOT NULL,
        started_at INTEGER,
        finished_at INTEGER,
        duration_ms INTEGER
    );
    CREATE INDEX jobs_kind ON jobs (kind, id);
    "#,
    // 5: every file a queued variant needs, so a variant counts as installed
    // only when all of them arrived (cancelled files leave no download row)
    r#"
    CREATE TABLE variant_files (
        model_id TEXT NOT NULL,
        variant_id TEXT NOT NULL,
        file_path TEXT NOT NULL,
        PRIMARY KEY (model_id, variant_id, file_path)
    );
    "#,
    // 6: voice library (V6, M4): reference recordings for voice cloning with
    // the consent statement the user confirmed
    r#"
    CREATE TABLE voices (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        language TEXT NOT NULL DEFAULT 'pl',
        model_id TEXT,
        sample_file TEXT NOT NULL,
        sample_ms INTEGER NOT NULL,
        quality TEXT NOT NULL DEFAULT '{}',
        consent_text TEXT NOT NULL,
        consent_at INTEGER NOT NULL,
        source TEXT NOT NULL DEFAULT 'upload',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    "#,
    // 7 (M5): saved pipelines and their runs (PRD §8.2, V7); phones paired
    // with the local API gateway (PRD §15 item 12) – their access keys live
    // in the OS credential store, never here
    r#"
    CREATE TABLE pipelines (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        template TEXT,
        steps TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE pipeline_runs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        pipeline_id INTEGER,
        name TEXT NOT NULL,
        status TEXT NOT NULL,
        definition TEXT NOT NULL,
        input TEXT NOT NULL DEFAULT '{}',
        steps TEXT NOT NULL DEFAULT '[]',
        error TEXT,
        created_at INTEGER NOT NULL,
        finished_at INTEGER
    );
    CREATE TABLE paired_devices (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        public_key TEXT NOT NULL,
        paired_at INTEGER NOT NULL,
        last_seen INTEGER
    );
    "#,
    // M5c: access keys of the local OpenAI-compatible server (ADR 0025)
    r#"
    CREATE TABLE api_keys (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        hash TEXT NOT NULL,
        agent TEXT,
        created_at INTEGER NOT NULL,
        last_used_at INTEGER,
        requests INTEGER NOT NULL DEFAULT 0
    );
    "#,
    // M7: MCP clients the user allowed, per access key (ADR 0034)
    r#"
    CREATE TABLE mcp_clients (
        key_id TEXT NOT NULL,
        name TEXT NOT NULL,
        version TEXT,
        allowed_at INTEGER NOT NULL,
        last_seen_at INTEGER,
        PRIMARY KEY (key_id, name)
    );
    "#,
    // Default parameters of a chat model, set in the Inspector (ADR 0036)
    r#"
    CREATE TABLE model_settings (
        model_id TEXT NOT NULL,
        variant_id TEXT NOT NULL,
        params TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (model_id, variant_id)
    );
    "#,
];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("cannot create database directory: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "database schema version {found} is newer than this application supports ({supported})"
    )]
    TooNew { found: i64, supported: i64 },
    #[error("stored data is invalid: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct Db {
    conn: Mutex<Connection>,
}

/// Where an installed file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSource {
    /// Downloaded by the app into the data directory (deleted with the model).
    Download,
    /// Found in the local Hugging Face cache (never deleted by the app).
    HfCache,
    /// A file the user pointed at on disk (never deleted by the app).
    UserFile,
}

impl FileSource {
    pub fn as_str(self) -> &'static str {
        match self {
            FileSource::Download => "download",
            FileSource::HfCache => "hf_cache",
            FileSource::UserFile => "user_file",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "hf_cache" => FileSource::HfCache,
            "user_file" => FileSource::UserFile,
            _ => FileSource::Download,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledFile {
    pub model_id: String,
    pub variant_id: String,
    pub file_path: String,
    pub repo: String,
    pub local_path: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub source: FileSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomModelRow {
    pub id: String,
    pub name: String,
    /// `disk` or `hf`.
    pub source: String,
    pub repo: Option<String>,
    pub file_path: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub added_at: i64,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, DbError> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&conn)?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn schema_version(&self) -> i64 {
        self.conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap_or(0)
    }

    // --- hardware profile -------------------------------------------------

    pub fn save_hardware(&self, profile: &lokalsi_hwprobe::HardwareProfile) -> Result<(), DbError> {
        let json = serde_json::to_string(profile)?;
        self.conn().execute(
            "INSERT INTO hardware_profile (id, json, probed_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET json = excluded.json, probed_at = excluded.probed_at",
            params![json, profile.probed_at as i64],
        )?;
        Ok(())
    }

    pub fn load_hardware(&self) -> Result<Option<lokalsi_hwprobe::HardwareProfile>, DbError> {
        let json: Option<String> = self
            .conn()
            .query_row("SELECT json FROM hardware_profile WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        match json {
            // A profile from an older build that no longer parses, or lacks the
            // compute details of the current probe, is simply re-probed.
            Some(j) => Ok(serde_json::from_str::<lokalsi_hwprobe::HardwareProfile>(&j)
                .ok()
                .filter(|p| p.probe_version >= lokalsi_hwprobe::PROBE_VERSION)),
            None => Ok(None),
        }
    }

    // --- installed files ----------------------------------------------------

    pub fn insert_installed_file(&self, f: &InstalledFile) -> Result<(), DbError> {
        self.conn().execute(
            "INSERT OR REPLACE INTO model_files
             (model_id, variant_id, file_path, repo, local_path, size, sha256, source, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                f.model_id,
                f.variant_id,
                f.file_path,
                f.repo,
                f.local_path,
                f.size as i64,
                f.sha256,
                f.source.as_str(),
                now_secs()
            ],
        )?;
        Ok(())
    }

    pub fn installed_files(&self) -> Result<Vec<InstalledFile>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT model_id, variant_id, file_path, repo, local_path, size, sha256, source
             FROM model_files ORDER BY model_id, variant_id, file_path",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(InstalledFile {
                model_id: r.get(0)?,
                variant_id: r.get(1)?,
                file_path: r.get(2)?,
                repo: r.get(3)?,
                local_path: r.get(4)?,
                size: r.get::<_, i64>(5)? as u64,
                sha256: r.get(6)?,
                source: FileSource::parse(&r.get::<_, String>(7)?),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn installed_files_for(
        &self,
        model_id: &str,
        variant_id: &str,
    ) -> Result<Vec<InstalledFile>, DbError> {
        Ok(self
            .installed_files()?
            .into_iter()
            .filter(|f| f.model_id == model_id && f.variant_id == variant_id)
            .collect())
    }

    /// Records the full file list of a variant (replacing an older list).
    pub fn set_variant_files(
        &self,
        model_id: &str,
        variant_id: &str,
        files: &[&str],
    ) -> Result<(), DbError> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM variant_files WHERE model_id = ?1 AND variant_id = ?2",
            params![model_id, variant_id],
        )?;
        for f in files {
            conn.execute(
                "INSERT OR IGNORE INTO variant_files (model_id, variant_id, file_path) VALUES (?1, ?2, ?3)",
                params![model_id, variant_id, f],
            )?;
        }
        Ok(())
    }

    /// Files a variant needs; empty for variants queued before migration 5.
    pub fn variant_files(&self, model_id: &str, variant_id: &str) -> Result<Vec<String>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT file_path FROM variant_files WHERE model_id = ?1 AND variant_id = ?2 ORDER BY file_path",
        )?;
        let files = stmt
            .query_map(params![model_id, variant_id], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(files)
    }

    pub fn remove_installed_variant(
        &self,
        model_id: &str,
        variant_id: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM model_files WHERE model_id = ?1 AND variant_id = ?2",
            params![model_id, variant_id],
        )?;
        conn.execute(
            "DELETE FROM default_models WHERE model_id = ?1 AND variant_id = ?2",
            params![model_id, variant_id],
        )?;
        Ok(())
    }

    // --- defaults -----------------------------------------------------------

    pub fn set_default_model(
        &self,
        modality: &str,
        model_id: &str,
        variant_id: &str,
    ) -> Result<(), DbError> {
        self.conn().execute(
            "INSERT INTO default_models (modality, model_id, variant_id) VALUES (?1, ?2, ?3)
             ON CONFLICT(modality) DO UPDATE SET model_id = excluded.model_id, variant_id = excluded.variant_id",
            params![modality, model_id, variant_id],
        )?;
        Ok(())
    }

    /// `(modality, model_id, variant_id)`.
    pub fn default_models(&self) -> Result<Vec<(String, String, String)>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT modality, model_id, variant_id FROM default_models")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // --- custom models ------------------------------------------------------

    pub fn insert_custom_model(&self, m: &CustomModelRow) -> Result<(), DbError> {
        self.conn().execute(
            "INSERT OR REPLACE INTO custom_models
             (id, name, source, repo, file_path, size, sha256, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                m.id,
                m.name,
                m.source,
                m.repo,
                m.file_path,
                m.size as i64,
                m.sha256,
                m.added_at
            ],
        )?;
        Ok(())
    }

    pub fn custom_models(&self) -> Result<Vec<CustomModelRow>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, source, repo, file_path, size, sha256, added_at
             FROM custom_models ORDER BY added_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(CustomModelRow {
                id: r.get(0)?,
                name: r.get(1)?,
                source: r.get(2)?,
                repo: r.get(3)?,
                file_path: r.get(4)?,
                size: r.get::<_, i64>(5)? as u64,
                sha256: r.get(6)?,
                added_at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn remove_custom_model(&self, id: &str) -> Result<(), DbError> {
        self.conn()
            .execute("DELETE FROM custom_models WHERE id = ?1", params![id])?;
        Ok(())
    }
}

fn migrate(conn: &Connection) -> Result<(), DbError> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let supported = MIGRATIONS.len() as i64;
    if current > supported {
        return Err(DbError::TooNew {
            found: current,
            supported,
        });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = i as i64 + 1;
        conn.execute_batch(&format!(
            "BEGIN; {sql}; PRAGMA user_version = {version}; COMMIT;"
        ))?;
        tracing::info!(version, "database migrated");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.db");
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version(), MIGRATIONS.len() as i64);
        drop(db);
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version(), MIGRATIONS.len() as i64);
    }

    #[test]
    fn refuses_newer_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        assert!(matches!(Db::init(conn), Err(DbError::TooNew { .. })));
    }

    #[test]
    fn installed_files_and_defaults() {
        let db = Db::open_in_memory().unwrap();
        let f = InstalledFile {
            model_id: "m".into(),
            variant_id: "v".into(),
            file_path: "a.gguf".into(),
            repo: "org/repo".into(),
            local_path: "C:/models/a.gguf".into(),
            size: 10,
            sha256: Some("ab".into()),
            source: FileSource::HfCache,
        };
        db.insert_installed_file(&f).unwrap();
        db.set_default_model("llm", "m", "v").unwrap();
        assert_eq!(db.installed_files_for("m", "v").unwrap(), vec![f]);
        assert_eq!(db.default_models().unwrap().len(), 1);
        db.remove_installed_variant("m", "v").unwrap();
        assert!(db.installed_files().unwrap().is_empty());
        assert!(
            db.default_models().unwrap().is_empty(),
            "default cleared with the variant"
        );
    }
}

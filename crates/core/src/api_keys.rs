//! Access keys of the local OpenAI-compatible server (PRD §15 point 9, ADR 0025).
//!
//! A key looks like `lk-<id>.<secret>` and is sent as `Authorization: Bearer …`.
//! The database keeps only its SHA-256 hash; the caller may store the whole key
//! in the OS credential store (`secrets::*_api_key`) so an agent can be set up
//! again later without a new key.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::{Db, DbError, now_secs};

/// Prefix of every key, so users and secret scanners recognise them.
pub const KEY_PREFIX: &str = "lk-";
/// Longest key name.
pub const MAX_NAME_CHARS: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    /// The agent this key was made for (M5c agents), `None` = made by hand.
    pub agent: Option<String>,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub requests: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiKeyError {
    #[error("the key name is empty or too long")]
    BadName,
    #[error("no random numbers: {0}")]
    Random(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

impl From<rusqlite::Error> for ApiKeyError {
    fn from(e: rusqlite::Error) -> Self {
        ApiKeyError::Db(DbError::Sql(e))
    }
}

fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Constant-time comparison of two equal-length hex strings.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// `lk-<id>.<secret>` → `id`.
pub fn key_id(token: &str) -> Option<&str> {
    let rest = token.strip_prefix(KEY_PREFIX)?;
    let (id, secret) = rest.split_once('.')?;
    (!id.is_empty() && !secret.is_empty()).then_some(id)
}

fn random_hex(bytes: usize) -> Result<String, ApiKeyError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| ApiKeyError::Random(e.to_string()))?;
    Ok(hex::encode(buf))
}

pub struct ApiKeys<'a> {
    db: &'a Db,
}

const COLUMNS: &str = "id, name, agent, created_at, last_used_at, requests";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ApiKey> {
    Ok(ApiKey {
        id: r.get(0)?,
        name: r.get(1)?,
        agent: r.get(2)?,
        created_at: r.get(3)?,
        last_used_at: r.get(4)?,
        requests: r.get(5)?,
    })
}

impl<'a> ApiKeys<'a> {
    pub fn new(db: &'a Db) -> Self {
        ApiKeys { db }
    }

    /// Makes a key; returns it with the whole secret, shown to the user once.
    pub fn create(&self, name: &str, agent: Option<&str>) -> Result<(ApiKey, String), ApiKeyError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return Err(ApiKeyError::BadName);
        }
        let id = random_hex(6)?;
        let token = format!("{KEY_PREFIX}{id}.{}", random_hex(24)?);
        let now = now_secs();
        self.db.conn().execute(
            "INSERT INTO api_keys (id, name, hash, agent, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, name, hash(&token), agent, now],
        )?;
        tracing::info!(key_id = %id, agent = ?agent, "server access key created");
        Ok((
            ApiKey {
                id,
                name: name.to_owned(),
                agent: agent.map(str::to_owned),
                created_at: now,
                last_used_at: None,
                requests: 0,
            },
            token,
        ))
    }

    pub fn list(&self) -> Result<Vec<ApiKey>, ApiKeyError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM api_keys ORDER BY created_at DESC, id"
        ))?;
        let rows = stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get(&self, id: &str) -> Result<Option<ApiKey>, ApiKeyError> {
        Ok(self
            .db
            .conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM api_keys WHERE id = ?1"),
                params![id],
                row,
            )
            .optional()?)
    }

    /// The key of `agent`, if one was made for it.
    pub fn for_agent(&self, agent: &str) -> Result<Option<ApiKey>, ApiKeyError> {
        Ok(self
            .db
            .conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM api_keys WHERE agent = ?1 ORDER BY created_at DESC LIMIT 1"),
                params![agent],
                row,
            )
            .optional()?)
    }

    pub fn revoke(&self, id: &str) -> Result<bool, ApiKeyError> {
        let n = self
            .db
            .conn()
            .execute("DELETE FROM api_keys WHERE id = ?1", params![id])?;
        self.db
            .conn()
            .execute("DELETE FROM mcp_clients WHERE key_id = ?1", params![id])?;
        if n > 0 {
            tracing::info!(key_id = %id, "server access key revoked");
        }
        Ok(n > 0)
    }

    /// The key a bearer token belongs to, or `None` for a wrong or revoked one.
    pub fn verify(&self, token: &str) -> Result<Option<ApiKey>, ApiKeyError> {
        let Some(id) = key_id(token) else {
            return Ok(None);
        };
        let stored: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT hash FROM api_keys WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        match stored {
            Some(h) if same(&h, &hash(token)) => self.get(id),
            _ => Ok(None),
        }
    }

    /// Counts a request made with key `id`.
    pub fn touch(&self, id: &str) -> Result<(), ApiKeyError> {
        self.db.conn().execute(
            "UPDATE api_keys SET last_used_at = ?2, requests = requests + 1 WHERE id = ?1",
            params![id, now_secs()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_verify_only_with_the_whole_secret() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("app.db")).unwrap();
        let keys = ApiKeys::new(&db);
        let (key, token) = keys.create("Goose", Some("goose")).unwrap();
        assert!(token.starts_with("lk-"));
        assert_eq!(key_id(&token), Some(key.id.as_str()));
        assert_eq!(keys.verify(&token).unwrap().unwrap().id, key.id);
        assert!(keys.verify(&format!("{token}x")).unwrap().is_none());
        assert!(keys.verify("lk-nope.secret").unwrap().is_none());
        assert!(keys.verify("Bearer junk").unwrap().is_none());
        keys.touch(&key.id).unwrap();
        let listed = keys.list().unwrap();
        assert_eq!(listed[0].requests, 1);
        assert!(listed[0].last_used_at.is_some());
        assert_eq!(keys.for_agent("goose").unwrap().unwrap().id, key.id);
        assert!(keys.revoke(&key.id).unwrap());
        assert!(keys.verify(&token).unwrap().is_none());
        assert!(matches!(keys.create("  ", None), Err(ApiKeyError::BadName)));
    }
}

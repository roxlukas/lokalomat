//! MCP clients the user allowed to use the local server (PRD §15 point 15, ADR 0034).
//!
//! A client is an MCP program (Claude Code, Claude Desktop...) that connects with
//! one of the access keys. The first time a client introduces itself with a key,
//! the app asks the user; an allowed client is remembered here until the user
//! revokes it. The protocol has no sessions (ADR 0033), so later requests are
//! checked against the key: a key with no allowed client cannot use MCP.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError, now_secs};

/// Longest client name or version kept (they come from the client itself).
const MAX_TEXT: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpClient {
    pub key_id: String,
    /// Name of the access key the client uses.
    pub key_name: String,
    /// What the client calls itself (`clientInfo.name`).
    pub name: String,
    pub version: Option<String>,
    pub allowed_at: i64,
    pub last_seen_at: Option<i64>,
}

/// A name from a client: no control characters, not too long, never empty.
pub fn clean(text: &str) -> String {
    let t: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect();
    let t = t.trim();
    if t.is_empty() {
        "?".to_owned()
    } else {
        t.to_owned()
    }
}

pub struct McpClients<'a> {
    db: &'a Db,
}

impl<'a> McpClients<'a> {
    pub fn new(db: &'a Db) -> Self {
        McpClients { db }
    }

    /// Whether this client was allowed with this key before.
    pub fn is_known(&self, key_id: &str, name: &str) -> Result<bool, DbError> {
        Ok(self.db.conn().query_row(
            "SELECT COUNT(*) FROM mcp_clients WHERE key_id = ?1 AND name = ?2",
            params![key_id, clean(name)],
            |r| r.get::<_, i64>(0),
        )? > 0)
    }

    /// Whether any client is allowed with this key.
    pub fn has_access(&self, key_id: &str) -> Result<bool, DbError> {
        Ok(self.db.conn().query_row(
            "SELECT COUNT(*) FROM mcp_clients WHERE key_id = ?1",
            params![key_id],
            |r| r.get::<_, i64>(0),
        )? > 0)
    }

    /// Remembers an allowed client (or refreshes its version).
    pub fn allow(&self, key_id: &str, name: &str, version: Option<&str>) -> Result<(), DbError> {
        let now = now_secs();
        self.db.conn().execute(
            "INSERT INTO mcp_clients (key_id, name, version, allowed_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT (key_id, name) DO UPDATE SET version = ?3, last_seen_at = ?4",
            params![key_id, clean(name), version.map(clean), now],
        )?;
        tracing::info!(key_id, client = %clean(name), "MCP client allowed");
        Ok(())
    }

    /// A request was made with this key.
    pub fn seen(&self, key_id: &str) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE mcp_clients SET last_seen_at = ?2 WHERE key_id = ?1",
            params![key_id, now_secs()],
        )?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<McpClient>, DbError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT c.key_id, COALESCE(k.name, ''), c.name, c.version, c.allowed_at, c.last_seen_at
             FROM mcp_clients c LEFT JOIN api_keys k ON k.id = c.key_id
             ORDER BY c.allowed_at DESC, c.name",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(McpClient {
                    key_id: r.get(0)?,
                    key_name: r.get(1)?,
                    name: r.get(2)?,
                    version: r.get(3)?,
                    allowed_at: r.get(4)?,
                    last_seen_at: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Withdraws a client's permission; it is asked about again next time.
    pub fn revoke(&self, key_id: &str, name: &str) -> Result<bool, DbError> {
        let n = self.db.conn().execute(
            "DELETE FROM mcp_clients WHERE key_id = ?1 AND name = ?2",
            params![key_id, name],
        )?;
        if n > 0 {
            tracing::info!(key_id, client = %clean(name), "MCP client revoked");
        }
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_keys::ApiKeys;

    #[test]
    fn clients_are_remembered_until_revoked() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("app.db")).unwrap();
        let (key, _) = ApiKeys::new(&db).create("Laptop", None).unwrap();
        let clients = McpClients::new(&db);
        assert!(!clients.has_access(&key.id).unwrap());
        assert!(!clients.is_known(&key.id, "claude-code").unwrap());
        clients.allow(&key.id, "claude-code", Some("2.1")).unwrap();
        assert!(clients.is_known(&key.id, "claude-code").unwrap());
        assert!(!clients.is_known(&key.id, "other").unwrap());
        assert!(clients.has_access(&key.id).unwrap());
        // Allowing again only refreshes it.
        clients.allow(&key.id, "claude-code", Some("2.2")).unwrap();
        clients.seen(&key.id).unwrap();
        let list = clients.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].version.as_deref(), Some("2.2"));
        assert_eq!(list[0].key_name, "Laptop");
        assert!(list[0].last_seen_at.is_some());
        assert!(clients.revoke(&key.id, "claude-code").unwrap());
        assert!(!clients.revoke(&key.id, "claude-code").unwrap());
        assert!(!clients.has_access(&key.id).unwrap());
    }

    #[test]
    fn revoking_a_key_removes_its_clients() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("app.db")).unwrap();
        let keys = ApiKeys::new(&db);
        let (key, _) = keys.create("Laptop", None).unwrap();
        McpClients::new(&db).allow(&key.id, "x", None).unwrap();
        keys.revoke(&key.id).unwrap();
        assert!(McpClients::new(&db).list().unwrap().is_empty());
    }

    #[test]
    fn names_from_clients_are_cleaned() {
        assert_eq!(clean("  Claude\u{7} Code\n "), "Claude Code");
        assert_eq!(clean("\u{7}"), "?");
        assert_eq!(clean(&"x".repeat(500)).len(), MAX_TEXT);
    }
}

//! Chat sessions and messages (Workshop V2, PRD §8.1). An assistant message
//! doubles as the persisted record of its chat job: status, error, timings.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError, now_secs};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "system" => Role::System,
            "assistant" => Role::Assistant,
            _ => Role::User,
        }
    }
}

/// Job status (PRD §8.1) as stored on the assistant message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Queued,
    Preparing,
    LoadingModel,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl MessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageStatus::Queued => "queued",
            MessageStatus::Preparing => "preparing",
            MessageStatus::LoadingModel => "loading_model",
            MessageStatus::Running => "running",
            MessageStatus::Done => "done",
            MessageStatus::Failed => "failed",
            MessageStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "queued" => MessageStatus::Queued,
            "preparing" => MessageStatus::Preparing,
            "loading_model" => MessageStatus::LoadingModel,
            "running" => MessageStatus::Running,
            "failed" => MessageStatus::Failed,
            "cancelled" => MessageStatus::Cancelled,
            _ => MessageStatus::Done,
        }
    }

    pub fn is_finished(self) -> bool {
        matches!(
            self,
            MessageStatus::Done | MessageStatus::Failed | MessageStatus::Cancelled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: i64,
    pub title: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub session_id: i64,
    pub role: Role,
    pub content: String,
    pub reasoning: String,
    pub model_id: Option<String>,
    pub variant_id: Option<String>,
    pub status: MessageStatus,
    pub error: Option<String>,
    pub tokens: Option<i64>,
    pub tokens_per_second: Option<f64>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    /// Tool calls made while answering (JSON array, see `lokalsi-engines`).
    pub tool_calls: serde_json::Value,
}

const MESSAGE_COLUMNS: &str = "id, session_id, role, content, reasoning, model_id, variant_id, status, error, tokens, tokens_per_second, created_at, finished_at, tool_calls";

fn row_to_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get(0)?,
        session_id: r.get(1)?,
        role: Role::parse(&r.get::<_, String>(2)?),
        content: r.get(3)?,
        reasoning: r.get(4)?,
        model_id: r.get(5)?,
        variant_id: r.get(6)?,
        status: MessageStatus::parse(&r.get::<_, String>(7)?),
        error: r.get(8)?,
        tokens: r.get(9)?,
        tokens_per_second: r.get(10)?,
        created_at: r.get(11)?,
        finished_at: r.get(12)?,
        tool_calls: serde_json::from_str(&r.get::<_, String>(13)?)
            .unwrap_or_else(|_| serde_json::json!([])),
    })
}

/// Session title from the first user message: first line, at most 60 chars.
pub fn title_from(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    title
}

pub struct Chats<'a> {
    db: &'a Db,
}

impl<'a> Chats<'a> {
    pub fn new(db: &'a Db) -> Self {
        Chats { db }
    }

    pub fn create_session(&self, title: &str) -> Result<Session, DbError> {
        let now = now_secs();
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO sessions (title, pinned, created_at, updated_at) VALUES (?1, 0, ?2, ?2)",
            params![title, now],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.session(id)?
            .ok_or(DbError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    pub fn session(&self, id: i64) -> Result<Option<Session>, DbError> {
        Ok(self
            .db
            .conn()
            .query_row(
                "SELECT s.id, s.title, s.pinned, s.created_at, s.updated_at,
                        (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id)
                 FROM sessions s WHERE s.id = ?1",
                params![id],
                row_to_session,
            )
            .optional()?)
    }

    /// Pinned first, then most recently active; optional case-insensitive
    /// search in titles and message contents.
    pub fn list_sessions(&self, query: Option<&str>) -> Result<Vec<Session>, DbError> {
        let conn = self.db.conn();
        let pattern = query
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(|q| format!("%{}%", q.replace('%', "\\%").replace('_', "\\_")));
        let mut stmt = conn.prepare(
            "SELECT s.id, s.title, s.pinned, s.created_at, s.updated_at,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id)
             FROM sessions s
             WHERE ?1 IS NULL OR s.title LIKE ?1 ESCAPE '\\'
                OR EXISTS (SELECT 1 FROM messages m WHERE m.session_id = s.id AND m.content LIKE ?1 ESCAPE '\\')
             ORDER BY s.pinned DESC, s.updated_at DESC, s.id DESC",
        )?;
        let rows = stmt.query_map(params![pattern], row_to_session)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn rename_session(&self, id: i64, title: &str) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE sessions SET title = ?2 WHERE id = ?1",
            params![id, title.trim()],
        )?;
        Ok(())
    }

    pub fn pin_session(&self, id: i64, pinned: bool) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE sessions SET pinned = ?2 WHERE id = ?1",
            params![id, pinned as i64],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, id: i64) -> Result<(), DbError> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn add_message(
        &self,
        session_id: i64,
        role: Role,
        content: &str,
        model: Option<(&str, &str)>,
        status: MessageStatus,
    ) -> Result<Message, DbError> {
        let now = now_secs();
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, model_id, variant_id, status, created_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session_id,
                role.as_str(),
                content,
                model.map(|m| m.0),
                model.map(|m| m.1),
                status.as_str(),
                now,
                status.is_finished().then_some(now)
            ],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
            params![session_id, now],
        )?;
        drop(conn);
        self.message(id)?
            .ok_or(DbError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    pub fn message(&self, id: i64) -> Result<Option<Message>, DbError> {
        Ok(self
            .db
            .conn()
            .query_row(
                &format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"),
                params![id],
                row_to_message,
            )
            .optional()?)
    }

    pub fn messages(&self, session_id: i64) -> Result<Vec<Message>, DbError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE session_id = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map(params![session_id], row_to_message)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_status(
        &self,
        id: i64,
        status: MessageStatus,
        error: Option<&str>,
    ) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE messages SET status = ?2, error = ?3,
                finished_at = CASE WHEN ?4 THEN ?5 ELSE finished_at END
             WHERE id = ?1",
            params![id, status.as_str(), error, status.is_finished(), now_secs()],
        )?;
        Ok(())
    }

    /// Saves the generated text (also partial text on cancel/failure).
    pub fn save_output(
        &self,
        id: i64,
        content: &str,
        reasoning: &str,
        tokens: Option<i64>,
        tokens_per_second: Option<f64>,
    ) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE messages SET content = ?2, reasoning = ?3, tokens = ?4, tokens_per_second = ?5 WHERE id = ?1",
            params![id, content, reasoning, tokens, tokens_per_second],
        )?;
        Ok(())
    }

    pub fn save_tool_calls(&self, id: i64, calls: &serde_json::Value) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE messages SET tool_calls = ?2 WHERE id = ?1",
            params![id, calls.to_string()],
        )?;
        Ok(())
    }

    /// Messages left unfinished by a crash or kill are marked failed at startup.
    pub fn fail_unfinished(&self, error: &str) -> Result<usize, DbError> {
        Ok(self.db.conn().execute(
            "UPDATE messages SET status = 'failed', error = ?1, finished_at = ?2
             WHERE status IN ('queued', 'preparing', 'loading_model', 'running')",
            params![error, now_secs()],
        )?)
    }
}

fn row_to_session(r: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        title: r.get(1)?,
        pinned: r.get::<_, i64>(2)? != 0,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        messages: r.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_and_messages_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let chats = Chats::new(&db);
        let a = chats.create_session("Pierwsza").unwrap();
        let b = chats.create_session("Druga").unwrap();
        chats
            .add_message(
                a.id,
                Role::User,
                "Jak działa RAG?",
                None,
                MessageStatus::Done,
            )
            .unwrap();
        let reply = chats
            .add_message(
                a.id,
                Role::Assistant,
                "",
                Some(("qwen3.5-4b", "q4_k_m")),
                MessageStatus::Queued,
            )
            .unwrap();
        chats
            .save_output(reply.id, "Odpowiedź", "myślenie", Some(12), Some(40.5))
            .unwrap();
        chats
            .set_status(reply.id, MessageStatus::Done, None)
            .unwrap();

        let msgs = chats.messages(a.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "Odpowiedź");
        assert!(msgs[1].finished_at.is_some());

        chats.pin_session(b.id, true).unwrap();
        let list = chats.list_sessions(None).unwrap();
        assert_eq!(list[0].id, b.id, "pinned first");
        assert_eq!(
            chats.list_sessions(Some("rag")).unwrap().len(),
            1,
            "search in content"
        );
        assert_eq!(
            chats.list_sessions(Some("100%")).unwrap().len(),
            0,
            "wildcards escaped"
        );

        chats.delete_session(a.id).unwrap();
        assert!(chats.messages(a.id).unwrap().is_empty());
    }

    #[test]
    fn unfinished_messages_fail_after_restart() {
        let db = Db::open_in_memory().unwrap();
        let chats = Chats::new(&db);
        let s = chats.create_session("x").unwrap();
        chats
            .add_message(s.id, Role::Assistant, "", None, MessageStatus::Running)
            .unwrap();
        assert_eq!(chats.fail_unfinished("interrupted").unwrap(), 1);
        assert_eq!(
            chats.messages(s.id).unwrap()[0].status,
            MessageStatus::Failed
        );
    }

    #[test]
    fn titles_are_short_first_lines() {
        assert_eq!(title_from("\n  Napisz wiersz\nо kotach"), "Napisz wiersz");
        assert_eq!(title_from(&"a".repeat(80)).chars().count(), 61);
    }
}

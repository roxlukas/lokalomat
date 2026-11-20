//! Transcription, image and speech jobs with their results (PRD §8.1, §4.5). The
//! Gallery (V3) lists finished jobs; chat jobs stay on their messages.

use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError, now_secs};

pub use crate::chats::MessageStatus as JobStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Image,
    Stt,
    /// Speech synthesis (M4).
    Tts,
    Music,
    /// Typed decision (ADR 0024): the answers are in `outputs`, no files.
    Decision,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Image => "image",
            JobKind::Stt => "stt",
            JobKind::Tts => "tts",
            JobKind::Music => "music",
            JobKind::Decision => "decision",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "image" => Some(JobKind::Image),
            "stt" => Some(JobKind::Stt),
            "tts" => Some(JobKind::Tts),
            "music" => Some(JobKind::Music),
            "decision" => Some(JobKind::Decision),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: i64,
    pub kind: JobKind,
    pub status: JobStatus,
    /// 0–1; `None` = unknown.
    pub progress: Option<f64>,
    pub model_id: Option<String>,
    pub variant_id: Option<String>,
    pub model_name: String,
    pub engine: String,
    pub params: serde_json::Value,
    pub inputs: serde_json::Value,
    pub outputs: serde_json::Value,
    /// Searchable text: the image prompt, the transcript or the spoken text.
    pub text: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub duration_ms: Option<i64>,
}

/// What to create: everything known before the job runs.
#[derive(Debug, Clone, Default)]
pub struct NewJob<'a> {
    pub model_id: Option<&'a str>,
    pub variant_id: Option<&'a str>,
    pub model_name: &'a str,
    pub engine: &'a str,
    pub params: serde_json::Value,
    pub inputs: serde_json::Value,
    pub text: &'a str,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct JobFilter {
    pub kind: Option<JobKind>,
    pub query: Option<String>,
    pub model_id: Option<String>,
    /// Only jobs that finished successfully (Gallery).
    #[serde(default)]
    pub done_only: bool,
    /// Only jobs with media results (Gallery): leaves out decisions.
    #[serde(default)]
    pub media_only: bool,
    /// Created at or after (unix seconds).
    pub since: Option<i64>,
    pub limit: Option<u32>,
}

const COLUMNS: &str = "id, kind, status, progress, model_id, variant_id, model_name, engine, params, inputs, outputs, text, error, created_at, started_at, finished_at, duration_ms";

fn json(s: String) -> serde_json::Value {
    serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)
}

fn row_to_job(r: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        kind: JobKind::parse(&r.get::<_, String>(1)?).unwrap_or(JobKind::Image),
        status: JobStatus::parse(&r.get::<_, String>(2)?),
        progress: r.get(3)?,
        model_id: r.get(4)?,
        variant_id: r.get(5)?,
        model_name: r.get(6)?,
        engine: r.get(7)?,
        params: json(r.get(8)?),
        inputs: json(r.get(9)?),
        outputs: json(r.get(10)?),
        text: r.get(11)?,
        error: r.get(12)?,
        created_at: r.get(13)?,
        started_at: r.get(14)?,
        finished_at: r.get(15)?,
        duration_ms: r.get(16)?,
    })
}

pub struct Jobs<'a> {
    db: &'a Db,
}

impl<'a> Jobs<'a> {
    pub fn new(db: &'a Db) -> Self {
        Jobs { db }
    }

    pub fn create(&self, kind: JobKind, new: &NewJob<'_>) -> Result<Job, DbError> {
        let id = {
            let conn = self.db.conn();
            conn.execute(
                "INSERT INTO jobs (kind, status, model_id, variant_id, model_name, engine, params, inputs, text, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    kind.as_str(),
                    JobStatus::Queued.as_str(),
                    new.model_id,
                    new.variant_id,
                    new.model_name,
                    new.engine,
                    new.params.to_string(),
                    new.inputs.to_string(),
                    new.text,
                    now_secs()
                ],
            )?;
            conn.last_insert_rowid()
        };
        self.get(id)?
            .ok_or_else(|| DbError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    pub fn get(&self, id: i64) -> Result<Option<Job>, DbError> {
        Ok(self
            .db
            .conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM jobs WHERE id = ?1"),
                params![id],
                row_to_job,
            )
            .optional()?)
    }

    /// Sets the status; records the start of running and the end of a finished job.
    pub fn set_status(
        &self,
        id: i64,
        status: JobStatus,
        error: Option<&str>,
    ) -> Result<(), DbError> {
        let now = now_secs();
        let conn = self.db.conn();
        conn.execute(
            "UPDATE jobs SET status = ?2, error = ?3,
                started_at = CASE WHEN ?2 = 'running' AND started_at IS NULL THEN ?4 ELSE started_at END,
                finished_at = CASE WHEN ?5 THEN ?4 ELSE finished_at END,
                progress = CASE WHEN ?2 = 'done' THEN 1.0 ELSE progress END
             WHERE id = ?1",
            params![id, status.as_str(), error, now, status.is_finished()],
        )?;
        Ok(())
    }

    /// Records where a job came from (`inputs.source`, PRD §4.5), e.g. a pipeline run.
    pub fn set_source(&self, id: i64, source: &serde_json::Value) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE jobs SET inputs = json_set(inputs, '$.source', json(?2)) WHERE id = ?1",
            params![id, source.to_string()],
        )?;
        Ok(())
    }

    pub fn set_progress(&self, id: i64, progress: Option<f64>) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE jobs SET progress = ?2 WHERE id = ?1",
            params![id, progress],
        )?;
        Ok(())
    }

    /// Stores results; `text` replaces the searchable text when given.
    pub fn save_outputs(
        &self,
        id: i64,
        outputs: &serde_json::Value,
        text: Option<&str>,
        duration_ms: Option<i64>,
    ) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE jobs SET outputs = ?2, text = COALESCE(?3, text), duration_ms = ?4 WHERE id = ?1",
            params![id, outputs.to_string(), text, duration_ms],
        )?;
        Ok(())
    }

    pub fn list(&self, filter: &JobFilter) -> Result<Vec<Job>, DbError> {
        let mut sql = format!("SELECT {COLUMNS} FROM jobs WHERE 1 = 1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(kind) = filter.kind {
            sql.push_str(" AND kind = ?");
            args.push(kind.as_str().to_owned().into());
        }
        if let Some(model) = filter.model_id.as_deref().filter(|m| !m.is_empty()) {
            sql.push_str(" AND model_id = ?");
            args.push(model.to_owned().into());
        }
        if let Some(q) = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
        {
            sql.push_str(" AND (text LIKE ? ESCAPE '\\' OR model_name LIKE ? ESCAPE '\\')");
            let like = format!(
                "%{}%",
                q.replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            args.push(like.clone().into());
            args.push(like.into());
        }
        if filter.done_only {
            sql.push_str(" AND status = 'done'");
        }
        if filter.media_only {
            sql.push_str(" AND kind != 'decision'");
        }
        if let Some(since) = filter.since {
            sql.push_str(" AND created_at >= ?");
            args.push(since.into());
        }
        sql.push_str(" ORDER BY id DESC LIMIT ?");
        args.push(i64::from(filter.limit.unwrap_or(200).min(1000)).into());
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&sql)?;
        let jobs = stmt
            .query_map(params_from_iter(args), row_to_job)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    /// Deletes the row and returns it, so the caller can remove its files.
    pub fn delete(&self, id: i64) -> Result<Option<Job>, DbError> {
        let job = self.get(id)?;
        self.db
            .conn()
            .execute("DELETE FROM jobs WHERE id = ?1", params![id])?;
        Ok(job)
    }

    /// Marks jobs left unfinished by a previous run (app killed) as failed.
    pub fn fail_unfinished(&self, code: &str) -> Result<usize, DbError> {
        Ok(self.db.conn().execute(
            "UPDATE jobs SET status = 'failed', error = ?1, finished_at = ?2
             WHERE status NOT IN ('done', 'failed', 'cancelled')",
            params![code, now_secs()],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_job<'a>(text: &'a str) -> NewJob<'a> {
        NewJob {
            model_id: Some("z-image-turbo"),
            variant_id: Some("q4_k"),
            model_name: "Z-Image Turbo Q4_K",
            engine: "sd-cpp",
            params: serde_json::json!({ "width": 1024 }),
            inputs: serde_json::json!({}),
            text,
        }
    }

    #[test]
    fn lifecycle_and_gallery_filters() {
        let db = Db::open_in_memory().unwrap();
        let jobs = Jobs::new(&db);
        let a = jobs.create(JobKind::Image, &new_job("a red fox")).unwrap();
        assert_eq!(a.status, JobStatus::Queued);
        assert_eq!(a.params["width"], 1024);
        jobs.set_status(a.id, JobStatus::Running, None).unwrap();
        jobs.set_progress(a.id, Some(0.5)).unwrap();
        let running = jobs.get(a.id).unwrap().unwrap();
        assert!(running.started_at.is_some() && running.finished_at.is_none());
        assert_eq!(running.progress, Some(0.5));
        jobs.save_outputs(
            a.id,
            &serde_json::json!({ "files": ["x.png"] }),
            None,
            Some(1200),
        )
        .unwrap();
        jobs.set_status(a.id, JobStatus::Done, None).unwrap();
        let done = jobs.get(a.id).unwrap().unwrap();
        assert_eq!(done.progress, Some(1.0));
        assert!(done.finished_at.is_some());
        assert_eq!(done.outputs["files"][0], "x.png");

        let b = jobs
            .create(
                JobKind::Stt,
                &NewJob {
                    text: "",
                    ..new_job("")
                },
            )
            .unwrap();
        jobs.save_outputs(b.id, &serde_json::json!({}), Some("dzień dobry 100%"), None)
            .unwrap();

        let all = jobs.list(&JobFilter::default()).unwrap();
        assert_eq!(all.iter().map(|j| j.id).collect::<Vec<_>>(), [b.id, a.id]);
        let done_only = jobs
            .list(&JobFilter {
                done_only: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(done_only.len(), 1);
        let found = jobs
            .list(&JobFilter {
                query: Some("100%".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(found.len(), 1, "% is matched literally");
        let fox = jobs
            .list(&JobFilter {
                kind: Some(JobKind::Image),
                query: Some("FOX".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(fox.len(), 1);

        let d = jobs
            .create(JobKind::Decision, &new_job("czy to spam?"))
            .unwrap();
        assert_eq!(jobs.get(d.id).unwrap().unwrap().kind, JobKind::Decision);
        let media = jobs
            .list(&JobFilter {
                media_only: true,
                ..Default::default()
            })
            .unwrap();
        assert!(media.iter().all(|j| j.kind != JobKind::Decision));
        assert_eq!(media.len(), 2);
        jobs.delete(d.id).unwrap();

        jobs.set_source(b.id, &serde_json::json!({ "pipeline_run": 3 }))
            .unwrap();
        assert_eq!(
            jobs.get(b.id).unwrap().unwrap().inputs["source"]["pipeline_run"],
            3
        );
        assert_eq!(jobs.fail_unfinished("interrupted").unwrap(), 1);
        assert_eq!(jobs.get(b.id).unwrap().unwrap().status, JobStatus::Failed);
        assert!(jobs.delete(a.id).unwrap().is_some());
        assert!(jobs.get(a.id).unwrap().is_none());
    }
}

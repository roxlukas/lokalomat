//! Pipelines (PRD §8.2, view V7): a linear list of steps. Every step is a
//! mode (transcribe, chat, speak, image, decide) with typed ports; its input comes
//! from the pipeline input or from any earlier step. This module holds the
//! definitions, type validation, built-in templates and storage. Running a
//! pipeline lives in `lokalsi_engines_media::pipeline` (job queue + VRAM scheduler).

use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::catalog::Modality;
pub use crate::catalog::ModelRef;
use crate::db::{Db, DbError, now_secs};
use crate::decisions::{self, Gate, Question};

pub use crate::chats::MessageStatus as RunStatus;

/// Most steps a pipeline may have (keeps runs and the editor manageable).
pub const MAX_STEPS: usize = 12;
/// Longest prompt template of a chat step.
pub const MAX_PROMPT_CHARS: usize = 8_000;

/// Data flowing between steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Port {
    Text,
    Audio,
    Image,
    /// Text with timestamps (a transcript); usable wherever text is.
    Subtitles,
}

impl Port {
    /// True when data of this type can feed a step expecting `into`.
    pub fn feeds(self, into: Port) -> bool {
        self == into || (self == Port::Subtitles && into == Port::Text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepMode {
    /// Speech to text (whisper.cpp): audio → subtitles.
    Transcribe,
    /// Language model (llama.cpp): text → text, with a prompt template.
    Chat,
    /// Text to speech (Python worker): text → audio.
    Speak,
    /// Text to image (stable-diffusion.cpp): text → image.
    Image,
    /// Decision model (ADR 0024): text → the chosen answer; a gate may stop the run.
    Decide,
}

impl StepMode {
    pub fn input(self) -> Port {
        match self {
            StepMode::Transcribe => Port::Audio,
            StepMode::Chat | StepMode::Speak | StepMode::Image | StepMode::Decide => Port::Text,
        }
    }

    pub fn output(self) -> Port {
        match self {
            StepMode::Transcribe => Port::Subtitles,
            StepMode::Chat | StepMode::Decide => Port::Text,
            StepMode::Speak => Port::Audio,
            StepMode::Image => Port::Image,
        }
    }

    pub fn modality(self) -> Modality {
        match self {
            StepMode::Transcribe => Modality::Stt,
            StepMode::Chat => Modality::Llm,
            StepMode::Speak => Modality::Tts,
            StepMode::Image => Modality::Image,
            StepMode::Decide => Modality::Decision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub mode: StepMode,
    /// `None` = the default model of the mode.
    #[serde(default)]
    pub model: Option<ModelRef>,
    /// Chat: prompt template with `{{input}}`.
    #[serde(default)]
    pub prompt: Option<String>,
    /// Where the input comes from: `0` = pipeline input, `n` = output of
    /// step `n` (1-based). `None` = the previous step (pipeline input for the first).
    #[serde(default)]
    pub from: Option<u32>,
    /// Transcribe: spoken language (`auto`); Speak: language of the text.
    #[serde(default)]
    pub language: Option<String>,
    /// Speak: voice profile from the library (V6).
    #[serde(default)]
    pub voice_id: Option<i64>,
    /// Image: aspect ratio (`1:1`, `3:4`, `16:9`).
    #[serde(default)]
    pub aspect: Option<String>,
    /// Decide: the question (its `id` is not shown; the answer is the output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<Question>,
    /// Decide: continue only on this answer; `None` = always continue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
}

impl Step {
    pub fn new(mode: StepMode) -> Self {
        Step {
            mode,
            model: None,
            prompt: None,
            from: None,
            language: None,
            voice_id: None,
            aspect: None,
            question: None,
            gate: None,
        }
    }

    /// Source of step `index` (0-based) as `0` = pipeline input or `n` = step `n`.
    pub fn source(&self, index: usize) -> u32 {
        self.from.unwrap_or(index as u32)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pipeline {
    /// Saved pipelines only.
    #[serde(default)]
    pub id: Option<i64>,
    pub name: String,
    /// Built-in template this pipeline came from.
    #[serde(default)]
    pub template: Option<String>,
    pub steps: Vec<Step>,
}

/// A problem found by [`validate`]; `step` is 0-based, `None` = whole pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub step: Option<usize>,
    /// `no_steps`, `too_many_steps`, `no_name`, `bad_source`, `type_mismatch`,
    /// `input_mismatch`, `prompt_too_long`, `no_question`, `bad_question`,
    /// `bad_gate` (`pipelines.issues.*` in the UI).
    pub code: String,
}

impl Issue {
    fn at(step: usize, code: &str) -> Self {
        Issue {
            step: Some(step),
            code: code.to_owned(),
        }
    }

    fn whole(code: &str) -> Self {
        Issue {
            step: None,
            code: code.to_owned(),
        }
    }
}

/// Type of the output a step reads: the pipeline input (`None`) or step `n`'s output.
fn source_port(steps: &[Step], source: u32) -> Option<Port> {
    match source {
        0 => None,
        n => steps.get(n as usize - 1).map(|s| s.mode.output()),
    }
}

/// What the pipeline needs as its input: the input port of the steps that
/// read it. `None` when no step reads the pipeline input.
pub fn input_port(pipeline: &Pipeline) -> Option<Port> {
    pipeline
        .steps
        .iter()
        .enumerate()
        .find(|(i, s)| s.source(*i) == 0)
        .map(|(_, s)| s.mode.input())
}

/// Checks a pipeline before it runs and while it is edited (red arrows in V7).
pub fn validate(pipeline: &Pipeline) -> Vec<Issue> {
    let mut issues = Vec::new();
    if pipeline.name.trim().is_empty() {
        issues.push(Issue::whole("no_name"));
    }
    if pipeline.steps.is_empty() {
        issues.push(Issue::whole("no_steps"));
        return issues;
    }
    if pipeline.steps.len() > MAX_STEPS {
        issues.push(Issue::whole("too_many_steps"));
    }
    let input = input_port(pipeline);
    for (i, step) in pipeline.steps.iter().enumerate() {
        let source = step.source(i);
        if source as usize > i {
            issues.push(Issue::at(i, "bad_source"));
            continue;
        }
        match source_port(&pipeline.steps, source) {
            Some(port) if !port.feeds(step.mode.input()) => {
                issues.push(Issue::at(i, "type_mismatch"));
            }
            None if input.is_some_and(|p| p != step.mode.input()) => {
                issues.push(Issue::at(i, "input_mismatch"));
            }
            _ => {}
        }
        if step
            .prompt
            .as_deref()
            .is_some_and(|p| p.chars().count() > MAX_PROMPT_CHARS)
        {
            issues.push(Issue::at(i, "prompt_too_long"));
        }
        if step.mode == StepMode::Decide {
            match &step.question {
                None => issues.push(Issue::at(i, "no_question")),
                Some(q) if decisions::validate("-", std::slice::from_ref(q)).is_err() => {
                    issues.push(Issue::at(i, "bad_question"));
                }
                Some(q) => {
                    if step.gate.as_ref().is_some_and(|g| !g.fits(q)) {
                        issues.push(Issue::at(i, "bad_gate"));
                    }
                }
            }
        }
    }
    issues
}

pub const INPUT_VAR: &str = "{{input}}";

/// The message a chat step sends: the template with `{{input}}` replaced; a
/// template without the variable gets the input appended.
pub fn render_prompt(template: Option<&str>, input: &str) -> String {
    match template.map(str::trim).filter(|t| !t.is_empty()) {
        None => input.to_owned(),
        Some(t) if t.contains(INPUT_VAR) => t.replace(INPUT_VAR, input),
        Some(t) => format!("{t}\n\n{input}"),
    }
}

/// Built-in templates (PRD §8.2) with prompts in the UI language (`pl`/`en`).
pub fn templates(lang: &str) -> Vec<Pipeline> {
    let pl = lang != "en";
    let chat = |prompt: &str| Step {
        prompt: Some(prompt.to_owned()),
        ..Step::new(StepMode::Chat)
    };
    let summary = if pl {
        "Streść poniższe nagranie w kilku punktach, po polsku. Napisz tylko streszczenie.\n\n{{input}}"
    } else {
        "Summarise the recording below in a few bullet points. Write only the summary.\n\n{{input}}"
    };
    let translate = if pl {
        "Przetłumacz poniższy tekst na język angielski. Odpowiedz wyłącznie tłumaczeniem, bez komentarzy.\n\n{{input}}"
    } else {
        "Translate the text below into Polish. Reply with the translation only, no comments.\n\n{{input}}"
    };
    let image_prompt = if pl {
        "Napisz jedno zdanie po angielsku opisujące ilustrację do poniższego tekstu: scena, styl, światło. Odpowiedz tylko tym zdaniem.\n\n{{input}}"
    } else {
        "Write one sentence describing an illustration for the text below: scene, style, lighting. Reply with that sentence only.\n\n{{input}}"
    };
    let narration = if pl {
        "Napisz krótką narrację (3–4 zdania) do przeczytania przez lektora na podstawie poniższego tekstu. Odpowiedz tylko narracją, bez nagłówków i formatowania.\n\n{{input}}"
    } else {
        "Write a short narration (3–4 sentences) for a voice-over based on the text below. Reply with the narration only, no headings or formatting.\n\n{{input}}"
    };
    let (target_lang, own_lang) = if pl { ("en", "pl") } else { ("pl", "en") };
    let topics: Vec<String> = if pl {
        vec![
            "praca",
            "rodzina i znajomi",
            "zakupy",
            "zdrowie",
            "sprawy urzędowe",
            "inne",
        ]
    } else {
        vec![
            "work",
            "family and friends",
            "shopping",
            "health",
            "official matters",
            "other",
        ]
    }
    .into_iter()
    .map(str::to_owned)
    .collect();
    let topic_question = Question {
        id: "topic".into(),
        instructions: if pl {
            "Czego dotyczy ta wiadomość?"
        } else {
            "What is this message about?"
        }
        .into(),
        kind: decisions::QuestionKind::Choice { options: topics },
    };
    vec![
        Pipeline {
            id: None,
            name: if pl {
                "Nagranie → napisy → streszczenie"
            } else {
                "Recording → subtitles → summary"
            }
            .into(),
            template: Some("summary".into()),
            steps: vec![
                Step {
                    language: Some("auto".into()),
                    ..Step::new(StepMode::Transcribe)
                },
                chat(summary),
            ],
        },
        Pipeline {
            id: None,
            name: if pl {
                "Nagranie → tłumaczenie → lektor"
            } else {
                "Recording → translation → voice-over"
            }
            .into(),
            template: Some("voiceover".into()),
            steps: vec![
                Step {
                    language: Some("auto".into()),
                    ..Step::new(StepMode::Transcribe)
                },
                chat(translate),
                Step {
                    language: Some(target_lang.into()),
                    ..Step::new(StepMode::Speak)
                },
            ],
        },
        Pipeline {
            id: None,
            name: if pl {
                "Tekst → ilustracja + narracja"
            } else {
                "Text → illustration + narration"
            }
            .into(),
            template: Some("illustration".into()),
            steps: vec![
                chat(image_prompt),
                Step {
                    aspect: Some("16:9".into()),
                    ..Step::new(StepMode::Image)
                },
                Step {
                    from: Some(0),
                    ..chat(narration)
                },
                Step {
                    language: Some(own_lang.into()),
                    ..Step::new(StepMode::Speak)
                },
            ],
        },
        Pipeline {
            id: None,
            name: if pl {
                "Wiadomość głosowa → temat"
            } else {
                "Voice message → topic"
            }
            .into(),
            template: Some("topic".into()),
            steps: vec![
                Step {
                    language: Some("auto".into()),
                    ..Step::new(StepMode::Transcribe)
                },
                Step {
                    question: Some(topic_question),
                    ..Step::new(StepMode::Decide)
                },
            ],
        },
    ]
}

// --- storage ------------------------------------------------------------------------

fn row_to_pipeline(r: &rusqlite::Row<'_>) -> rusqlite::Result<Pipeline> {
    let steps: String = r.get(3)?;
    Ok(Pipeline {
        id: Some(r.get(0)?),
        name: r.get(1)?,
        template: r.get(2)?,
        steps: serde_json::from_str(&steps).unwrap_or_default(),
    })
}

/// Pipelines saved by the user.
pub struct Pipelines<'a> {
    db: &'a Db,
}

impl<'a> Pipelines<'a> {
    pub fn new(db: &'a Db) -> Self {
        Pipelines { db }
    }

    pub fn list(&self) -> Result<Vec<Pipeline>, DbError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, template, steps FROM pipelines ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt
            .query_map([], row_to_pipeline)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get(&self, id: i64) -> Result<Option<Pipeline>, DbError> {
        Ok(self
            .db
            .conn()
            .query_row(
                "SELECT id, name, template, steps FROM pipelines WHERE id = ?1",
                params![id],
                row_to_pipeline,
            )
            .optional()?)
    }

    /// Inserts a new pipeline (`id` = `None`) or updates an existing one.
    pub fn save(&self, p: &Pipeline) -> Result<Pipeline, DbError> {
        let steps = serde_json::to_string(&p.steps)?;
        let now = now_secs();
        let name = p.name.trim();
        let id = {
            let conn = self.db.conn();
            match p.id {
                Some(id) => {
                    let n = conn.execute(
                        "UPDATE pipelines SET name = ?2, template = ?3, steps = ?4, updated_at = ?5 WHERE id = ?1",
                        params![id, name, p.template, steps, now],
                    )?;
                    if n == 0 {
                        return Err(DbError::Sql(rusqlite::Error::QueryReturnedNoRows));
                    }
                    id
                }
                None => {
                    conn.execute(
                        "INSERT INTO pipelines (name, template, steps, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
                        params![name, p.template, steps, now],
                    )?;
                    conn.last_insert_rowid()
                }
            }
        };
        tracing::info!(pipeline_id = id, steps = p.steps.len(), "pipeline saved");
        self.get(id)?
            .ok_or(DbError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    pub fn delete(&self, id: i64) -> Result<bool, DbError> {
        let n = self
            .db
            .conn()
            .execute("DELETE FROM pipelines WHERE id = ?1", params![id])?;
        tracing::info!(pipeline_id = id, deleted = n > 0, "pipeline deleted");
        Ok(n > 0)
    }
}

/// What a run starts with: text or a file (recording).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunInput {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub file: Option<PathBuf>,
}

/// Result of one step, passed on to the steps that read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepOutput {
    pub port: Port,
    /// Text, transcript or the prompt the image was made from.
    #[serde(default)]
    pub text: String,
    /// Audio or image files.
    #[serde(default)]
    pub files: Vec<PathBuf>,
    /// Decide steps with a gate: whether the run went on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_passed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRun {
    pub status: RunStatus,
    /// The job (Gallery entry) a media step created.
    #[serde(default)]
    pub job_id: Option<i64>,
    #[serde(default)]
    pub output: Option<StepOutput>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Not run because a decision gate stopped the run (status `cancelled`).
    #[serde(default)]
    pub skipped: bool,
}

impl StepRun {
    pub fn queued() -> Self {
        StepRun {
            status: RunStatus::Queued,
            job_id: None,
            output: None,
            error: None,
            duration_ms: None,
            skipped: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: i64,
    pub pipeline_id: Option<i64>,
    pub name: String,
    pub status: RunStatus,
    /// The definition as it was when the run started.
    pub definition: Pipeline,
    pub input: RunInput,
    pub steps: Vec<StepRun>,
    pub error: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

const RUN_COLUMNS: &str =
    "id, pipeline_id, name, status, definition, input, steps, error, created_at, finished_at";

fn row_to_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<PipelineRun> {
    let definition: String = r.get(4)?;
    let input: String = r.get(5)?;
    let steps: String = r.get(6)?;
    let name: String = r.get(2)?;
    Ok(PipelineRun {
        id: r.get(0)?,
        pipeline_id: r.get(1)?,
        status: RunStatus::parse(&r.get::<_, String>(3)?),
        definition: serde_json::from_str(&definition).unwrap_or(Pipeline {
            id: None,
            name: name.clone(),
            template: None,
            steps: Vec::new(),
        }),
        name,
        input: serde_json::from_str(&input).unwrap_or_default(),
        steps: serde_json::from_str(&steps).unwrap_or_default(),
        error: r.get(7)?,
        created_at: r.get(8)?,
        finished_at: r.get(9)?,
    })
}

/// Pipeline runs ("Uruchom testowo" and runs from the Workshop).
pub struct Runs<'a> {
    db: &'a Db,
}

impl<'a> Runs<'a> {
    pub fn new(db: &'a Db) -> Self {
        Runs { db }
    }

    pub fn create(&self, pipeline: &Pipeline, input: &RunInput) -> Result<PipelineRun, DbError> {
        let steps: Vec<StepRun> = pipeline.steps.iter().map(|_| StepRun::queued()).collect();
        let id = {
            let conn = self.db.conn();
            conn.execute(
                "INSERT INTO pipeline_runs (pipeline_id, name, status, definition, input, steps, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    pipeline.id,
                    pipeline.name.trim(),
                    RunStatus::Queued.as_str(),
                    serde_json::to_string(pipeline)?,
                    serde_json::to_string(input)?,
                    serde_json::to_string(&steps)?,
                    now_secs()
                ],
            )?;
            conn.last_insert_rowid()
        };
        self.get(id)?
            .ok_or(DbError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    pub fn get(&self, id: i64) -> Result<Option<PipelineRun>, DbError> {
        Ok(self
            .db
            .conn()
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM pipeline_runs WHERE id = ?1"),
                params![id],
                row_to_run,
            )
            .optional()?)
    }

    pub fn list(&self, limit: u32) -> Result<Vec<PipelineRun>, DbError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM pipeline_runs ORDER BY id DESC LIMIT ?1"
        ))?;
        let rows = stmt
            .query_map(params![limit.min(500)], row_to_run)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn save_steps(&self, id: i64, steps: &[StepRun]) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE pipeline_runs SET steps = ?2 WHERE id = ?1",
            params![id, serde_json::to_string(steps)?],
        )?;
        Ok(())
    }

    pub fn set_status(
        &self,
        id: i64,
        status: RunStatus,
        error: Option<&str>,
    ) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE pipeline_runs SET status = ?2, error = ?3,
                finished_at = CASE WHEN ?4 THEN ?5 ELSE finished_at END
             WHERE id = ?1",
            params![id, status.as_str(), error, status.is_finished(), now_secs()],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<bool, DbError> {
        Ok(self
            .db
            .conn()
            .execute("DELETE FROM pipeline_runs WHERE id = ?1", params![id])?
            > 0)
    }

    /// Marks runs left unfinished by a previous app run as failed.
    pub fn fail_unfinished(&self, code: &str) -> Result<usize, DbError> {
        Ok(self.db.conn().execute(
            "UPDATE pipeline_runs SET status = 'failed', error = ?1, finished_at = ?2
             WHERE status NOT IN ('done', 'failed', 'cancelled')",
            params![code, now_secs()],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(p: &Pipeline) -> Vec<(Option<usize>, String)> {
        validate(p).into_iter().map(|i| (i.step, i.code)).collect()
    }

    #[test]
    fn templates_are_valid_in_both_languages() {
        for lang in ["pl", "en"] {
            let all = templates(lang);
            assert_eq!(all.len(), 4);
            for p in &all {
                assert!(
                    validate(p).is_empty(),
                    "{lang} {}: {:?}",
                    p.name,
                    validate(p)
                );
            }
            assert_eq!(input_port(&all[0]), Some(Port::Audio));
            assert_eq!(input_port(&all[1]), Some(Port::Audio));
            assert_eq!(input_port(&all[2]), Some(Port::Text));
            assert_eq!(input_port(&all[3]), Some(Port::Audio));
            assert_eq!(all[3].steps[1].mode.modality(), Modality::Decision);
        }
    }

    #[test]
    fn decide_steps_need_a_valid_question_and_gate() {
        let question = Question {
            id: "spam".into(),
            instructions: "Czy to spam?".into(),
            kind: decisions::QuestionKind::YesNo,
        };
        let mut p = Pipeline {
            id: None,
            name: "x".into(),
            template: None,
            steps: vec![Step::new(StepMode::Decide), Step::new(StepMode::Speak)],
        };
        assert_eq!(codes(&p), vec![(Some(0), "no_question".into())]);
        p.steps[0].question = Some(Question {
            kind: decisions::QuestionKind::Choice {
                options: vec!["a".into()],
            },
            ..question.clone()
        });
        assert_eq!(codes(&p), vec![(Some(0), "bad_question".into())]);
        p.steps[0].question = Some(question);
        p.steps[0].gate = Some(Gate {
            answer: "maybe".into(),
            min_percent: None,
        });
        assert_eq!(codes(&p), vec![(Some(0), "bad_gate".into())]);
        p.steps[0].gate = Some(Gate {
            answer: decisions::NO.into(),
            min_percent: Some(70),
        });
        // decide → speak: the answer is text
        assert!(validate(&p).is_empty());
        let json = serde_json::to_string(&p.steps).unwrap();
        let back: Vec<Step> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p.steps);
    }

    #[test]
    fn type_checks_between_steps() {
        let mut p = Pipeline {
            id: None,
            name: "x".into(),
            template: None,
            steps: vec![Step::new(StepMode::Speak), Step::new(StepMode::Chat)],
        };
        // audio → chat (text) is wrong
        assert_eq!(codes(&p), vec![(Some(1), "type_mismatch".into())]);
        // subtitles feed text
        p.steps = vec![Step::new(StepMode::Transcribe), Step::new(StepMode::Speak)];
        assert!(validate(&p).is_empty());
        // image output feeds nothing
        p.steps = vec![Step::new(StepMode::Image), Step::new(StepMode::Speak)];
        assert_eq!(codes(&p), vec![(Some(1), "type_mismatch".into())]);
        // two steps read the pipeline input with different types
        p.steps = vec![
            Step::new(StepMode::Transcribe),
            Step {
                from: Some(0),
                ..Step::new(StepMode::Chat)
            },
        ];
        assert_eq!(codes(&p), vec![(Some(1), "input_mismatch".into())]);
        // a step cannot read itself or a later step
        p.steps = vec![Step {
            from: Some(1),
            ..Step::new(StepMode::Chat)
        }];
        assert_eq!(codes(&p), vec![(Some(0), "bad_source".into())]);
        p.steps.clear();
        p.name = " ".into();
        assert_eq!(
            codes(&p),
            vec![(None, "no_name".into()), (None, "no_steps".into())]
        );
    }

    #[test]
    fn prompt_rendering() {
        assert_eq!(
            render_prompt(Some("Streść: {{input}}"), "abc"),
            "Streść: abc"
        );
        assert_eq!(render_prompt(Some("Streść"), "abc"), "Streść\n\nabc");
        assert_eq!(render_prompt(None, "abc"), "abc");
        assert_eq!(render_prompt(Some("  "), "abc"), "abc");
    }

    #[test]
    fn storage_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let store = Pipelines::new(&db);
        let mut p = templates("pl").remove(1);
        let saved = store.save(&p).unwrap();
        assert!(saved.id.is_some());
        assert_eq!(saved.steps, p.steps);
        p = saved.clone();
        p.name = "Mój łańcuch".into();
        let updated = store.save(&p).unwrap();
        assert_eq!(updated.id, saved.id);
        assert_eq!(store.list().unwrap().len(), 1);

        let runs = Runs::new(&db);
        let run = runs
            .create(
                &updated,
                &RunInput {
                    file: Some("a.wav".into()),
                    text: None,
                },
            )
            .unwrap();
        assert_eq!(run.steps.len(), 3);
        assert_eq!(run.status, RunStatus::Queued);
        let mut steps = run.steps.clone();
        steps[0] = StepRun {
            status: RunStatus::Done,
            job_id: Some(7),
            output: Some(StepOutput {
                port: Port::Subtitles,
                text: "dzień dobry".into(),
                files: vec![],
                gate_passed: None,
            }),
            error: None,
            duration_ms: Some(10),
            skipped: false,
        };
        runs.save_steps(run.id, &steps).unwrap();
        runs.set_status(run.id, RunStatus::Running, None).unwrap();
        assert_eq!(runs.fail_unfinished("interrupted").unwrap(), 1);
        let back = runs.get(run.id).unwrap().unwrap();
        assert_eq!(back.status, RunStatus::Failed);
        assert!(back.finished_at.is_some());
        assert_eq!(back.steps[0].job_id, Some(7));
        assert_eq!(back.definition.name, "Mój łańcuch");
        assert!(store.delete(updated.id.unwrap()).unwrap());
        assert!(store.list().unwrap().is_empty());
    }
}

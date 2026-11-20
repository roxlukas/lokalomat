//! Typed decisions (PRD §15 point 10, ADR 0024): a decision model (Basal)
//! answers questions about a text with a choice, yes/no or a score and the
//! probability of every answer. This module holds the questions and answers,
//! their validation and the mapping to llama.cpp's `/v1/systemone` (the
//! TypeSafe System One API); the HTTP call lives in `lokalsi_engines::llama`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// llama.cpp reads the options of the Basal files as the codes `A`–`Z`.
pub const MAX_OPTIONS: usize = 26;
/// Most score levels the System One API accepts.
pub const MAX_LEVELS: usize = 10;
/// Most questions in one request (keeps the Workshop form and the prompt small).
pub const MAX_QUESTIONS: usize = 12;
/// Longest text to judge (Basal reads 32k tokens; the server runs with 16k).
pub const MAX_STATE_CHARS: usize = 20_000;
/// Longest question or option.
pub const MAX_FIELD_CHARS: usize = 500;

/// Labels of a yes/no answer in [`Answer::label`] and in gates.
pub const YES: &str = "yes";
pub const NO: &str = "no";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuestionKind {
    /// Pick one of the options.
    Choice { options: Vec<String> },
    /// Yes or no (System One `noul`).
    YesNo,
    /// A level on a scale, lowest first.
    Score { levels: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    /// Key of the answer; unique within a request.
    pub id: String,
    /// The question, e.g. "Który dział powinien się tym zająć?".
    pub instructions: String,
    #[serde(flatten)]
    pub kind: QuestionKind,
}

impl Question {
    /// Answer labels in order: the options, `yes`/`no` or the levels.
    pub fn labels(&self) -> Vec<String> {
        match &self.kind {
            QuestionKind::Choice { options } => options.clone(),
            QuestionKind::YesNo => vec![YES.to_owned(), NO.to_owned()],
            QuestionKind::Score { levels } => levels.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DecisionError {
    #[error("the text to judge is empty")]
    EmptyState,
    #[error("the text to judge is too long ({0} characters)")]
    StateTooLong(usize),
    #[error("ask between 1 and {MAX_QUESTIONS} questions")]
    QuestionCount,
    #[error("question {0}: the question is empty")]
    EmptyQuestion(usize),
    #[error("question {0}: duplicate id")]
    DuplicateId(usize),
    #[error("question {0}: give between 2 and {1} distinct, non-empty options")]
    Options(usize, usize),
    #[error("question {0}: a text is longer than {MAX_FIELD_CHARS} characters")]
    TooLong(usize),
}

/// Checks a request before it reaches the engine (1-based question numbers in errors).
pub fn validate(state: &str, questions: &[Question]) -> Result<(), DecisionError> {
    let chars = state.chars().count();
    if state.trim().is_empty() {
        return Err(DecisionError::EmptyState);
    }
    if chars > MAX_STATE_CHARS {
        return Err(DecisionError::StateTooLong(chars));
    }
    if questions.is_empty() || questions.len() > MAX_QUESTIONS {
        return Err(DecisionError::QuestionCount);
    }
    for (i, q) in questions.iter().enumerate() {
        let n = i + 1;
        if q.instructions.trim().is_empty() || q.id.trim().is_empty() {
            return Err(DecisionError::EmptyQuestion(n));
        }
        if questions[..i].iter().any(|p| p.id == q.id) {
            return Err(DecisionError::DuplicateId(n));
        }
        if q.instructions.chars().count() > MAX_FIELD_CHARS {
            return Err(DecisionError::TooLong(n));
        }
        let (items, max) = match &q.kind {
            QuestionKind::Choice { options } => (options, MAX_OPTIONS),
            QuestionKind::Score { levels } => (levels, MAX_LEVELS),
            QuestionKind::YesNo => continue,
        };
        let distinct = items
            .iter()
            .enumerate()
            .all(|(j, o)| !o.trim().is_empty() && !items[..j].contains(o));
        if items.len() < 2 || items.len() > max || !distinct {
            return Err(DecisionError::Options(n, max));
        }
        if items.iter().any(|o| o.chars().count() > MAX_FIELD_CHARS) {
            return Err(DecisionError::TooLong(n));
        }
    }
    Ok(())
}

/// Body of `POST /v1/systemone`.
pub fn systemone_request(state: &str, questions: &[Question]) -> Value {
    let mut qs = Map::new();
    for q in questions {
        let mut item = json!({ "instructions": q.instructions });
        match &q.kind {
            QuestionKind::Choice { options } => {
                item["type"] = json!("choice");
                item["criteria"] =
                    Value::Object(options.iter().map(|o| (o.clone(), Value::Null)).collect());
            }
            QuestionKind::YesNo => item["type"] = json!("noul"),
            QuestionKind::Score { levels } => {
                item["type"] = json!("score");
                item["criteria"] = json!(levels);
            }
        }
        qs.insert(q.id.clone(), item);
    }
    json!({ "state": state, "questions": qs })
}

/// Probability of one answer label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probability {
    pub label: String,
    pub p: f64,
}

/// One answered question, the same shape for every kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub id: String,
    /// The most likely answer: an option, `yes`/`no` or a level.
    pub label: String,
    /// Every label in question order; they sum to 1.
    pub probabilities: Vec<Probability>,
    /// 0 = all answers equally likely, 1 = certain (System One `confidence`;
    /// for yes/no `|2p − 1|`).
    pub confidence: f64,
    /// Score questions: expected level index (may lie between two levels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

impl Answer {
    pub fn probability(&self, label: &str) -> Option<f64> {
        self.probabilities
            .iter()
            .find(|p| p.label == label)
            .map(|p| p.p)
    }
}

/// Answers of a request plus what the engine reported.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub answers: Vec<Answer>,
    pub input_tokens: Option<u64>,
}

/// Reads a `/v1/systemone` response for `questions` (answers in question order).
pub fn parse_systemone(questions: &[Question], body: &Value) -> Result<Decision, String> {
    let answers = body
        .get("answers")
        .and_then(Value::as_object)
        .ok_or("response has no answers")?;
    let mut out = Vec::with_capacity(questions.len());
    for q in questions {
        let a = answers
            .get(&q.id)
            .ok_or_else(|| format!("no answer for {}", q.id))?;
        let num = |v: Option<&Value>| v.and_then(Value::as_f64);
        let answer = match &q.kind {
            QuestionKind::YesNo => {
                let p = num(a.get("noul"))
                    .ok_or_else(|| format!("{}: no yes/no probability", q.id))?
                    .clamp(0.0, 1.0);
                Answer {
                    id: q.id.clone(),
                    label: if p >= 0.5 { YES } else { NO }.to_owned(),
                    probabilities: vec![
                        Probability {
                            label: YES.to_owned(),
                            p,
                        },
                        Probability {
                            label: NO.to_owned(),
                            p: 1.0 - p,
                        },
                    ],
                    confidence: (2.0 * p - 1.0).abs(),
                    score: None,
                }
            }
            QuestionKind::Choice { options } => {
                let probs = a.get("probabilities").and_then(Value::as_object);
                let probabilities: Vec<Probability> = options
                    .iter()
                    .map(|o| Probability {
                        label: o.clone(),
                        p: probs.and_then(|m| num(m.get(o))).unwrap_or(0.0),
                    })
                    .collect();
                let label = a
                    .get("choice")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| top(&probabilities));
                Answer {
                    id: q.id.clone(),
                    label,
                    confidence: num(a.get("confidence")).unwrap_or(0.0),
                    probabilities,
                    score: None,
                }
            }
            QuestionKind::Score { levels } => {
                let probs = a.get("probabilities").and_then(Value::as_object);
                let probabilities: Vec<Probability> = levels
                    .iter()
                    .enumerate()
                    .map(|(i, l)| Probability {
                        label: l.clone(),
                        p: probs
                            .and_then(|m| num(m.get(&i.to_string())))
                            .unwrap_or(0.0),
                    })
                    .collect();
                Answer {
                    id: q.id.clone(),
                    label: top(&probabilities),
                    confidence: num(a.get("confidence")).unwrap_or(0.0),
                    probabilities,
                    score: num(a.get("score")),
                }
            }
        };
        out.push(answer);
    }
    Ok(Decision {
        answers: out,
        input_tokens: body
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(Value::as_u64),
    })
}

fn top(probabilities: &[Probability]) -> String {
    probabilities
        .iter()
        .max_by(|a, b| a.p.total_cmp(&b.p))
        .map(|p| p.label.clone())
        .unwrap_or_default()
}

/// Pipeline gate: the run goes on only when the decision says so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    /// The answer label to continue on (an option, `yes`/`no` or a level).
    pub answer: String,
    /// `None`: the label must be the most likely answer; `Some(t)`: its
    /// probability must be at least `t` percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_percent: Option<u8>,
}

impl Gate {
    pub fn passes(&self, answer: &Answer) -> bool {
        match self.min_percent {
            Some(t) => answer.probability(&self.answer).unwrap_or(0.0) * 100.0 >= f64::from(t),
            None => answer.label == self.answer,
        }
    }

    /// True when the gate names a label the question can give.
    pub fn fits(&self, question: &Question) -> bool {
        question.labels().contains(&self.answer) && self.min_percent.is_none_or(|t| t <= 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn questions() -> Vec<Question> {
        vec![
            Question {
                id: "route".into(),
                instructions: "Which team should handle this?".into(),
                kind: QuestionKind::Choice {
                    options: vec!["billing".into(), "shipping".into(), "technical".into()],
                },
            },
            Question {
                id: "angry".into(),
                instructions: "Is the customer angry?".into(),
                kind: QuestionKind::YesNo,
            },
            Question {
                id: "urgency".into(),
                instructions: "How urgent is this?".into(),
                kind: QuestionKind::Score {
                    levels: vec![
                        "can wait".into(),
                        "this week".into(),
                        "today".into(),
                        "right now".into(),
                    ],
                },
            },
        ]
    }

    #[test]
    fn request_matches_the_system_one_shape() {
        let body = systemone_request("Customer message: charged twice.", &questions());
        assert_eq!(body["state"], "Customer message: charged twice.");
        let q = &body["questions"];
        assert_eq!(q["route"]["type"], "choice");
        assert_eq!(q["route"]["criteria"]["billing"], Value::Null);
        assert_eq!(q["angry"]["type"], "noul");
        assert!(q["angry"].get("criteria").is_none());
        assert_eq!(q["urgency"]["type"], "score");
        assert_eq!(q["urgency"]["criteria"][3], "right now");
    }

    /// The example response from llama.cpp's server README.
    #[test]
    fn response_is_read_in_question_order() {
        let body = json!({
          "model": "openjev",
          "answers": {
            "route": {"type": "choice", "choice": "billing",
              "probabilities": {"billing": 0.9998, "shipping": 0.0001, "technical": 0.0001},
              "confidence": 0.9997},
            "angry": {"type": "noul", "noul": 0.6328},
            "urgency": {"type": "score", "score": 2.0858,
              "legend": {"0": "can wait", "1": "this week", "2": "today", "3": "right now"},
              "probabilities": {"0": 0.0023, "1": 0.116, "2": 0.6753, "3": 0.2064},
              "confidence": 0.673}
          },
          "usage": {"input_tokens": 239, "output_tokens": 0}
        });
        let d = parse_systemone(&questions(), &body).unwrap();
        assert_eq!(d.input_tokens, Some(239));
        assert_eq!(d.answers[0].label, "billing");
        assert_eq!(d.answers[1].label, YES);
        assert!((d.answers[1].probability(NO).unwrap() - 0.3672).abs() < 1e-9);
        assert!((d.answers[1].confidence - 0.2656).abs() < 1e-9);
        assert_eq!(d.answers[2].label, "today");
        assert_eq!(d.answers[2].score, Some(2.0858));
        for a in &d.answers {
            let sum: f64 = a.probabilities.iter().map(|p| p.p).sum();
            assert!((sum - 1.0).abs() < 1e-3, "{a:?}");
        }
        assert!(parse_systemone(&questions(), &json!({"answers": {}})).is_err());
    }

    #[test]
    fn validation_rejects_bad_requests() {
        let qs = questions();
        assert!(validate("text", &qs).is_ok());
        assert_eq!(validate("  ", &qs), Err(DecisionError::EmptyState));
        assert_eq!(validate("text", &[]), Err(DecisionError::QuestionCount));
        let mut dup = qs.clone();
        dup[1].id = "route".into();
        assert_eq!(validate("text", &dup), Err(DecisionError::DuplicateId(2)));
        let mut one = qs.clone();
        one[0].kind = QuestionKind::Choice {
            options: vec!["only".into()],
        };
        assert_eq!(validate("text", &one), Err(DecisionError::Options(1, 26)));
        let mut same = qs.clone();
        same[2].kind = QuestionKind::Score {
            levels: vec!["a".into(), "a".into()],
        };
        assert_eq!(validate("text", &same), Err(DecisionError::Options(3, 10)));
        let long = "x".repeat(MAX_STATE_CHARS + 1);
        assert!(matches!(
            validate(&long, &qs),
            Err(DecisionError::StateTooLong(_))
        ));
    }

    #[test]
    fn gates_follow_the_top_answer_or_a_threshold() {
        let a = Answer {
            id: "angry".into(),
            label: YES.into(),
            probabilities: vec![
                Probability {
                    label: YES.into(),
                    p: 0.6,
                },
                Probability {
                    label: NO.into(),
                    p: 0.4,
                },
            ],
            confidence: 0.2,
            score: None,
        };
        let top = Gate {
            answer: YES.into(),
            min_percent: None,
        };
        assert!(top.passes(&a));
        let strict = Gate {
            answer: YES.into(),
            min_percent: Some(80),
        };
        assert!(!strict.passes(&a));
        let no = Gate {
            answer: NO.into(),
            min_percent: Some(30),
        };
        assert!(no.passes(&a));
        let q = &questions()[1];
        assert!(top.fits(q));
        assert!(
            !Gate {
                answer: "maybe".into(),
                min_percent: None
            }
            .fits(q)
        );
    }
}

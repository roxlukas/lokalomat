//! Transcript segments and their export formats (PRD V2: TXT/SRT/VTT/JSON).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Txt,
    Srt,
    Vtt,
    Json,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "txt" => Some(Format::Txt),
            "srt" => Some(Format::Srt),
            "vtt" => Some(Format::Vtt),
            "json" => Some(Format::Json),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Txt => "txt",
            Format::Srt => "srt",
            Format::Vtt => "vtt",
            Format::Json => "json",
        }
    }
}

/// `HH:MM:SS<sep>mmm`.
fn timestamp(ms: u64, sep: char) -> String {
    let h = ms / 3_600_000;
    let m = (ms / 60_000) % 60;
    let s = (ms / 1000) % 60;
    format!("{h:02}:{m:02}:{s:02}{sep}{:03}", ms % 1000)
}

/// Segments with text (empty ones are dropped), trimmed.
fn cues(segments: &[Segment]) -> impl Iterator<Item = (&Segment, &str)> {
    segments
        .iter()
        .map(|s| (s, s.text.trim()))
        .filter(|(_, t)| !t.is_empty())
}

/// Plain text: one segment per line.
pub fn to_txt(segments: &[Segment]) -> String {
    let mut out = String::new();
    for (_, text) in cues(segments) {
        out.push_str(text);
        out.push('\n');
    }
    out
}

pub fn to_srt(segments: &[Segment]) -> String {
    let mut out = String::new();
    for (i, (s, text)) in cues(segments).enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            timestamp(s.start_ms, ','),
            timestamp(s.end_ms.max(s.start_ms), ','),
            text
        ));
    }
    out
}

pub fn to_vtt(segments: &[Segment]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for (s, text) in cues(segments) {
        out.push_str(&format!(
            "{} --> {}\n{}\n\n",
            timestamp(s.start_ms, '.'),
            timestamp(s.end_ms.max(s.start_ms), '.'),
            text
        ));
    }
    out
}

pub fn to_json(segments: &[Segment], language: Option<&str>) -> String {
    let segs: Vec<_> = cues(segments)
        .map(|(s, text)| serde_json::json!({ "start_ms": s.start_ms, "end_ms": s.end_ms, "text": text }))
        .collect();
    let doc = serde_json::json!({ "language": language, "segments": segs });
    serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n"
}

pub fn render(format: Format, segments: &[Segment], language: Option<&str>) -> String {
    match format {
        Format::Txt => to_txt(segments),
        Format::Srt => to_srt(segments),
        Format::Vtt => to_vtt(segments),
        Format::Json => to_json(segments, language),
    }
}

/// Text of all segments joined with spaces (search, preview).
pub fn plain_text(segments: &[Segment]) -> String {
    cues(segments).map(|(_, t)| t).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs() -> Vec<Segment> {
        vec![
            Segment {
                start_ms: 0,
                end_ms: 2_500,
                text: " Dzień dobry.".into(),
            },
            Segment {
                start_ms: 2_500,
                end_ms: 3_725_042,
                text: "Witamy w studiu.".into(),
            },
            Segment {
                start_ms: 3_725_042,
                end_ms: 3_725_042,
                text: "  ".into(),
            },
        ]
    }

    #[test]
    fn srt_numbering_and_times() {
        assert_eq!(
            to_srt(&segs()),
            "1\n00:00:00,000 --> 00:00:02,500\nDzień dobry.\n\n2\n00:00:02,500 --> 01:02:05,042\nWitamy w studiu.\n\n"
        );
    }

    #[test]
    fn vtt_txt_and_json() {
        let vtt = to_vtt(&segs());
        assert!(vtt.starts_with("WEBVTT\n\n00:00:00.000 --> 00:00:02.500\nDzień dobry.\n"));
        assert_eq!(to_txt(&segs()), "Dzień dobry.\nWitamy w studiu.\n");
        let json: serde_json::Value = serde_json::from_str(&to_json(&segs(), Some("pl"))).unwrap();
        assert_eq!(json["language"], "pl");
        assert_eq!(json["segments"].as_array().unwrap().len(), 2);
        assert_eq!(plain_text(&segs()), "Dzień dobry. Witamy w studiu.");
        assert_eq!(Format::parse("SRT"), Some(Format::Srt));
    }
}

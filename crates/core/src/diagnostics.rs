//! Diagnostics bundle: a ZIP the user can attach to a bug report.
//! Contains redacted logs plus caller-provided files (system info, settings).

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::logging::log_files;
use crate::redact::redact;

/// Per-file cap; for bigger logs only the newest part is included.
const MAX_LOG_BYTES: u64 = 20 * 1024 * 1024;

const README: &str = "Lokalomat AI Studio – paczka diagnostyczna / diagnostics bundle\n\
\n\
PL: Paczka zawiera logi aplikacji, opis sprzętu i ustawienia. Tokeny, hasła i klucze\n\
API zostały zamaskowane (***). Treść poleceń trafia do logów tylko, jeśli włączono to\n\
ręcznie w trybie szczegółowym. Przejrzyj pliki przed wysłaniem.\n\
\n\
EN: Contains application logs, hardware description and settings. Tokens, passwords\n\
and API keys are masked (***). Prompt contents are only logged when explicitly enabled\n\
in verbose mode. Review the files before sending.\n";

#[derive(Debug, Clone, Serialize)]
pub struct ExportSummary {
    pub path: PathBuf,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticsError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Writes the bundle to `out`. `extra` entries are `(file name, content)`
/// and are redacted like the logs.
pub fn export_bundle(
    log_dir: &Path,
    extra: &[(&str, String)],
    out: &Path,
) -> Result<ExportSummary, DiagnosticsError> {
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut zip = zip::ZipWriter::new(File::create(out)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut count = 0usize;

    zip.start_file("README.txt", options)?;
    zip.write_all(README.as_bytes())?;
    count += 1;

    for (name, content) in extra {
        zip.start_file(*name, options)?;
        zip.write_all(redact(content).as_bytes())?;
        count += 1;
    }

    let logs = if log_dir.exists() {
        log_files(log_dir)?
    } else {
        Vec::new()
    };
    for path in logs {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let text = read_capped(&path)?;
        zip.start_file(format!("logs/{name}"), options)?;
        zip.write_all(redact(&text).as_bytes())?;
        count += 1;
    }

    zip.finish()?;
    let bytes = fs::metadata(out)?.len();
    Ok(ExportSummary {
        path: out.to_path_buf(),
        files: count,
        bytes,
    })
}

fn read_capped(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(MAX_LOG_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        text.insert_str(
            0,
            &format!("[… truncated, first {start} bytes omitted …]\n"),
        );
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_entry(zip_path: &Path, name: &str) -> String {
        let mut archive = zip::ZipArchive::new(File::open(zip_path).unwrap()).unwrap();
        let mut entry = archive.by_name(name).unwrap();
        let mut s = String::new();
        entry.read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn bundle_contains_redacted_logs_and_extras() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("app.2026-11-11.log"),
            "INFO start\nDEBUG token=hf_SECRETSECRET123 used\n",
        )
        .unwrap();
        let out = dir.path().join("out").join("diag.zip");

        let summary = export_bundle(
            &logs,
            &[(
                "settings.json",
                r#"{"hf_token": "abc-def", "theme": "dark"}"#.into(),
            )],
            &out,
        )
        .unwrap();

        assert_eq!(summary.files, 3);
        assert!(summary.bytes > 0);
        let log = read_entry(&out, "logs/app.2026-11-11.log");
        assert!(log.contains("INFO start"));
        assert!(!log.contains("SECRETSECRET"), "{log}");
        let settings = read_entry(&out, "settings.json");
        assert!(!settings.contains("abc-def"), "{settings}");
        assert!(settings.contains("dark"));
        assert!(read_entry(&out, "README.txt").contains("diagnostics bundle"));
    }

    #[test]
    fn bundle_works_without_logs() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("diag.zip");
        let summary = export_bundle(&dir.path().join("missing"), &[], &out).unwrap();
        assert_eq!(summary.files, 1);
    }
}

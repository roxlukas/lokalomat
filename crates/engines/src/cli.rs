//! One engine process per job (`whisper-cli`, `sd-cli`, ADR 0015): start it,
//! feed its output lines to a progress parser, kill it on cancel.
//!
//! Output goes to the engine log, except lines that would reveal content
//! (prompts, transcripts): those are replaced by a marker (PRD §10).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("cannot start the engine: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("the engine stopped with exit code {code:?}: {detail}")]
    Exited { code: Option<i32>, detail: String },
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    MissingLibrary(crate::syslibs::MissingLibrary),
}

pub struct CliJob<'a> {
    pub binary: &'a Path,
    pub args: Vec<String>,
    /// Working directory (relative paths in `args` resolve here).
    pub cwd: &'a Path,
    pub log_file: &'a Path,
    /// Lines containing any of these are not written to the log.
    pub hide: Vec<String>,
    /// Lines matching this are not written to the log either.
    pub hide_line: fn(&str) -> bool,
    /// Extra environment variables for the process.
    pub env: Vec<(String, String)>,
}

pub const HIDDEN: &str = "[content hidden]";

fn no_filter(_: &str) -> bool {
    false
}

impl<'a> CliJob<'a> {
    pub fn new(binary: &'a Path, args: Vec<String>, cwd: &'a Path, log_file: &'a Path) -> Self {
        CliJob {
            binary,
            args,
            cwd,
            log_file,
            hide: Vec::new(),
            hide_line: no_filter,
            env: Vec::new(),
        }
    }

    fn loggable(&self, line: &str) -> bool {
        !(self.hide_line)(line)
            && !self
                .hide
                .iter()
                .any(|h| !h.is_empty() && line.contains(h.as_str()))
    }
}

/// Splits `buf` on `\r` / `\n`, returning complete lines and keeping the rest.
fn take_lines(buf: &mut Vec<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, b) in buf.iter().enumerate() {
        if *b == b'\n' || *b == b'\r' {
            if i > start {
                lines.push(String::from_utf8_lossy(&buf[start..i]).into_owned());
            }
            start = i + 1;
        }
    }
    buf.drain(..start);
    lines
}

/// Runs the job to completion. `on_line` sees every output line (stdout and
/// stderr, progress bars split on `\r`). Cancelling kills the process.
pub async fn run(
    job: &CliJob<'_>,
    cancel: &CancellationToken,
    on_line: &mut (dyn FnMut(&str) + Send),
) -> Result<(), CliError> {
    if let Some(parent) = job.log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(job.log_file)?;
    let _ = writeln!(
        log,
        "--- {} {}",
        lokalsi_core::db::now_secs(),
        job.binary.display()
    );

    let mut cmd = tokio::process::Command::new(job.binary);
    cmd.args(&job.args)
        .envs(job.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(job.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    tracing::info!(binary = %job.binary.display(), "starting engine job");
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");

    let (mut out_buf, mut err_buf) = (Vec::new(), Vec::new());
    let (mut out_open, mut err_open) = (true, true);
    let mut tail: Vec<String> = Vec::new();
    let mut chunk_out = [0u8; 4096];
    let mut chunk_err = [0u8; 4096];
    let mut handle = |lines: Vec<String>, log: &mut std::fs::File| {
        for line in lines {
            let shown = if job.loggable(&line) {
                line.as_str()
            } else {
                HIDDEN
            };
            let _ = writeln!(log, "{shown}");
            if tail.len() >= 12 {
                tail.remove(0);
            }
            tail.push(shown.to_owned());
            on_line(&line);
        }
    };

    while out_open || err_open {
        tokio::select! {
            n = stdout.read(&mut chunk_out), if out_open => match n {
                Ok(0) | Err(_) => out_open = false,
                Ok(n) => { out_buf.extend_from_slice(&chunk_out[..n]); handle(take_lines(&mut out_buf), &mut log); }
            },
            n = stderr.read(&mut chunk_err), if err_open => match n {
                Ok(0) | Err(_) => err_open = false,
                Ok(n) => { err_buf.extend_from_slice(&chunk_err[..n]); handle(take_lines(&mut err_buf), &mut log); }
            },
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                let _ = writeln!(log, "--- cancelled");
                tracing::info!("engine job cancelled, process killed");
                return Err(CliError::Cancelled);
            }
        }
    }
    for rest in [out_buf, err_buf] {
        if !rest.is_empty() {
            let mut rest = rest;
            rest.push(b'\n');
            handle(take_lines(&mut rest), &mut log);
        }
    }
    let status = tokio::select! {
        s = child.wait() => s?,
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            return Err(CliError::Cancelled);
        }
    };
    let _ = writeln!(log, "--- exit {:?}", status.code());
    if status.success() {
        Ok(())
    } else {
        let detail = tail
            .iter()
            .rev()
            .find(|l| {
                let l = l.to_lowercase();
                l.contains("error") || l.contains("failed") || l.contains("out of memory")
            })
            .cloned()
            .unwrap_or_else(|| tail.last().cloned().unwrap_or_default());
        tracing::error!(code = ?status.code(), log = %tail.join("\n"), "engine job failed");
        if let Some(missing) = crate::syslibs::detect(&tail.join("\n")) {
            return Err(CliError::MissingLibrary(missing));
        }
        Err(CliError::Exited {
            code: status.code(),
            detail: detail.chars().take(300).collect(),
        })
    }
}

/// Per-job scratch folder `<engines_dir>/../tmp/job-<id>`; removed on drop.
pub struct JobDir(pub PathBuf);

impl JobDir {
    pub fn create(root: &Path, job_id: i64) -> std::io::Result<Self> {
        let dir = root.join(format!("job-{job_id}"));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(JobDir(dir))
    }
}

impl Drop for JobDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_progress_bars_and_lines() {
        let mut buf = b"|==>  | 1/8 - 2.1s/it\r|===> | 2/8\nnext line\npart".to_vec();
        let lines = take_lines(&mut buf);
        assert_eq!(lines, ["|==>  | 1/8 - 2.1s/it", "|===> | 2/8", "next line"]);
        assert_eq!(buf, b"part");
    }

    #[test]
    fn hides_content_lines() {
        let p = PathBuf::new();
        let mut job = CliJob::new(&p, vec![], &p, &p);
        job.hide = vec!["a red fox".into()];
        job.hide_line = |l| l.contains("-->");
        assert!(!job.loggable("prompt: a red fox in snow"));
        assert!(!job.loggable("[00:00:00.000 --> 00:00:02.000] tekst"));
        assert!(job.loggable("loading model"));
    }

    #[tokio::test]
    async fn recognises_a_missing_system_library() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("e.log");
        let line = "whisper-cli: error while loading shared libraries: libgomp.so.1: cannot open shared object file";
        let (bin, args): (PathBuf, Vec<String>) = if cfg!(windows) {
            (
                PathBuf::from("cmd"),
                vec!["/C".into(), format!("echo {line} 1>&2& exit 127")],
            )
        } else {
            (
                PathBuf::from("sh"),
                vec!["-c".into(), format!("echo '{line}' >&2; exit 127")],
            )
        };
        let job = CliJob::new(&bin, args, dir.path(), &log);
        let err = run(&job, &CancellationToken::new(), &mut |_| {})
            .await
            .unwrap_err();
        match err {
            CliError::MissingLibrary(m) => assert_eq!(m.package, "libgomp1"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn reports_failure_with_the_error_line() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("e.log");
        let (bin, args): (PathBuf, Vec<String>) = if cfg!(windows) {
            (
                PathBuf::from("cmd"),
                vec!["/C".into(), "echo error: no model& exit 3".into()],
            )
        } else {
            (
                PathBuf::from("sh"),
                vec!["-c".into(), "echo 'error: no model'; exit 3".into()],
            )
        };
        let job = CliJob::new(&bin, args, dir.path(), &log);
        let mut seen = Vec::new();
        let err = run(&job, &CancellationToken::new(), &mut |l| {
            seen.push(l.to_owned())
        })
        .await
        .unwrap_err();
        match err {
            CliError::Exited { code, detail } => {
                assert_eq!(code, Some(3));
                assert!(detail.contains("no model"), "{detail}");
            }
            other => panic!("{other:?}"),
        }
        assert!(seen.iter().any(|l| l.contains("no model")));
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .contains("exit Some(3)")
        );
    }
}

//! M1 acceptance: an interrupted download resumes after an application
//! restart (HTTP Range), and a file with a wrong checksum is rejected after
//! one automatic retry. Uses a local HTTP server that can stall mid-file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lokalsi_core::db::{Db, FileSource};
use lokalsi_core::downloads::{
    DownloadConfig, DownloadEvents, DownloadItem, DownloadManager, DownloadStatus, EnqueueOutcome,
    PlannedFile, VariantPlan, part_path,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BODY_LEN: usize = 3 * 1024 * 1024 + 123;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    /// Send this many bytes of the body, then hang without closing.
    StallAfter(usize),
}

struct TestServer {
    addr: String,
    body: Arc<Vec<u8>>,
    mode: Arc<Mutex<Mode>>,
    ranges: Arc<Mutex<Vec<Option<String>>>>,
    requests: Arc<AtomicUsize>,
}

impl TestServer {
    async fn start(mode: Mode) -> Self {
        let body: Vec<u8> = (0..BODY_LEN).map(|i| (i * 31 % 251) as u8).collect();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let server = TestServer {
            addr,
            body: Arc::new(body),
            mode: Arc::new(Mutex::new(mode)),
            ranges: Arc::new(Mutex::new(Vec::new())),
            requests: Arc::new(AtomicUsize::new(0)),
        };
        let (body, mode, ranges, requests) = (
            server.body.clone(),
            server.mode.clone(),
            server.ranges.clone(),
            server.requests.clone(),
        );
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let (body, mode, ranges, requests) =
                    (body.clone(), mode.clone(), ranges.clone(), requests.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = socket.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let head = String::from_utf8_lossy(&buf).to_lowercase();
                    let range = head
                        .lines()
                        .find_map(|l| l.strip_prefix("range:").map(|v| v.trim().to_owned()));
                    requests.fetch_add(1, Ordering::SeqCst);
                    ranges.lock().unwrap().push(range.clone());
                    let start = range
                        .as_deref()
                        .and_then(|r| r.strip_prefix("bytes="))
                        .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok())
                        .unwrap_or(0);
                    let total = body.len();
                    let header = if range.is_some() {
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{total}\r\nConnection: close\r\n\r\n",
                            total - start,
                            total - 1
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
                        )
                    };
                    if socket.write_all(header.as_bytes()).await.is_err() {
                        return;
                    }
                    let current_mode = *mode.lock().unwrap();
                    match current_mode {
                        Mode::Normal => {
                            let _ = socket.write_all(&body[start..]).await;
                        }
                        Mode::StallAfter(n) => {
                            let end = (start + n).min(total);
                            let _ = socket.write_all(&body[start..end]).await;
                            let _ = socket.flush().await;
                            tokio::time::sleep(Duration::from_secs(3600)).await;
                        }
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });
        server
    }

    fn set_mode(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }

    fn sha256(&self) -> String {
        hex::encode(Sha256::digest(self.body.as_slice()))
    }
}

#[derive(Default)]
struct Recorder {
    installed: Mutex<Vec<(String, String)>>,
}

impl DownloadEvents for Recorder {
    fn variant_installed(&self, model_id: &str, variant_id: &str) {
        self.installed
            .lock()
            .unwrap()
            .push((model_id.to_owned(), variant_id.to_owned()));
    }
}

fn plan(server: &TestServer, dest_dir: &Path, sha256: String) -> VariantPlan {
    VariantPlan {
        model_id: "test-model".into(),
        variant_id: "q8".into(),
        dest_dir: dest_dir.to_path_buf(),
        files: vec![PlannedFile {
            repo: "org/test".into(),
            path: "model.gguf".into(),
            url: format!("{}/model.gguf", server.addr),
            size: server.body.len() as u64,
            sha256: Some(sha256),
            gated: false,
            local: String::new(),
        }],
    }
}

fn manager(db: &Arc<Db>, events: Arc<Recorder>) -> DownloadManager {
    DownloadManager::new(db.clone(), events, DownloadConfig::default()).unwrap()
}

async fn wait_for<F: Fn() -> bool>(what: &str, timeout: Duration, f: F) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < timeout, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn only_item(m: &DownloadManager) -> DownloadItem {
    let items = m.list().unwrap();
    assert_eq!(items.len(), 1, "{items:#?}");
    items.into_iter().next().unwrap()
}

fn file_len(p: &PathBuf) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupted_download_resumes_after_restart() {
    let server = TestServer::start(Mode::StallAfter(1024 * 1024)).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open(&dir.path().join("app.db")).unwrap());
    let dest_dir = dir.path().join("models");
    let dest = dest_dir.join("model.gguf");
    let part = part_path(&dest);

    // First run: the transfer stalls after 1 MiB and the app "crashes".
    let events = Arc::new(Recorder::default());
    let first = manager(&db, events.clone());
    first
        .enqueue_variant(&plan(&server, &dest_dir, server.sha256()))
        .unwrap();
    let worker = tokio::spawn(first.clone().run());
    // Up to one write buffer (256 KiB) may still be in memory when the app dies.
    wait_for("partial data on disk", Duration::from_secs(10), || {
        file_len(&part) >= 512 * 1024
    })
    .await;
    worker.abort();
    let _ = worker.await;
    drop(first);
    assert_eq!(
        only_item(&manager(&db, events.clone())).status,
        DownloadStatus::Queued,
        "an interrupted download is re-queued on the next start"
    );

    // Second run: resumes from the .part file with an HTTP Range request.
    server.set_mode(Mode::Normal);
    let second = manager(&db, events.clone());
    let worker = tokio::spawn(second.clone().run());
    wait_for("completed download", Duration::from_secs(20), || {
        only_item(&second).status == DownloadStatus::Completed
    })
    .await;
    second.shutdown();
    let _ = worker.await;

    let ranges = server.ranges.lock().unwrap().clone();
    let resumed_from: u64 = ranges
        .iter()
        .flatten()
        .find_map(|r| r.strip_prefix("bytes=")?.trim_end_matches('-').parse().ok())
        .expect("second request must use Range");
    assert!(
        resumed_from >= 512 * 1024,
        "resumed from {resumed_from}, ranges: {ranges:?}"
    );
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        *server.body,
        "file content intact"
    );
    assert!(!part.exists(), ".part file renamed");
    let installed = db.installed_files_for("test-model", "q8").unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].source, FileSource::Download);
    assert_eq!(
        installed[0].sha256.as_deref(),
        Some(server.sha256().as_str())
    );
    assert_eq!(
        *events.installed.lock().unwrap(),
        vec![("test-model".to_owned(), "q8".to_owned())]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_checksum_is_rejected_after_one_retry() {
    let server = TestServer::start(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open(&dir.path().join("app.db")).unwrap());
    let dest_dir = dir.path().join("models");
    let events = Arc::new(Recorder::default());
    let m = manager(&db, events.clone());
    m.enqueue_variant(&plan(&server, &dest_dir, "0".repeat(64)))
        .unwrap();
    let worker = tokio::spawn(m.clone().run());
    wait_for("failed download", Duration::from_secs(20), || {
        only_item(&m).status == DownloadStatus::Failed
    })
    .await;
    m.shutdown();
    let _ = worker.await;

    let item = only_item(&m);
    assert_eq!(item.error.as_deref(), Some("checksum_mismatch"));
    assert_eq!(item.attempts, 2, "one automatic retry");
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    let dest = dest_dir.join("model.gguf");
    assert!(!dest.exists(), "a bad file is never marked ready");
    assert!(!part_path(&dest).exists(), "bad data is discarded");
    assert!(db.installed_files().unwrap().is_empty());
    assert!(events.installed.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_keeps_partial_file_and_resume_finishes() {
    let server = TestServer::start(Mode::StallAfter(512 * 1024)).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open(&dir.path().join("app.db")).unwrap());
    let dest_dir = dir.path().join("models");
    let dest = dest_dir.join("model.gguf");
    let m = manager(&db, Arc::new(Recorder::default()));
    m.enqueue_variant(&plan(&server, &dest_dir, server.sha256()))
        .unwrap();
    let worker = tokio::spawn(m.clone().run());

    wait_for("downloading", Duration::from_secs(10), || {
        only_item(&m).status == DownloadStatus::Downloading
    })
    .await;
    // Give the stalled transfer time to write its first bytes.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let id = only_item(&m).id;
    m.pause(id).unwrap();
    wait_for("paused", Duration::from_secs(10), || {
        only_item(&m).status == DownloadStatus::Paused
    })
    .await;
    assert!(
        file_len(&part_path(&dest)) > 0,
        "partial data kept while paused"
    );

    server.set_mode(Mode::Normal);
    m.resume(id).unwrap();
    wait_for("completed", Duration::from_secs(20), || {
        only_item(&m).status == DownloadStatus::Completed
    })
    .await;
    m.shutdown();
    let _ = worker.await;
    assert_eq!(std::fs::read(&dest).unwrap(), *server.body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_removes_partial_file() {
    let server = TestServer::start(Mode::StallAfter(256 * 1024)).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open(&dir.path().join("app.db")).unwrap());
    let dest_dir = dir.path().join("models");
    let part = part_path(&dest_dir.join("model.gguf"));
    let m = manager(&db, Arc::new(Recorder::default()));
    m.enqueue_variant(&plan(&server, &dest_dir, server.sha256()))
        .unwrap();
    let worker = tokio::spawn(m.clone().run());
    wait_for("partial data", Duration::from_secs(10), || {
        file_len(&part) > 0 || {
            // the 256 KiB write buffer may not be flushed yet; the DB status is enough
            only_item(&m).status == DownloadStatus::Downloading
        }
    })
    .await;
    let id = only_item(&m).id;
    m.cancel(id).unwrap();
    wait_for("removed", Duration::from_secs(10), || {
        m.list().unwrap().is_empty()
    })
    .await;
    m.shutdown();
    let _ = worker.await;
    assert!(!part.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn files_in_hf_cache_are_reused_without_download() {
    let server = TestServer::start(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("hf-cache");
    let sha = server.sha256();
    let blobs = cache.join("models--org--test").join("blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    std::fs::write(blobs.join(&sha), server.body.as_slice()).unwrap();

    let db = Arc::new(Db::open(&dir.path().join("app.db")).unwrap());
    let events = Arc::new(Recorder::default());
    let m = DownloadManager::new(
        db.clone(),
        events.clone(),
        DownloadConfig {
            hf_cache: Some(cache),
            ..Default::default()
        },
    )
    .unwrap();
    let outcome = m
        .enqueue_variant(&plan(&server, &dir.path().join("models"), sha))
        .unwrap();
    assert_eq!(outcome, EnqueueOutcome::AlreadyInstalled { reused: 1 });
    assert!(m.list().unwrap().is_empty());
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
    let installed = db.installed_files().unwrap();
    assert_eq!(installed[0].source, FileSource::HfCache);
    assert_eq!(events.installed.lock().unwrap().len(), 1);
}

/// Owner's M3 bug: cancelling some files of a multi-file variant must not
/// mark it installed; queueing it again downloads only what is missing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_file_leaves_variant_incomplete_until_requeued() {
    let server = TestServer::start(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open(&dir.path().join("app.db")).unwrap());
    let dest_dir = dir.path().join("models");
    let events = Arc::new(Recorder::default());
    let m = manager(&db, events.clone());
    let mut p = plan(&server, &dest_dir, server.sha256());
    let mut vae = p.files[0].clone();
    vae.path = "split_files/vae/ae.safetensors".into();
    vae.url = format!("{}/ae.safetensors", server.addr);
    p.files.push(vae);
    m.enqueue_variant(&p).unwrap();
    // Cancel the second file before the worker starts.
    let vae_id = m
        .list()
        .unwrap()
        .into_iter()
        .find(|i| i.file_path.ends_with("ae.safetensors"))
        .unwrap()
        .id;
    m.cancel(vae_id).unwrap();

    let worker = tokio::spawn(m.clone().run());
    wait_for("first file", Duration::from_secs(10), || {
        db.installed_files_for("test-model", "q8").unwrap().len() == 1
    })
    .await;
    assert!(
        events.installed.lock().unwrap().is_empty(),
        "a half-downloaded variant is not installed"
    );

    // "Pobierz brakujące": only the cancelled file is queued again.
    match m.enqueue_variant(&p).unwrap() {
        EnqueueOutcome::Queued { files, .. } => assert_eq!(files, 1),
        other => panic!("{other:?}"),
    }
    wait_for("variant installed", Duration::from_secs(10), || {
        !events.installed.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(db.installed_files_for("test-model", "q8").unwrap().len(), 2);
    m.shutdown();
    let _ = worker.await;
}

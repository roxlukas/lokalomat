//! Phone model download: resume after an interruption (HTTP Range), a wrong
//! checksum is rejected and removes the file, a full disk is refused before
//! anything is fetched. Uses a local HTTP server that can stall mid-file.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lokalsi_phone_models::catalog::{PhoneCatalog, PhoneFile};
use lokalsi_phone_models::download::{DownloadError, Progress, client, download};
use lokalsi_phone_models::storage::{is_installed, part_path};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const BODY_LEN: usize = 3 * 1024 * 1024 + 123;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    /// Send this many bytes of the body, then hang without closing.
    StallAfter(usize),
    /// Ignore Range requests and always send the whole file.
    IgnoreRange,
}

struct TestServer {
    url: String,
    body: Arc<Vec<u8>>,
    mode: Arc<Mutex<Mode>>,
    ranges: Arc<Mutex<Vec<Option<String>>>>,
    requests: Arc<AtomicUsize>,
}

impl TestServer {
    async fn start(mode: Mode) -> Self {
        let body: Vec<u8> = (0..BODY_LEN).map(|i| (i * 31 % 251) as u8).collect();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = TestServer {
            url: format!("http://{}/model.litertlm", listener.local_addr().unwrap()),
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
                    let current_mode = *mode.lock().unwrap();
                    let range = range.filter(|_| current_mode != Mode::IgnoreRange);
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
                    match current_mode {
                        Mode::Normal | Mode::IgnoreRange => {
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

    fn file(&self, sha256: Option<String>) -> PhoneFile {
        PhoneFile {
            repo: "test/repo".into(),
            path: "model.litertlm".into(),
            revision: Some("0123456789abcdef".into()),
            size: self.body.len() as u64,
            sha256: sha256.unwrap_or_else(|| hex::encode(Sha256::digest(self.body.as_slice()))),
        }
    }
}

async fn run(
    server: &TestServer,
    file: &PhoneFile,
    dest: &Path,
    available: Option<u64>,
    cancel: &CancellationToken,
) -> Result<Vec<Progress>, DownloadError> {
    let mut seen = Vec::new();
    download(&client(), &server.url, file, dest, available, cancel, |p| {
        seen.push(p)
    })
    .await
    .map(|()| seen)
}

#[tokio::test]
async fn downloads_and_verifies() {
    let server = TestServer::start(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("m").join("model.litertlm");
    let file = server.file(None);
    let progress = run(&server, &file, &dest, None, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), *server.body);
    assert!(!part_path(&dest).exists());
    let last = progress.last().unwrap();
    assert_eq!((last.downloaded, last.total), (file.size, file.size));
}

#[tokio::test]
async fn resumes_with_a_range_request_after_an_interruption() {
    let server = TestServer::start(Mode::StallAfter(1_500_000)).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("model.litertlm");
    let file = server.file(None);

    // First attempt: the server stalls mid-file, we cancel like a user would.
    let cancel = CancellationToken::new();
    let stopper = cancel.clone();
    let part = part_path(&dest);
    tokio::spawn(async move {
        loop {
            if std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) >= 1_500_000 {
                stopper.cancel();
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    let err = run(&server, &file, &dest, None, &cancel).await.unwrap_err();
    assert!(matches!(err, DownloadError::Cancelled));
    assert_eq!(
        std::fs::metadata(part_path(&dest)).unwrap().len(),
        1_500_000
    );
    assert!(!dest.exists());

    // Second attempt continues from the .part file.
    server.set_mode(Mode::Normal);
    run(&server, &file, &dest, None, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), *server.body);
    let ranges = server.ranges.lock().unwrap().clone();
    assert_eq!(ranges, vec![None, Some("bytes=1500000-".to_owned())]);
}

#[tokio::test]
async fn starts_again_when_the_server_ignores_the_range() {
    let server = TestServer::start(Mode::IgnoreRange).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("model.litertlm");
    let file = server.file(None);
    std::fs::write(part_path(&dest), &server.body[..1000]).unwrap();
    run(&server, &file, &dest, None, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), *server.body);
}

#[tokio::test]
async fn rejects_a_wrong_checksum_and_removes_the_file() {
    let server = TestServer::start(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("model.litertlm");
    let file = server.file(Some("0".repeat(64)));
    let err = run(&server, &file, &dest, None, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, DownloadError::Checksum));
    assert_eq!(err.code(), "download_checksum");
    assert!(!dest.exists());
    assert!(!part_path(&dest).exists());
}

#[tokio::test]
async fn refuses_when_there_is_no_room_before_fetching() {
    let server = TestServer::start(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("model.litertlm");
    let file = server.file(None);
    let err = run(&server, &file, &dest, Some(1000), &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, DownloadError::Space { .. }));
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn refuses_an_unsynced_catalog_entry() {
    let server = TestServer::start(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let mut file = server.file(None);
    file.sha256.clear();
    let err = run(
        &server,
        &file,
        &dir.path().join("x"),
        None,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DownloadError::NotSynced));
}

#[test]
fn embedded_catalog_is_valid_and_fully_synced() {
    let catalog = PhoneCatalog::embedded();
    assert!(!catalog.models.is_empty());
    assert!(
        catalog.unsynced().is_empty(),
        "run `cargo run -p catalog-sync`"
    );
    // Exactly one recommended model: the one the app suggests first.
    assert_eq!(catalog.models.iter().filter(|m| m.recommended).count(), 1);
    for m in &catalog.models {
        assert!(m.file.url().starts_with("https://huggingface.co/"));
        assert!(m.file.url().contains(m.file.revision.as_deref().unwrap()));
    }
}

#[test]
fn memory_fit_follows_the_catalog_estimate() {
    use lokalsi_phone_models::catalog::Fit;
    let catalog = PhoneCatalog::embedded();
    let e2b = catalog.find("gemma-4-e2b").unwrap();
    assert_eq!(e2b.fit(7573), Fit::Ok);
    // A phone sold as "6 GB" reports about 5.6 GB of MemTotal: the recommended model fits.
    assert_eq!(e2b.fit(5600), Fit::Ok);
    assert_eq!(e2b.fit(4096), Fit::Low);
    let e4b = catalog.find("gemma-4-e4b").unwrap();
    assert_eq!(e4b.fit(7573), Fit::Low);
}

#[test]
fn installed_means_the_exact_size() {
    let catalog = PhoneCatalog::embedded();
    let model = catalog.find("gemma-4-e2b").unwrap();
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_installed(dir.path(), model));
    let path = lokalsi_phone_models::storage::model_path(dir.path(), model);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"short").unwrap();
    assert!(!is_installed(dir.path(), model));
    lokalsi_phone_models::storage::delete(dir.path(), model).unwrap();
    assert!(!path.exists());
    // Deleting twice is fine.
    lokalsi_phone_models::storage::delete(dir.path(), model).unwrap();
}

#[test]
fn reads_total_memory_from_meminfo() {
    use lokalsi_phone_models::storage::parse_meminfo_total_mb;
    let text = "MemTotal:        7754792 kB
MemFree:          590268 kB
";
    assert_eq!(parse_meminfo_total_mb(text), Some(7573));
    assert_eq!(parse_meminfo_total_mb("nothing here"), None);
}

/// Real HTTPS to Hugging Face (not part of the gate): the first 100 bytes of the
/// default model through the same TLS set-up the phone uses.
/// `cargo test -p lokalsi-phone-models -- --ignored`
#[tokio::test]
#[ignore = "needs the internet"]
async fn live_range_request_over_https() {
    let catalog = PhoneCatalog::embedded();
    let model = catalog.find("gemma-4-e2b").unwrap();
    let resp = client()
        .get(model.file.url())
        .header(reqwest::header::RANGE, "bytes=0-99")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    assert_eq!(resp.bytes().await.unwrap().len(), 100);
}

#[tokio::test]
async fn finishes_a_part_file_that_is_already_complete_without_asking_the_server() {
    let server = TestServer::start(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("model.litertlm");
    let file = server.file(None);
    // Stopped after the last byte, before the rename.
    std::fs::write(part_path(&dest), server.body.as_slice()).unwrap();
    run(&server, &file, &dest, None, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), *server.body);
    assert!(!part_path(&dest).exists());
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_damaged_complete_part_file_is_downloaded_again() {
    let server = TestServer::start(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("model.litertlm");
    let file = server.file(None);
    std::fs::write(part_path(&dest), vec![0u8; BODY_LEN]).unwrap();
    run(&server, &file, &dest, None, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), *server.body);
    // One request for the whole file, no range.
    assert_eq!(server.ranges.lock().unwrap().clone(), vec![None]);
}

#[tokio::test]
async fn room_is_checked_again_when_the_server_restarts_the_file() {
    let server = TestServer::start(Mode::IgnoreRange).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("model.litertlm");
    let file = server.file(None);
    std::fs::write(part_path(&dest), &server.body[..1000]).unwrap();
    // Enough for the rest (size - 1000), not for the whole file.
    let available = Some(file.size - 500);
    let err = run(&server, &file, &dest, available, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, DownloadError::Space { .. }));
    assert!(!dest.exists());
}

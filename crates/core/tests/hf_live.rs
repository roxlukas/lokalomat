//! Live check against Hugging Face (network, ~42 MB): downloads the smallest
//! catalog file through the real plan and verifies its sha256.
//! Run with `cargo test -p lokalsi-core --test hf_live -- --ignored`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lokalsi_core::catalog::Catalog;
use lokalsi_core::db::Db;
use lokalsi_core::downloads::{DownloadConfig, DownloadManager, DownloadStatus, NoEvents};
use lokalsi_core::library::Library;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "network: downloads ~42 MB from huggingface.co"]
async fn smallest_catalog_file_downloads_and_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open(&dir.path().join("app.db")).unwrap());
    let catalog = Catalog::embedded().unwrap();
    let plan = Library::new(&catalog, &db)
        .plan("whisper-tiny", "q8_0", dir.path())
        .unwrap();
    let manager =
        DownloadManager::new(db.clone(), Arc::new(NoEvents), DownloadConfig::default()).unwrap();
    manager.enqueue_variant(&plan).unwrap();
    let worker = tokio::spawn(manager.clone().run());

    let start = Instant::now();
    loop {
        let item = manager.list().unwrap().remove(0);
        match item.status {
            DownloadStatus::Completed => break,
            DownloadStatus::Failed => panic!("download failed: {:?}", item.error),
            _ => {}
        }
        assert!(start.elapsed() < Duration::from_secs(300), "timed out");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    manager.shutdown();
    let _ = worker.await;

    let files = db.installed_files_for("whisper-tiny", "q8_0").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].sha256.as_deref(), plan.files[0].sha256.as_deref());
    assert_eq!(
        std::fs::metadata(&files[0].local_path).unwrap().len(),
        plan.files[0].size
    );
}

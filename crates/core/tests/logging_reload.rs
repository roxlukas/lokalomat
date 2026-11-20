//! Integration test (own process): the log mode switches at runtime and
//! the change is visible in the log file without restarting.

use lokalsi_core::logging::{self, LogMode};

#[test]
fn switching_to_verbose_at_runtime_writes_debug_lines() {
    let dir = tempfile::tempdir().unwrap();
    let handle = logging::init(dir.path(), LogMode::Normal).unwrap();

    tracing::info!("marker-info-normal");
    tracing::debug!("marker-debug-hidden");
    handle.set_mode(LogMode::Verbose).unwrap();
    assert_eq!(handle.mode(), LogMode::Verbose);
    tracing::debug!("marker-debug-visible");
    handle.set_mode(LogMode::Normal).unwrap();
    tracing::debug!("marker-debug-hidden-again");

    drop(handle); // flushes the non-blocking writer

    let mut text = String::new();
    for path in logging::log_files(dir.path()).unwrap() {
        text.push_str(&std::fs::read_to_string(path).unwrap());
    }
    assert!(text.contains("marker-info-normal"), "{text}");
    assert!(text.contains("marker-debug-visible"), "{text}");
    assert!(text.contains("log mode changed"), "{text}");
    assert!(!text.contains("marker-debug-hidden"), "{text}");
}

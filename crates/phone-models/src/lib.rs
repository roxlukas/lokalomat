//! Models that run on the phone itself (Android, LiteRT-LM).
//!
//! Kept separate from `lokalsi-core` on purpose: the phone build has no
//! database or credential store, only this small catalog and a resumable,
//! checksum-verified download.

pub mod catalog;
pub mod download;
pub mod storage;

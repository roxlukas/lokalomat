//! Resumable download of one phone model file: HTTP Range, sha256 checked at
//! the end, a wrong sum deletes the file. Cancelling keeps the `.part` file so
//! the next download continues from it.

use std::io::SeekFrom;
use std::path::Path;
use std::time::Duration;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::catalog::PhoneFile;
use crate::storage::part_path;

pub const USER_AGENT: &str = concat!("Lokalomat-Phone/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("the model has no size or checksum in the catalog yet")]
    NotSynced,
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("the server answered {0}")]
    Status(u16),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not enough free space: need {needed} bytes, have {available}")]
    Space { needed: u64, available: u64 },
    #[error("the downloaded file is damaged (checksum mismatch)")]
    Checksum,
    #[error("cancelled")]
    Cancelled,
}

impl DownloadError {
    /// Stable code the UI translates through `errors.*`.
    pub fn code(&self) -> &'static str {
        match self {
            DownloadError::NotSynced => "model_not_synced",
            DownloadError::Http(_) => "download_network",
            DownloadError::Status(_) => "download_status",
            DownloadError::Io(_) => "download_io",
            DownloadError::Space { .. } => "download_space",
            DownloadError::Checksum => "download_checksum",
            DownloadError::Cancelled => "download_cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
}

/// TLS with the `ring` provider and the bundled Mozilla roots: no OpenSSL and no
/// platform verifier (which needs Android JNI set-up).
fn tls_config() -> rustls::ClientConfig {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    config
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .use_preconfigured_tls(tls_config())
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .expect("HTTP client configuration is valid")
}

/// Downloads `file` to `dest` (a `.part` file is used until the checksum matches).
///
/// `url` is normally `file.url()`; tests pass a local server. `available`
/// is the free space on the target volume, when known.
pub async fn download(
    client: &reqwest::Client,
    url: &str,
    file: &PhoneFile,
    dest: &Path,
    available: Option<u64>,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(Progress),
) -> Result<(), DownloadError> {
    if !file.is_synced() {
        return Err(DownloadError::NotSynced);
    }
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let part = part_path(dest);

    // What is already on disk: a file longer than expected is garbage.
    let mut offset = match tokio::fs::metadata(&part).await {
        Ok(m) if m.len() <= file.size => m.len(),
        Ok(_) => {
            tokio::fs::remove_file(&part).await?;
            0
        }
        Err(_) => 0,
    };

    // Room for what is still to come, counted from `from` bytes on disk.
    let check_space = |from: u64| -> Result<(), DownloadError> {
        match available {
            Some(available) if file.size - from > available => Err(DownloadError::Space {
                needed: file.size - from,
                available,
            }),
            _ => Ok(()),
        }
    };
    check_space(offset)?;

    // The hash has to cover the bytes we resume from.
    let mut hasher = Sha256::new();
    if offset > 0 {
        let mut old = tokio::fs::File::open(&part).await?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = old.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    }

    // Everything is on disk already (stopped before the rename): there is nothing to
    // request, and asking for a range past the end would only get a 416.
    if offset == file.size {
        if hex::encode(hasher.clone().finalize()) == file.sha256 {
            tokio::fs::rename(&part, dest).await?;
            on_progress(Progress {
                downloaded: offset,
                total: file.size,
            });
            tracing::info!(
                file = file.file_name(),
                "phone model finished from a complete part file"
            );
            return Ok(());
        }
        // Damaged: start over.
        tokio::fs::remove_file(&part).await?;
        offset = 0;
        hasher = Sha256::new();
        check_space(0)?;
    }

    let mut req = client.get(url);
    if offset > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let resp = req.send().await?;
    let status = resp.status();
    if offset > 0 && status == reqwest::StatusCode::OK {
        // The server ignored the range: start again from the beginning, which needs
        // room for the whole file.
        offset = 0;
        hasher = Sha256::new();
        check_space(0)?;
    } else if !status.is_success() {
        return Err(DownloadError::Status(status.as_u16()));
    }

    let mut out = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(offset == 0)
        .open(&part)
        .await?;
    out.seek(SeekFrom::Start(offset)).await?;

    let mut downloaded = offset;
    let mut reported = offset;
    on_progress(Progress {
        downloaded,
        total: file.size,
    });
    let mut stream = resp.bytes_stream();
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => {
                out.flush().await?;
                return Err(DownloadError::Cancelled);
            }
            next = stream.next() => next,
        };
        let Some(chunk) = next else { break };
        let chunk = chunk?;
        hasher.update(&chunk);
        out.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if downloaded - reported >= 1 << 20 {
            reported = downloaded;
            on_progress(Progress {
                downloaded,
                total: file.size,
            });
        }
    }
    out.flush().await?;
    drop(out);

    if downloaded != file.size || hex::encode(hasher.finalize()) != file.sha256 {
        tracing::warn!(
            file = file.file_name(),
            downloaded,
            "phone model download failed the size or checksum test"
        );
        let _ = tokio::fs::remove_file(&part).await;
        return Err(DownloadError::Checksum);
    }
    tokio::fs::rename(&part, dest).await?;
    on_progress(Progress {
        downloaded,
        total: file.size,
    });
    tracing::info!(
        file = file.file_name(),
        bytes = downloaded,
        "phone model downloaded"
    );
    Ok(())
}

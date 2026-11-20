//! Hugging Face Hub helpers: download URLs, the public API (sizes and
//! sha256 of LFS files) and lookups in the local HF cache.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const HF_ENDPOINT: &str = "https://huggingface.co";
pub const USER_AGENT: &str = concat!("StudioLokalsi/", env!("CARGO_PKG_VERSION"));

/// HTTP client type used by the helpers in this module.
pub type Client = reqwest::Client;

/// `https://huggingface.co/{repo}/resolve/{revision}/{path}`.
pub fn resolve_url(repo: &str, revision: Option<&str>, path: &str) -> String {
    format!(
        "{HF_ENDPOINT}/{repo}/resolve/{}/{}",
        revision.unwrap_or("main"),
        encode_path(path)
    )
}

fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// True for URLs that should carry the user's HF token.
pub fn is_hf_url(url: &str) -> bool {
    url.starts_with(&format!("{HF_ENDPOINT}/"))
}

#[derive(Debug, thiserror::Error)]
pub enum HfError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Hugging Face returned {status} for {url}")]
    Status { status: u16, url: String },
    #[error("file `{path}` not found in {repo}")]
    NotFound { repo: String, path: String },
    #[error("invalid repository or file name")]
    InvalidName,
    #[error("file is too large to hash remotely ({0} bytes)")]
    TooLarge(u64),
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .expect("HTTP client configuration is valid")
}

#[derive(Debug, Clone, Deserialize)]
struct ModelInfo {
    sha: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    size: u64,
    lfs: Option<LfsInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct LfsInfo {
    oid: String,
    size: u64,
}

/// Facts about one file in a repository, as reported by the HF API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub size: u64,
    /// sha256 of the content for LFS files; `None` for small git files.
    pub sha256: Option<String>,
    /// Commit the information belongs to.
    pub revision: String,
}

fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok = |s: Option<&str>| {
        s.is_some_and(|s| {
            !s.is_empty()
                && s != ".."
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        })
    };
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> Result<T, HfError> {
    let mut req = client.get(url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(HfError::Status {
            status: resp.status().as_u16(),
            url: url.to_owned(),
        });
    }
    Ok(resp.json().await?)
}

/// Current commit of `repo` (main branch).
pub async fn repo_revision(
    client: &reqwest::Client,
    repo: &str,
    token: Option<&str>,
) -> Result<String, HfError> {
    if !valid_repo(repo) {
        return Err(HfError::InvalidName);
    }
    let info: ModelInfo =
        get_json(client, &format!("{HF_ENDPOINT}/api/models/{repo}"), token).await?;
    info.sha.ok_or_else(|| HfError::NotFound {
        repo: repo.to_owned(),
        path: String::new(),
    })
}

/// Size and sha256 of `path` in `repo` at `revision` (or the current commit).
pub async fn file_info(
    client: &reqwest::Client,
    repo: &str,
    path: &str,
    revision: Option<&str>,
    token: Option<&str>,
) -> Result<FileInfo, HfError> {
    if !valid_repo(repo) || path.is_empty() || path.contains("..") {
        return Err(HfError::InvalidName);
    }
    let revision = match revision {
        Some(r) => r.to_owned(),
        None => repo_revision(client, repo, token).await?,
    };
    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut url = format!("{HF_ENDPOINT}/api/models/{repo}/tree/{revision}");
    if !dir.is_empty() {
        url.push('/');
        url.push_str(&encode_path(dir));
    }
    let entries: Vec<TreeEntry> = get_json(client, &url, token).await?;
    let entry = entries
        .into_iter()
        .find(|e| e.kind == "file" && e.path == path)
        .ok_or_else(|| HfError::NotFound {
            repo: repo.to_owned(),
            path: path.to_owned(),
        })?;
    Ok(match entry.lfs {
        Some(lfs) => FileInfo {
            size: lfs.size,
            sha256: Some(lfs.oid.to_lowercase()),
            revision,
        },
        None => FileInfo {
            size: entry.size,
            sha256: None,
            revision,
        },
    })
}

/// Downloads a small (non-LFS) file and returns its sha256. The HF API only
/// reports git blob ids for such files, so the sum has to be computed.
pub async fn sha256_of_url(
    client: &reqwest::Client,
    url: &str,
    max_bytes: u64,
    token: Option<&str>,
) -> Result<String, HfError> {
    let mut req = client.get(url);
    if let Some(t) = token.filter(|_| is_hf_url(url)) {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(HfError::Status {
            status: resp.status().as_u16(),
            url: url.to_owned(),
        });
    }
    if let Some(len) = resp.content_length().filter(|l| *l > max_bytes) {
        return Err(HfError::TooLarge(len));
    }
    let bytes = resp.bytes().await?;
    if bytes.len() as u64 > max_bytes {
        return Err(HfError::TooLarge(bytes.len() as u64));
    }
    Ok(hex::encode(Sha256::digest(&bytes)))
}

/// Local Hugging Face cache (`HF_HUB_CACHE`, `HF_HOME/hub` or `~/.cache/huggingface/hub`).
pub fn cache_dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    env("HF_HUB_CACHE")
        .or_else(|| env("HF_HOME").map(|h| h.join("hub")))
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache").join("huggingface").join("hub")))
}

/// A file already present in the HF cache. LFS blobs are stored under their
/// sha256, so a hit with the expected size is the exact file we need.
pub fn find_in_cache(cache: &Path, repo: &str, sha256: &str, size: u64) -> Option<PathBuf> {
    if !valid_repo(repo) || !crate::catalog::is_sha256(sha256) {
        return None;
    }
    let blob = cache
        .join(format!("models--{}", repo.replace('/', "--")))
        .join("blobs")
        .join(sha256);
    let meta = std::fs::metadata(&blob).ok()?;
    (meta.is_file() && meta.len() == size).then_some(blob)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_resolve_urls() {
        assert_eq!(
            resolve_url("org/repo", Some("abc"), "dir/My File.gguf"),
            "https://huggingface.co/org/repo/resolve/abc/dir/My%20File.gguf"
        );
        assert_eq!(
            resolve_url("org/repo", None, "a.bin"),
            "https://huggingface.co/org/repo/resolve/main/a.bin"
        );
    }

    #[test]
    fn validates_repo_names() {
        assert!(valid_repo("speakleash/Bielik-11B-v3.1-Instruct-GGUF"));
        assert!(!valid_repo("../etc"));
        assert!(!valid_repo("a/b/c"));
        assert!(!valid_repo("a"));
    }

    #[test]
    fn finds_blobs_in_cache() {
        let dir = tempfile::tempdir().unwrap();
        let sha = "a".repeat(64);
        let blobs = dir.path().join("models--org--repo").join("blobs");
        std::fs::create_dir_all(&blobs).unwrap();
        std::fs::write(blobs.join(&sha), b"12345").unwrap();
        assert!(find_in_cache(dir.path(), "org/repo", &sha, 5).is_some());
        assert!(
            find_in_cache(dir.path(), "org/repo", &sha, 6).is_none(),
            "size must match"
        );
        assert!(find_in_cache(dir.path(), "org/other", &sha, 5).is_none());
    }
}

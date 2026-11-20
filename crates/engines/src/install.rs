//! Engine installation: official release assets only, each verified against
//! the sha256 from the manifest before it is unpacked (PRD §10).

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::manifest::{Build, EngineManifest};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("server returned HTTP {0}")]
    Status(u16),
    #[error("checksum mismatch for {0}")]
    Checksum(String),
    #[error("{0} has no checksum in the manifest")]
    NoChecksum(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("cannot unpack {name}: {reason}")]
    Unpack { name: String, reason: String },
    #[error("{0} not found in the downloaded archives")]
    MissingBinary(String),
    #[error("installation cancelled")]
    Cancelled,
}

/// Directory of an installed build: `<engines_dir>/<engine>/<tag>-<build>`.
pub fn build_dir(engines_dir: &Path, manifest: &EngineManifest, build: &Build) -> PathBuf {
    engines_dir
        .join(&manifest.id)
        .join(format!("{}-{}", manifest.install.tag, build.id))
}

/// Executable name on this OS (`llama-server.exe` on Windows).
pub fn exe_name(cmd: &str) -> String {
    if cfg!(windows) {
        format!("{cmd}.exe")
    } else {
        cmd.to_owned()
    }
}

/// Finds the launch binary inside an installed build (archives may wrap
/// their content in a top-level folder, and Unix archives such as Node.js
/// keep it in `bin/`).
pub fn find_binary(dir: &Path, cmd: &str) -> Option<PathBuf> {
    let name = exe_name(cmd);
    let here = |d: &Path| {
        [d.join(&name), d.join("bin").join(&name)]
            .into_iter()
            .find(|p| p.is_file())
    };
    if let Some(found) = here(dir) {
        return Some(found);
    }
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .find_map(|e| here(&e.path()))
}

/// Plug and play for small helper programs (e.g. `mcp-grafana`): returns the
/// installed binary, downloading and verifying it first when it is missing.
pub async fn ensure_package(
    client: &reqwest::Client,
    manifest: &EngineManifest,
    platform: &str,
    dir: &Path,
    cancel: &CancellationToken,
) -> Result<PathBuf, InstallError> {
    let build = manifest
        .builds_for(platform, None)
        .into_iter()
        .next()
        .ok_or_else(|| InstallError::MissingBinary(format!("{} for {platform}", manifest.id)))?;
    if let Some(binary) = find_binary(&build_dir(dir, manifest, build), &manifest.launch.cmd) {
        return Ok(binary);
    }
    tracing::info!(package = %manifest.id, version = %manifest.version, "installing built-in program");
    install(client, manifest, build, dir, cancel, &|_, _| {}).await
}

/// Downloads, verifies and unpacks `build`. `progress(done, total)` is called
/// as bytes arrive. Returns the path of the launch binary.
pub async fn install(
    client: &reqwest::Client,
    manifest: &EngineManifest,
    build: &Build,
    engines_dir: &Path,
    cancel: &CancellationToken,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<PathBuf, InstallError> {
    let target = build_dir(engines_dir, manifest, build);
    let staging = target.with_extension("partial");
    let downloads = engines_dir.join("downloads");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    fs::create_dir_all(&downloads)?;

    let total = build.total_size();
    let mut done = 0u64;
    for file in &build.files {
        if !lokalsi_core::catalog::is_sha256(&file.sha256) {
            return Err(InstallError::NoChecksum(file.name.clone()));
        }
        let archive = downloads.join(&file.name);
        if let Some(parent) = archive.parent() {
            fs::create_dir_all(parent)?;
        }
        let url = manifest.asset_url(file);
        tracing::info!(engine = %manifest.id, build = %build.id, file = %file.name, size = file.size, "downloading engine archive");
        tracing::debug!(%url, "engine archive url");
        download_verified(client, &url, &archive, &file.sha256, cancel, &mut |n| {
            progress(done + n, total);
        })
        .await?;
        done += file.size;
        let (name, staging2, archive2) = (file.name.clone(), staging.clone(), archive.clone());
        tokio::task::spawn_blocking(move || unpack(&archive2, &staging2, &name))
            .await
            .map_err(|e| InstallError::Unpack {
                name: file.name.clone(),
                reason: e.to_string(),
            })??;
        let _ = fs::remove_file(&archive);
    }

    if find_binary(&staging, &manifest.launch.cmd).is_none() {
        return Err(InstallError::MissingBinary(exe_name(&manifest.launch.cmd)));
    }
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::rename(&staging, &target)?;
    let binary = find_binary(&target, &manifest.launch.cmd)
        .ok_or_else(|| InstallError::MissingBinary(exe_name(&manifest.launch.cmd)))?;
    tracing::info!(engine = %manifest.id, build = %build.id, path = %binary.display(), "engine installed");
    Ok(binary)
}

/// Downloads `url` to `dest` and checks its sha256 (agents' source archives).
pub async fn download_checked(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha256: &str,
) -> Result<(), InstallError> {
    if !lokalsi_core::catalog::is_sha256(sha256) {
        return Err(InstallError::NoChecksum(url.to_owned()));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    download_verified(
        client,
        url,
        dest,
        sha256,
        &CancellationToken::new(),
        &mut |_| {},
    )
    .await
}

/// Unpacks a `.zip` / `.tar.gz` archive into `dest` (paths that would leave
/// it are refused).
pub fn unpack_archive(archive: &Path, dest: &Path) -> Result<(), InstallError> {
    let name = archive
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    unpack(archive, dest, &name)
}

async fn download_verified(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha256: &str,
    cancel: &CancellationToken,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<(), InstallError> {
    let resp = tokio::select! {
        r = client.get(url).send() => r?,
        _ = cancel.cancelled() => return Err(InstallError::Cancelled),
    };
    if !resp.status().is_success() {
        return Err(InstallError::Status(resp.status().as_u16()));
    }
    let mut file = tokio::fs::File::create(dest).await?;
    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();
    let mut n = 0u64;
    loop {
        let chunk = tokio::select! {
            c = stream.next() => c,
            _ = cancel.cancelled() => {
                drop(file);
                let _ = tokio::fs::remove_file(dest).await;
                return Err(InstallError::Cancelled);
            }
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        n += chunk.len() as u64;
        progress(n);
    }
    file.flush().await?;
    drop(file);
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(sha256) {
        let _ = tokio::fs::remove_file(dest).await;
        tracing::error!(%url, expected = sha256, %actual, "engine archive checksum mismatch");
        return Err(InstallError::Checksum(
            dest.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        ));
    }
    Ok(())
}

/// Unpacks `.zip` or `.tar.gz` into `dest`, refusing paths that escape it.
/// Any other file is a bare program (Droid ships one binary per platform)
/// and is copied as it is.
fn unpack(archive: &Path, dest: &Path, name: &str) -> Result<(), InstallError> {
    let err = |reason: String| InstallError::Unpack {
        name: name.to_owned(),
        reason,
    };
    let base = Path::new(name)
        .file_name()
        .ok_or_else(|| err("no file name".into()))?;
    let bare = !name.ends_with(".zip")
        && !name.ends_with(".tar.gz")
        && !name.ends_with(".tgz")
        && (Path::new(name).extension().is_none() || name.ends_with(".exe"));
    if bare {
        let out = dest.join(base);
        fs::copy(archive, &out)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&out, fs::Permissions::from_mode(0o755))?;
        }
        return Ok(());
    }
    if name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(File::open(archive)?).map_err(|e| err(e.to_string()))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| err(e.to_string()))?;
            let Some(rel) = entry.enclosed_name() else {
                return Err(err(format!("unsafe path {:?}", entry.name())));
            };
            let out = dest.join(rel);
            if entry.is_dir() {
                fs::create_dir_all(&out)?;
                continue;
            }
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = File::create(&out)?;
            io::copy(&mut entry, &mut f)?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&out, fs::Permissions::from_mode(mode))?;
            }
        }
        Ok(())
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let gz = flate2::read::GzDecoder::new(File::open(archive)?);
        let mut tar = tar::Archive::new(gz);
        tar.set_preserve_permissions(true);
        for entry in tar.entries()? {
            let mut entry = entry?;
            // `unpack_in` refuses entries that would land outside `dest`.
            if !entry.unpack_in(dest)? {
                return Err(err(format!("unsafe path {}", entry.path()?.display())));
            }
        }
        Ok(())
    } else {
        Err(err("unsupported archive format".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn finds_binaries_in_bin_folders() {
        // Node.js on Linux/macOS: node-v26.13.1-linux-x64/bin/node.
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("node-v26.13.1-linux-x64").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(exe_name("node")), b"x").unwrap();
        assert_eq!(
            find_binary(dir.path(), "node"),
            Some(bin.join(exe_name("node")))
        );
        assert_eq!(find_binary(dir.path(), "npm"), None);
    }

    #[test]
    fn unpacks_zip_and_finds_binary_in_subfolder() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file(format!("llama-b1/{}", exe_name("llama-server")), opts)
                .unwrap();
            z.write_all(b"bin").unwrap();
            z.start_file("llama-b1/ggml.dll", opts).unwrap();
            z.write_all(b"lib").unwrap();
            z.finish().unwrap();
        }
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        unpack(&archive, &out, "a.zip").unwrap();
        let bin = find_binary(&out, "llama-server").unwrap();
        assert!(bin.ends_with(exe_name("llama-server")));
    }

    #[test]
    fn bare_programs_are_copied() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("droid.bin");
        std::fs::write(&file, b"x").unwrap();
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let name = format!("windows/x64/{}", exe_name("droid"));
        unpack(&file, &out, &name).unwrap();
        assert!(find_binary(&out, "droid").is_some());
        assert!(unpack(&file, &out, "x.rar").is_err());
    }

    #[test]
    fn refuses_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("evil.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&archive).unwrap());
            z.start_file("../evil.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        assert!(unpack(&archive, &out, "evil.zip").is_err());
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn unpacks_tar_gz() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                File::create(&archive).unwrap(),
                flate2::Compression::fast(),
            );
            let mut tar = tar::Builder::new(gz);
            let data = b"bin";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(
                &mut header,
                format!("llama-b1/{}", exe_name("llama-server")),
                &data[..],
            )
            .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        unpack(&archive, &out, "a.tar.gz").unwrap();
        assert!(find_binary(&out, "llama-server").is_some());
    }
}

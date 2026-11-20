//! `cargo run -p catalog-sync [-- --refresh | --check]`
//!
//! Fills `size`, `sha256` and `revision` of every file in
//! `catalog/models.json` from the Hugging Face API and recomputes
//! `memory_estimate_mb` (PRD §6.2: entering sums by hand is forbidden).
//!
//! * LFS files: sha256 = `lfs.oid` reported by the API.
//! * Small git files (configs, tokenizers): the API only has a git blob id,
//!   so the file is downloaded (≤ 16 MiB) and hashed.
//!
//! An `HF_TOKEN` environment variable is used for gated repositories.
//!
//! It also checks that every `benchmark` mapping exists in the published AI
//! Transformation local-LLM results (the app fetches them live, docs/adr/0008).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use lokalsi_core::catalog::{Catalog, estimate_memory_mb};
use lokalsi_core::hf;

const MAX_HASHED_DOWNLOAD: u64 = 16 * 1024 * 1024;

#[derive(Parser)]
#[command(about = "Sync catalog/models.json with the Hugging Face API")]
struct Args {
    /// Catalog file (defaults to catalog/models.json in the repository).
    #[arg(long)]
    catalog: Option<PathBuf>,
    /// Re-fetch every file, not only unsynced ones (moves revisions to the latest commit).
    #[arg(long)]
    refresh: bool,
    /// Do not write; exit with an error if anything is unsynced.
    #[arg(long)]
    check: bool,
}

fn default_catalog() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("catalog")
        .join("models.json")
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let path = args.catalog.unwrap_or_else(default_catalog);
    match run(&path, args.refresh, args.check).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("catalog-sync: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(
    path: &PathBuf,
    refresh: bool,
    check: bool,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    let mut catalog = Catalog::parse(&text)?;

    if check {
        let missing = catalog.unsynced_files();
        if missing.is_empty() {
            println!("catalog is fully synced");
            return Ok(ExitCode::SUCCESS);
        }
        for m in &missing {
            println!("unsynced: {m}");
        }
        return Ok(ExitCode::FAILURE);
    }

    let client = hf::client();
    let token = std::env::var("HF_TOKEN").ok().filter(|t| !t.is_empty());
    let token = token.as_deref();
    let mut revisions: HashMap<String, String> = HashMap::new();
    let mut infos: HashMap<(String, String), (u64, String, String)> = HashMap::new();
    let mut updated = 0usize;
    let mut failed = 0usize;

    for model in &mut catalog.models {
        for variant in &mut model.variants {
            for file in &mut variant.files {
                let synced = file.size > 0 && file.has_valid_sha256() && file.revision.is_some();
                if synced && !refresh {
                    continue;
                }
                let key = (file.repo.clone(), file.path.clone());
                if !infos.contains_key(&key) {
                    match fetch(
                        &client,
                        &mut revisions,
                        &file.repo,
                        &file.path,
                        refresh,
                        file.revision.as_deref(),
                        token,
                    )
                    .await
                    {
                        Ok(info) => {
                            infos.insert(key.clone(), info);
                        }
                        Err(e) => {
                            eprintln!("  ! {}/{}: {e}", file.repo, file.path);
                            failed += 1;
                            continue;
                        }
                    }
                }
                let (size, sha, rev) = infos[&key].clone();
                if file.size != size
                    || file.sha256 != sha
                    || file.revision.as_deref() != Some(rev.as_str())
                {
                    println!("  {}/{} → {size} B, {}…", file.repo, file.path, &sha[..12]);
                    file.size = size;
                    file.sha256 = sha;
                    file.revision = Some(rev);
                    updated += 1;
                }
            }
            if variant.files.iter().all(|f| f.size > 0) {
                variant.memory_estimate_mb =
                    estimate_memory_mb(model.modality, variant.total_size());
            }
        }
    }

    let json = serde_json::to_string_pretty(&catalog)? + "\n";
    if json != text {
        std::fs::write(path, json)?;
    }
    println!(
        "{updated} file(s) updated, {failed} failed → {}",
        path.display()
    );

    let engines_dir = path.with_file_name("engines");
    match sync_engines(&client, &engines_dir).await {
        Ok(n) => println!(
            "engines: {n} release file(s) checked → {}",
            engines_dir.display()
        ),
        Err(e) => {
            eprintln!("  ! engines: {e}");
            failed += 1;
        }
    }

    match check_benchmark(&client, &catalog).await {
        Ok(n) => println!("benchmark: {n} mapped model(s) found in the published results"),
        Err(e) => {
            eprintln!("  ! benchmark: {e}");
            failed += 1;
        }
    }
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn fetch(
    client: &lokalsi_core::hf::Client,
    revisions: &mut HashMap<String, String>,
    repo: &str,
    path: &str,
    refresh: bool,
    pinned: Option<&str>,
    token: Option<&str>,
) -> Result<(u64, String, String), Box<dyn std::error::Error>> {
    let revision = match (pinned, refresh) {
        (Some(r), false) => r.to_owned(),
        _ => match revisions.get(repo) {
            Some(r) => r.clone(),
            None => {
                let r = hf::repo_revision(client, repo, token).await?;
                revisions.insert(repo.to_owned(), r.clone());
                r
            }
        },
    };
    let info = hf::file_info(client, repo, path, Some(&revision), token).await?;
    let sha = match info.sha256 {
        Some(s) => s,
        None => {
            let url = hf::resolve_url(repo, Some(&revision), path);
            hf::sha256_of_url(client, &url, MAX_HASHED_DOWNLOAD, token).await?
        }
    };
    // Gated repos mask `lfs.oid` ("****…") for requests without an accepted licence.
    if !lokalsi_core::catalog::is_sha256(&sha) {
        return Err(
            "checksum hidden (gated repo) - set HF_TOKEN of an account that accepted the licence"
                .into(),
        );
    }
    Ok((info.size, sha, revision))
}

/// Checks that every catalog `benchmark` mapping exists in the published results.
async fn check_benchmark(
    client: &hf::Client,
    catalog: &Catalog,
) -> Result<usize, Box<dyn std::error::Error>> {
    let bench = lokalsi_core::benchmark::Benchmark::fetch(client).await?;
    let missing = bench.missing_mappings(&catalog.models);
    if !missing.is_empty() {
        return Err(format!("mapped names not in the benchmark: {missing:?}").into());
    }
    Ok(catalog
        .models
        .iter()
        .filter(|m| !m.benchmark.is_empty())
        .count())
}

/// `url` sources (e.g. nodejs.org): sha256 from the folder's `SHASUMS256.txt`,
/// size from a `HEAD` request.
async fn sync_url_source(
    client: &hf::Client,
    manifest: &mut lokalsi_engines::manifest::EngineManifest,
) -> Result<usize, Box<dyn std::error::Error>> {
    let base = manifest
        .install
        .base_url
        .clone()
        .ok_or_else(|| format!("{}: url source without base_url", manifest.id))?;
    let per_file = manifest.install.checksums.as_deref() == Some("per-file");
    let sums = if per_file {
        String::new()
    } else {
        client
            .get(format!("{base}SHASUMS256.txt"))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?
    };
    let mut checked = 0;
    for build in &mut manifest.builds {
        for file in &mut build.files {
            let sha = if per_file {
                // `<file>.sha256`: the hex sum, optionally followed by the name.
                let text = client
                    .get(format!("{base}{}.sha256", file.name))
                    .send()
                    .await?
                    .error_for_status()?
                    .text()
                    .await?;
                text.split_whitespace()
                    .next()
                    .filter(|s| lokalsi_core::catalog::is_sha256(s))
                    .map(str::to_lowercase)
                    .ok_or_else(|| format!("{}: no sha256 in {}.sha256", file.name, file.name))?
            } else {
                sums.lines()
                    .find_map(|l| {
                        let (sha, name) = l.split_once("  ")?;
                        (name.trim() == file.name).then(|| sha.trim().to_lowercase())
                    })
                    .ok_or_else(|| format!("{} not in {base}SHASUMS256.txt", file.name))?
            };
            let head = client
                .head(format!("{base}{}", file.name))
                .send()
                .await?
                .error_for_status()?;
            let size = head
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| format!("{}: no size", file.name))?;
            if file.size != size || file.sha256 != sha {
                println!("  {} → {size} B, {}…", file.name, &sha[..12]);
                file.size = size;
                file.sha256 = sha;
            }
            checked += 1;
        }
    }
    Ok(checked)
}

/// Fills `size` and `sha256` of every engine release asset from the GitHub API
/// (`digest` field of the release assets).
async fn sync_engines(
    client: &hf::Client,
    dir: &std::path::Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut checked = 0;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        let mut manifest: lokalsi_engines::manifest::EngineManifest = serde_json::from_str(&text)?;
        if manifest.install.source == "url" {
            checked += sync_url_source(client, &mut manifest).await?;
            let json = serde_json::to_string_pretty(&manifest)?
                + "
";
            if json != text {
                std::fs::write(&path, json)?;
            }
            continue;
        }
        let url = format!(
            "https://api.github.com/repos/{}/releases/tags/{}",
            manifest.install.repo, manifest.install.tag
        );
        let release: serde_json::Value = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let assets = release["assets"]
            .as_array()
            .ok_or("release has no assets")?;
        for build in &mut manifest.builds {
            for file in &mut build.files {
                let asset = assets
                    .iter()
                    .find(|a| a["name"].as_str() == Some(file.name.as_str()))
                    .ok_or_else(|| {
                        format!("{} not in release {}", file.name, manifest.install.tag)
                    })?;
                let size = asset["size"].as_u64().ok_or("asset without size")?;
                let sha = asset["digest"]
                    .as_str()
                    .and_then(|d| d.strip_prefix("sha256:"))
                    .ok_or_else(|| format!("{} has no sha256 digest", file.name))?
                    .to_lowercase();
                if file.size != size || file.sha256 != sha {
                    println!("  {} → {size} B, {}…", file.name, &sha[..12]);
                    file.size = size;
                    file.sha256 = sha;
                }
                checked += 1;
            }
        }
        let json = serde_json::to_string_pretty(&manifest)? + "\n";
        if json != text {
            std::fs::write(&path, json)?;
        }
    }
    Ok(checked)
}

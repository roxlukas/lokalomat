//! System libraries the engine binaries load at start (ADR 0017).
//!
//! The upstream Linux builds of llama.cpp, whisper.cpp and sd.cpp link
//! against `libgomp.so.1` (OpenMP, package `libgomp1`), which a minimal
//! Ubuntu install does not have. The dynamic loader then stops the process
//! before `main` with "error while loading shared libraries: …". This module
//! recognises that line so the user gets the fix instead of a generic
//! "could not start".

/// A shared library the engine needs but the system does not have.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("missing system library {lib} (package {package})")]
pub struct MissingLibrary {
    /// Soname from the loader message, e.g. `libgomp.so.1`.
    pub lib: String,
    /// Ubuntu package that provides it, e.g. `libgomp1`.
    pub package: String,
}

const LOADER_MARKER: &str = "error while loading shared libraries:";

/// Packages whose name does not follow the soname rule in [`apt_package`].
const KNOWN: &[(&str, &str)] = &[
    ("libgomp.so.1", "libgomp1"),
    ("libvulkan.so.1", "libvulkan1"),
    ("libstdc++.so.6", "libstdc++6"),
    ("libgcc_s.so.1", "libgcc-s1"),
];

/// Finds the glibc loader error in engine output (any line).
pub fn detect(output: &str) -> Option<MissingLibrary> {
    output.lines().find_map(|line| {
        let rest = &line[line.find(LOADER_MARKER)? + LOADER_MARKER.len()..];
        let lib = rest.split(':').next()?.trim();
        (!lib.is_empty() && lib.contains(".so")).then(|| MissingLibrary {
            lib: lib.to_owned(),
            package: apt_package(lib),
        })
    })
}

/// Ubuntu package name for a soname: a known mapping, else Debian's library
/// naming rule (`libfoo.so.3` → `libfoo3`).
pub fn apt_package(lib: &str) -> String {
    if let Some((_, pkg)) = KNOWN.iter().find(|(l, _)| *l == lib) {
        return (*pkg).to_owned();
    }
    match lib.split_once(".so") {
        Some((stem, version)) => {
            let version = version.trim_start_matches('.');
            let major = version.split('.').next().unwrap_or("");
            let sep = if stem.ends_with(|c: char| c.is_ascii_digit()) && !major.is_empty() {
                "-"
            } else {
                ""
            };
            format!("{stem}{sep}{major}")
                .to_lowercase()
                .replace('_', "-")
        }
        None => lib.to_owned(),
    }
}

/// Runs `binary --version` and reports a library the loader could not find.
/// Linux only; elsewhere (and when the binary starts) returns `None`.
pub async fn probe(binary: &std::path::Path) -> Option<MissingLibrary> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let mut cmd = tokio::process::Command::new(binary);
    cmd.arg("--version")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(dir) = binary.parent() {
        cmd.current_dir(dir);
    }
    let out = tokio::time::timeout(std::time::Duration::from_secs(20), cmd.output())
        .await
        .ok()?
        .ok()?;
    let missing = detect(&String::from_utf8_lossy(&out.stderr))?;
    tracing::warn!(binary = %binary.display(), lib = %missing.lib, package = %missing.package, "engine needs a system library that is not installed");
    Some(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_loader_error() {
        let out = "./llama-server: error while loading shared libraries: libgomp.so.1: \
                   cannot open shared object file: No such file or directory";
        assert_eq!(
            detect(out),
            Some(MissingLibrary {
                lib: "libgomp.so.1".into(),
                package: "libgomp1".into()
            })
        );
        assert_eq!(detect("error: model not found"), None);
        assert_eq!(detect(""), None);
    }

    #[test]
    fn finds_it_among_other_lines() {
        let out = "--- 1 /x/whisper-cli\n/x/whisper-cli: error while loading shared libraries: \
                   libvulkan.so.1: cannot open shared object file\n--- exit Some(127)";
        assert_eq!(detect(out).unwrap().package, "libvulkan1");
    }

    #[test]
    fn guesses_package_names() {
        assert_eq!(apt_package("libssl.so.3"), "libssl3");
        assert_eq!(apt_package("libgomp.so.1"), "libgomp1");
        assert_eq!(apt_package("libfoo2.so.0"), "libfoo2-0");
        assert_eq!(apt_package("libgcc_s.so.1"), "libgcc-s1");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn probe_reports_a_missing_library() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("fake-engine");
        std::fs::write(
            &bin,
            "#!/bin/sh\necho \"$0: error while loading shared libraries: libgomp.so.1: \
             cannot open shared object file: No such file or directory\" >&2\nexit 127\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(probe(&bin).await.unwrap().package, "libgomp1");
    }
}

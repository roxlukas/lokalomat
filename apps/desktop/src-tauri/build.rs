//! Build script: Tauri plus the Microsoft C++ runtime for engines (ADR 0043).
//!
//! The upstream Windows builds of llama.cpp, whisper.cpp and sd.cpp need the
//! Visual C++ runtime DLLs, which a fresh Windows does not have. We copy them
//! from the build machine's MSVC redistributable folder into the binary
//! (`include_bytes!`); `src/vcrt.rs` unpacks them at start.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Runtime DLLs the engine binaries import (checked against llama.cpp b12110,
/// whisper.cpp and sd.cpp builds). `vcomp140.dll` is the OpenMP runtime.
const DLLS: &[&str] = &[
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "msvcp140.dll",
    "msvcp140_1.dll",
    "msvcp140_2.dll",
    "msvcp140_atomic_wait.dll",
    "msvcp140_codecvt_ids.dll",
    "vcomp140.dll",
];

fn main() {
    embed_vc_runtime();
    tauri_build::build()
}

fn embed_vc_runtime() {
    println!("cargo:rerun-if-env-changed=LOKALSI_VCRT_DIR");
    println!("cargo:rerun-if-env-changed=VCToolsRedistDir");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let mut code = String::from("pub const FILES: &[(&str, &[u8])] = &[\n");
    if target_os == "windows" {
        let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("aarch64") => "arm64",
            Ok("x86") => "x86",
            _ => "x64",
        };
        let found: Vec<(&str, PathBuf)> = DLLS
            .iter()
            .filter_map(|name| find_dll(name, arch).map(|p| (*name, p)))
            .collect();
        let missing: Vec<&str> = DLLS
            .iter()
            .filter(|n| !found.iter().any(|(f, _)| f == *n))
            .copied()
            .collect();
        if !missing.is_empty() {
            let msg = format!(
                "Visual C++ runtime DLLs not found: {}. Install the MSVC build tools \
                 or set LOKALSI_VCRT_DIR to a folder with them.",
                missing.join(", ")
            );
            // A release without them breaks every engine on a fresh Windows.
            if std::env::var("PROFILE").as_deref() == Ok("release") {
                panic!("{msg}");
            }
            println!("cargo:warning={msg}");
        }
        for (name, path) in found {
            println!("cargo:rerun-if-changed={}", path.display());
            let _ = writeln!(
                code,
                "    ({name:?}, include_bytes!({:?})),",
                path.display().to_string()
            );
        }
    }
    code.push_str("];\n");
    std::fs::write(out.join("vcrt.rs"), code).expect("write vcrt.rs");
}

/// Looks for one DLL: `LOKALSI_VCRT_DIR`, the redist folder of the active
/// MSVC toolset, the newest Visual Studio found by `vswhere`.
fn find_dll(name: &str, arch: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("LOKALSI_VCRT_DIR") {
        dirs.push(dir.into());
    }
    if let Ok(redist) = std::env::var("VCToolsRedistDir") {
        dirs.extend(crt_dirs(&Path::new(&redist).join(arch)));
    }
    for install in vs_installations() {
        let msvc = install.join("VC").join("Redist").join("MSVC");
        let mut versions: Vec<PathBuf> = std::fs::read_dir(&msvc)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join(arch).is_dir())
            .collect();
        versions.sort();
        for version in versions.iter().rev() {
            dirs.extend(crt_dirs(&version.join(arch)));
        }
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// `Microsoft.VC14x.CRT` and `Microsoft.VC14x.OpenMP` under an arch folder.
fn crt_dirs(arch_dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(arch_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".CRT") || n.ends_with(".OpenMP"))
        })
        .collect();
    dirs.sort();
    dirs.reverse();
    dirs
}

fn vs_installations() -> Vec<PathBuf> {
    let Some(pf86) = std::env::var_os("ProgramFiles(x86)") else {
        return Vec::new();
    };
    let vswhere = Path::new(&pf86)
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    let Ok(out) = Command::new(vswhere)
        .args(["-products", "*", "-property", "installationPath", "-sort"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

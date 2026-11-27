//! Microsoft Visual C++ runtime for engine binaries on Windows (ADR 0043).
//!
//! The upstream Windows builds of llama.cpp, whisper.cpp and sd.cpp import
//! `vcruntime140*.dll`, `msvcp140*.dll` and `vcomp140.dll`. A fresh Windows
//! without the "Visual C++ Redistributable" does not have them, and Windows
//! then stops the engine with a modal "VCRUNTIME140_1.dll was not found"
//! dialog. `build.rs` embeds the DLLs; at start we unpack them into the app
//! home and put that folder on `PATH`, which every engine process inherits.
//! An installed runtime in System32 still wins (it comes before `PATH`).

use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/vcrt.rs"));

/// Folder under the app home that holds the runtime DLLs.
pub fn dir(home: &Path) -> PathBuf {
    home.join("runtime").join("vcrt")
}

/// Writes the embedded DLLs into `dir` when missing or of another size.
/// Returns the folder, or `None` when this build embeds none.
pub fn unpack(dir: &Path, files: &[(&str, &[u8])]) -> std::io::Result<Option<PathBuf>> {
    if files.is_empty() {
        return Ok(None);
    }
    std::fs::create_dir_all(dir)?;
    for (name, bytes) in files {
        let target = dir.join(name);
        let current = std::fs::metadata(&target).map(|m| m.len()).ok();
        if current == Some(bytes.len() as u64) {
            continue;
        }
        let tmp = dir.join(format!("{name}.part"));
        std::fs::write(&tmp, bytes)?;
        if let Err(e) = std::fs::rename(&tmp, &target) {
            // A running engine may hold the old copy; it still works.
            let _ = std::fs::remove_file(&tmp);
            if current.is_none() {
                return Err(e);
            }
        }
    }
    Ok(Some(dir.to_path_buf()))
}

/// `PATH` with `dir` in front (unless it is already there).
pub fn path_with(dir: &Path, path: Option<std::ffi::OsString>) -> std::ffi::OsString {
    let mut entries: Vec<PathBuf> = path
        .as_deref()
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    entries.retain(|e| e != dir);
    entries.insert(0, dir.to_path_buf());
    std::env::join_paths(entries).unwrap_or_else(|_| dir.as_os_str().to_owned())
}

/// Result of [`prepare`], logged once logging is up.
pub enum Prepared {
    NotNeeded,
    Ready(PathBuf),
    Failed(String),
}

/// Unpacks the runtime and puts it on `PATH`; also stops Windows from showing
/// modal "missing DLL" dialogs for child processes, so a broken engine fails
/// with an exit code instead of hanging the chat.
///
/// Must run on the main thread before any other thread starts (it changes the
/// process environment).
pub fn prepare(home: &Path) -> Prepared {
    if !cfg!(windows) {
        return Prepared::NotNeeded;
    }
    suppress_error_dialogs();
    match unpack(&dir(home), FILES) {
        Ok(Some(dir)) => {
            let path = path_with(&dir, std::env::var_os("PATH"));
            // SAFETY: called first thing in `run_with`, before logging, Tauri
            // or Tokio start any thread, so nothing reads the environment
            // concurrently.
            unsafe { std::env::set_var("PATH", path) };
            Prepared::Ready(dir)
        }
        Ok(None) => Prepared::Failed("this build has no Visual C++ runtime embedded".into()),
        Err(e) => Prepared::Failed(e.to_string()),
    }
}

/// Logs what [`prepare`] did.
pub fn log(prepared: &Prepared) {
    match prepared {
        Prepared::NotNeeded => {}
        Prepared::Ready(dir) => {
            tracing::debug!(dir = %dir.display(), files = FILES.len(), "Visual C++ runtime ready for engines")
        }
        Prepared::Failed(e) => {
            tracing::warn!(error = %e, "cannot prepare the Visual C++ runtime; engines may not start on a fresh Windows")
        }
    }
}

#[cfg(windows)]
fn suppress_error_dialogs() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetErrorMode,
    };
    // SAFETY: plain Win32 call; the mode is inherited by child processes.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX);
    }
}

#[cfg(not(windows))]
fn suppress_error_dialogs() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_and_skips_same_size_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vcrt");
        let files: &[(&str, &[u8])] = &[("a.dll", b"abc"), ("b.dll", b"de")];
        assert_eq!(unpack(&dir, files).unwrap(), Some(dir.clone()));
        assert_eq!(std::fs::read(dir.join("a.dll")).unwrap(), b"abc");
        // Same size → left alone; other size → rewritten.
        std::fs::write(dir.join("a.dll"), b"xyz").unwrap();
        std::fs::write(dir.join("b.dll"), b"x").unwrap();
        unpack(&dir, files).unwrap();
        assert_eq!(std::fs::read(dir.join("a.dll")).unwrap(), b"xyz");
        assert_eq!(std::fs::read(dir.join("b.dll")).unwrap(), b"de");
        assert!(!dir.join("b.dll.part").exists());
    }

    #[test]
    fn nothing_to_unpack() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(unpack(&tmp.path().join("v"), &[]).unwrap(), None);
        assert!(!tmp.path().join("v").exists());
    }

    #[test]
    fn puts_the_folder_first_once() {
        let dir = PathBuf::from(if cfg!(windows) {
            r"C:\h\vcrt"
        } else {
            "/h/vcrt"
        });
        let other = if cfg!(windows) { r"C:\bin" } else { "/bin" };
        let base = std::env::join_paths([PathBuf::from(other), dir.clone()]).unwrap();
        let joined = path_with(&dir, Some(base));
        let parts: Vec<PathBuf> = std::env::split_paths(&joined).collect();
        assert_eq!(parts, vec![dir.clone(), PathBuf::from(other)]);
        let alone: Vec<PathBuf> = std::env::split_paths(&path_with(&dir, None)).collect();
        assert_eq!(alone, vec![dir]);
    }

    #[cfg(windows)]
    #[test]
    fn this_build_embeds_the_runtime() {
        let names: Vec<&str> = FILES.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"vcruntime140_1.dll"), "{names:?}");
        assert!(names.contains(&"vcomp140.dll"), "{names:?}");
    }
}

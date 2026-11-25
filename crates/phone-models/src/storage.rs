//! Where phone models live: `<root>/<model id>/<file name>`, with a `.part`
//! file next to it while a download is running.

use std::io;
use std::path::{Path, PathBuf};

use crate::catalog::PhoneModel;

pub fn model_dir(root: &Path, model: &PhoneModel) -> PathBuf {
    root.join(&model.id)
}

pub fn model_path(root: &Path, model: &PhoneModel) -> PathBuf {
    model_dir(root, model).join(model.file.file_name())
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Fully downloaded: the file exists and has exactly the catalog size.
/// (The sha256 was verified when the download finished.)
pub fn is_installed(root: &Path, model: &PhoneModel) -> bool {
    std::fs::metadata(model_path(root, model))
        .map(|m| m.is_file() && m.len() == model.file.size)
        .unwrap_or(false)
}

/// Bytes of an interrupted download that a new download would resume from.
pub fn partial_bytes(root: &Path, model: &PhoneModel) -> u64 {
    std::fs::metadata(part_path(&model_path(root, model)))
        .map(|m| m.len())
        .unwrap_or(0)
}

/// Removes the model and any partial download.
pub fn delete(root: &Path, model: &PhoneModel) -> io::Result<()> {
    match std::fs::remove_dir_all(model_dir(root, model)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Free bytes on the volume that holds `dir` (`None` when it cannot be read).
#[cfg(unix)]
pub fn available_space(dir: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let mut probe = dir;
    while !probe.exists() {
        probe = probe.parent()?;
    }
    let c = CString::new(probe.as_os_str().as_bytes()).ok()?;
    // SAFETY: an all-zero `statvfs` is a valid value for the out-parameter.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` is a properly sized out-parameter.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}

#[cfg(not(unix))]
pub fn available_space(_dir: &Path) -> Option<u64> {
    None
}

/// Total RAM of the phone in MB, from `/proc/meminfo` (`None` where there is none).
pub fn total_ram_mb() -> Option<u64> {
    parse_meminfo_total_mb(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// `MemTotal:        7754792 kB` → 7573.
pub fn parse_meminfo_total_mb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

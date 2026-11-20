//! Hybrid CPU detection: performance vs efficiency cores.
//!
//! * macOS: `sysctl hw.nperflevels` / `hw.perflevel{0,1}.physicalcpu`,
//! * Linux: the hybrid PMUs `/sys/devices/cpu_core/cpus` and `/sys/devices/cpu_atom/cpus`
//!   (Intel 12th gen and newer), physical cores counted via `topology/core_cpus_list`,
//! * Windows: `GetLogicalProcessorInformationEx(RelationProcessorCore)`, whose
//!   `EfficiencyClass` is higher for performance cores.
//!
//! Returns `(None, None)` for non-hybrid CPUs or when nothing can be read.

/// `(performance_cores, efficiency_cores)`, physical cores each.
pub(crate) fn hybrid_cores() -> (Option<usize>, Option<usize>) {
    let found = platform();
    match found {
        Some((p, e)) if p > 0 && e > 0 => {
            tracing::debug!(performance = p, efficiency = e, "hybrid CPU detected");
            (Some(p), Some(e))
        }
        _ => (None, None),
    }
}

#[cfg(target_os = "macos")]
fn platform() -> Option<(usize, usize)> {
    let read = |key: &str| -> Option<usize> {
        let out = std::process::Command::new("sysctl")
            .args(["-n", key])
            .output()
            .map_err(|e| tracing::debug!(error = %e, key, "sysctl failed"))
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    };
    if read("hw.nperflevels")? < 2 {
        return None;
    }
    Some((
        read("hw.perflevel0.physicalcpu")?,
        read("hw.perflevel1.physicalcpu")?,
    ))
}

#[cfg(target_os = "linux")]
fn platform() -> Option<(usize, usize)> {
    let p = linux_physical("/sys/devices/cpu_core/cpus")?;
    let e = linux_physical("/sys/devices/cpu_atom/cpus").unwrap_or(0);
    Some((p, e))
}

/// Physical cores among the logical CPUs listed in `list_file` (e.g. `0-11`).
#[cfg(target_os = "linux")]
fn linux_physical(list_file: &str) -> Option<usize> {
    let text = std::fs::read_to_string(list_file).ok()?;
    let cpus = parse_cpu_list(text.trim());
    if cpus.is_empty() {
        return None;
    }
    let mut cores = std::collections::BTreeSet::new();
    for cpu in cpus {
        let siblings = std::fs::read_to_string(format!(
            "/sys/devices/system/cpu/cpu{cpu}/topology/core_cpus_list"
        ))
        .or_else(|_| {
            std::fs::read_to_string(format!(
                "/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list"
            ))
        })
        .unwrap_or_else(|_| cpu.to_string());
        cores.insert(siblings.trim().to_owned());
    }
    Some(cores.len())
}

/// Parses Linux CPU lists such as `0-3,8,10-11`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_cpu_list(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    out.extend(a..=b.min(a + 4096));
                }
            }
            None => out.extend(part.parse::<usize>().ok()),
        }
    }
    out
}

#[cfg(windows)]
fn platform() -> Option<(usize, usize)> {
    use windows_sys::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore,
    };
    let mut len = 0u32;
    // SAFETY: a null buffer with length 0 only asks for the required size.
    unsafe {
        GetLogicalProcessorInformationEx(RelationProcessorCore, std::ptr::null_mut(), &mut len)
    };
    if len == 0 {
        tracing::debug!("GetLogicalProcessorInformationEx returned no size");
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: `buf` is `len` bytes long, as requested by the first call.
    let ok = unsafe {
        GetLogicalProcessorInformationEx(RelationProcessorCore, buf.as_mut_ptr().cast(), &mut len)
    };
    if ok == 0 {
        tracing::debug!("GetLogicalProcessorInformationEx failed");
        return None;
    }
    let classes = parse_core_records(&buf[..len as usize]);
    split_by_class(&classes)
}

/// Efficiency classes of the `SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX` records
/// in `buf`: `Relationship: i32`, `Size: u32`, then `PROCESSOR_RELATIONSHIP`
/// (`Flags: u8`, `EfficiencyClass: u8`, …). Parsed byte-wise to avoid
/// unaligned reads.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse_core_records(buf: &[u8]) -> Vec<u8> {
    let mut classes = Vec::new();
    let mut offset = 0usize;
    while offset + 10 <= buf.len() {
        let relationship = i32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap_or([0; 4]));
        let size = u32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap_or([0; 4]));
        if size < 10 {
            break;
        }
        if relationship == 0 {
            classes.push(buf[offset + 9]);
        }
        offset += size as usize;
    }
    classes
}

/// Highest efficiency class = performance cores; all equal = not hybrid.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn split_by_class(classes: &[u8]) -> Option<(usize, usize)> {
    let max = *classes.iter().max()?;
    let p = classes.iter().filter(|&&c| c == max).count();
    Some((p, classes.len() - p))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn platform() -> Option<(usize, usize)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpu_lists() {
        assert_eq!(parse_cpu_list("0-3,8,10-11"), vec![0, 1, 2, 3, 8, 10, 11]);
        assert_eq!(parse_cpu_list(""), Vec::<usize>::new());
        assert_eq!(parse_cpu_list("x,2"), vec![2]);
    }

    #[test]
    fn splits_windows_efficiency_classes() {
        // i5-1235U: 2 P-cores (class 1) + 8 E-cores (class 0)
        let mut classes = vec![1, 1];
        classes.extend([0; 8]);
        assert_eq!(split_by_class(&classes), Some((2, 8)));
        // non-hybrid: every core has the same class
        assert_eq!(split_by_class(&[0; 6]), Some((6, 0)));
        assert_eq!(split_by_class(&[]), None);
    }

    #[test]
    fn parses_processor_records() {
        // two records of 48 bytes: relationship 0, efficiency classes 1 and 0
        let mut buf = Vec::new();
        for class in [1u8, 0] {
            let mut rec = vec![0u8; 48];
            rec[4..8].copy_from_slice(&48u32.to_le_bytes());
            rec[9] = class;
            buf.extend(rec);
        }
        assert_eq!(parse_core_records(&buf), vec![1, 0]);
        assert!(parse_core_records(&[0; 4]).is_empty());
    }

    #[test]
    fn probe_does_not_panic() {
        let (p, e) = hybrid_cores();
        assert_eq!(p.is_some(), e.is_some());
    }
}

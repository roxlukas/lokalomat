//! GPU detection: NVML → `nvidia-smi` → OS list (`sysinfo`), then compute
//! details per platform (macOS `ioreg`, Linux KFD/DRM sysfs) and finally the
//! hardware catalogue `catalog/hardware.json` (`gpu_db`) for whatever is still missing.

use std::process::Command;

use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::Clock;
use sysinfo::Gpus;

use crate::{GpuInfo, GpuVendor, gpu_db};

pub(crate) fn detect(ram_mb: u64) -> Vec<GpuInfo> {
    let mut gpus = nvml().or_else(nvidia_smi).unwrap_or_default();
    let have_nvidia = !gpus.is_empty();

    for gpu in os_gpus(ram_mb) {
        if have_nvidia && gpu.vendor == GpuVendor::Nvidia {
            continue; // NVML/nvidia-smi already reported it with exact numbers
        }
        gpus.push(gpu);
    }
    if gpus.is_empty() {
        tracing::info!("no GPU detected, CPU only");
    }
    platform_details(&mut gpus);
    for gpu in &mut gpus {
        gpu_db::enrich(gpu);
    }
    gpus
}

/// Bandwidth from bus width (bits) and memory clock (MHz, as NVML reports it:
/// half the effective data rate for GDDR and HBM).
pub(crate) fn bandwidth_gbs(bus_bits: u32, mem_clock_mhz: u32) -> f32 {
    bus_bits as f32 / 8.0 * mem_clock_mhz as f32 * 2.0 / 1000.0
}

/// CUDA cores per SM for a compute capability.
pub(crate) fn cores_per_sm(major: i32, minor: i32) -> u32 {
    match (major, minor) {
        (7, _) | (8, 0) | (6, 0) => 64,
        _ => 128,
    }
}

fn nvml() -> Option<Vec<GpuInfo>> {
    let nvml = match Nvml::init() {
        Ok(n) => n,
        Err(e) => {
            tracing::debug!(error = %e, "NVML not available");
            return None;
        }
    };
    let driver = nvml.sys_driver_version().ok();
    let count = nvml.device_count().ok()?;
    let mut out = Vec::new();
    for i in 0..count {
        let device = match nvml.device_by_index(i) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(index = i, error = %e, "NVML cannot open device");
                continue;
            }
        };
        let name = device.name().unwrap_or_else(|_| "NVIDIA GPU".into());
        let vram_mb = device
            .memory_info()
            .map(|m| m.total / (1024 * 1024))
            .unwrap_or(0);
        let cores = device.num_cores().ok().filter(|&c| c > 0);
        let sm_clock = device.max_clock_info(Clock::SM).ok().filter(|&c| c > 0);
        let bandwidth = match (
            device.memory_bus_width().ok(),
            device.max_clock_info(Clock::Memory).ok(),
        ) {
            (Some(bus), Some(clock)) if bus > 0 && clock > 0 => Some(bandwidth_gbs(bus, clock)),
            _ => None,
        };
        // NVML packs the PCI IDs as (device << 16) | vendor.
        let pci_id = device.pci_info().ok().map(|p| {
            format!(
                "{:04x}:{:04x}",
                p.pci_device_id & 0xffff,
                p.pci_device_id >> 16
            )
        });
        let per_sm = device
            .cuda_compute_capability()
            .map(|cc| cores_per_sm(cc.major, cc.minor))
            .unwrap_or(128);
        tracing::debug!(
            index = i,
            ?cores,
            ?sm_clock,
            ?bandwidth,
            per_sm,
            "NVML compute details"
        );
        out.push(GpuInfo {
            vendor: GpuVendor::Nvidia,
            name,
            vram_mb,
            driver: driver.clone(),
            unified: false,
            source: "nvml".into(),
            compute_units: cores.map(|c| c / per_sm),
            shader_cores: cores,
            clock_mhz: sm_clock,
            memory_bandwidth_gbs: bandwidth,
            integrated: Some(false),
            pci_id,
            details_source: (cores.is_some() || bandwidth.is_some()).then(|| "nvml".into()),
            ..Default::default()
        });
    }
    (!out.is_empty()).then_some(out)
}

fn nvidia_smi() -> Option<Vec<GpuInfo>> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,memory.total,driver_version",
        "--format=csv,noheader,nounits",
    ]);
    hide_console(&mut cmd);
    let output = match cmd.output() {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            tracing::debug!(status = ?o.status, "nvidia-smi failed");
            return None;
        }
        Err(e) => {
            tracing::debug!(error = %e, "nvidia-smi not found");
            return None;
        }
    };
    let gpus = parse_nvidia_smi(&String::from_utf8_lossy(&output.stdout));
    (!gpus.is_empty()).then_some(gpus)
}

pub(crate) fn parse_nvidia_smi(text: &str) -> Vec<GpuInfo> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split(',').map(str::trim);
            let name = parts.next().filter(|s| !s.is_empty())?;
            let vram_mb = parts.next()?.parse::<u64>().ok()?;
            let driver = parts.next().map(str::to_owned);
            Some(GpuInfo {
                vendor: GpuVendor::Nvidia,
                name: name.to_owned(),
                vram_mb,
                driver,
                unified: false,
                source: "nvidia-smi".into(),
                integrated: Some(false),
                ..Default::default()
            })
        })
        .collect()
}

fn os_gpus(ram_mb: u64) -> Vec<GpuInfo> {
    let gpus = match Gpus::new_with_refreshed_list() {
        Ok(g) => g,
        Err(e) => {
            tracing::debug!(error = %e, "OS GPU list unavailable");
            return Vec::new();
        }
    };
    gpus.iter()
        .map(|g| {
            let name = g.model().unwrap_or("GPU").to_owned();
            let vendor = vendor_from(&format!("{:?} {name}", g.vendor()));
            let unified = vendor == GpuVendor::Apple;
            let vram_mb = if unified {
                ram_mb
            } else {
                g.total_memory().unwrap_or(0) / (1024 * 1024)
            };
            GpuInfo {
                vendor,
                name,
                vram_mb,
                driver: None,
                unified,
                source: "os".into(),
                ..Default::default()
            }
        })
        .collect()
}

// --- macOS: Apple GPU core count from the IORegistry ---------------------------

#[cfg(target_os = "macos")]
fn platform_details(gpus: &mut [GpuInfo]) {
    let Some(apple) = gpus.iter_mut().find(|g| g.vendor == GpuVendor::Apple) else {
        return;
    };
    let mut cmd = Command::new("ioreg");
    cmd.args(["-rc", "AGXAccelerator", "-d", "1"]);
    match cmd.output() {
        Ok(o) if o.status.success() => {
            if let Some(cores) = parse_ioreg_cores(&String::from_utf8_lossy(&o.stdout)) {
                apple.compute_units = Some(cores);
                apple.details_source = Some("ioreg".into());
                tracing::debug!(cores, "Apple GPU cores from ioreg");
            }
        }
        Ok(o) => tracing::debug!(status = ?o.status, "ioreg failed"),
        Err(e) => tracing::debug!(error = %e, "ioreg not available"),
    }
}

/// `"gpu-core-count" = 10` in `ioreg -rc AGXAccelerator` output.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn parse_ioreg_cores(text: &str) -> Option<u32> {
    text.lines()
        .find(|l| l.contains("\"gpu-core-count\""))
        .and_then(|l| l.rsplit('=').next())
        .and_then(|v| v.trim().parse().ok())
}

// --- Linux: AMD compute units from KFD, Intel/AMD clocks from DRM sysfs -------

#[cfg(target_os = "linux")]
fn platform_details(gpus: &mut [GpuInfo]) {
    // PCI IDs from DRM, assigned when a vendor has exactly one card.
    let cards = drm_cards();
    for gpu in gpus.iter_mut().filter(|g| g.pci_id.is_none()) {
        let vendor = match gpu.vendor {
            GpuVendor::Nvidia => "10de",
            GpuVendor::Amd => "1002",
            GpuVendor::Intel => "8086",
            _ => continue,
        };
        let same: Vec<_> = cards.iter().filter(|c| c.starts_with(vendor)).collect();
        if let [only] = same.as_slice() {
            gpu.pci_id = Some((*only).clone());
        }
    }
    let nodes = kfd_nodes();
    for node in &nodes {
        tracing::debug!(?node, "KFD GPU node");
    }
    for gpu in gpus.iter_mut().filter(|g| g.vendor == GpuVendor::Amd) {
        // Match by size order is fragile; KFD has no names, so use the only
        // node or the one whose local memory matches the reported VRAM.
        let node = if nodes.len() == 1 {
            nodes.first()
        } else {
            nodes
                .iter()
                .min_by_key(|n| (n.local_mem_mb as i64 - gpu.vram_mb as i64).abs())
        };
        if let Some(n) = node {
            gpu.compute_units = Some(n.compute_units);
            gpu.shader_cores = Some(n.compute_units * 64);
            if n.max_clock_mhz > 0 {
                gpu.clock_mhz = Some(n.max_clock_mhz);
            }
            gpu.details_source = Some("kfd".into());
        }
    }
    for gpu in gpus
        .iter_mut()
        .filter(|g| g.vendor == GpuVendor::Intel && g.clock_mhz.is_none())
    {
        if let Some(mhz) = intel_max_freq() {
            gpu.clock_mhz = Some(mhz);
            gpu.details_source = Some("sysfs".into());
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KfdNode {
    pub compute_units: u32,
    pub max_clock_mhz: u32,
    pub local_mem_mb: u64,
}

#[cfg(target_os = "linux")]
fn kfd_nodes() -> Vec<KfdNode> {
    let Ok(dir) = std::fs::read_dir("/sys/class/kfd/kfd/topology/nodes") else {
        tracing::debug!("KFD topology not available");
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("properties")).ok())
        .filter_map(|text| parse_kfd_properties(&text))
        .collect()
}

/// Parses a KFD `properties` file; CPU nodes (`simd_count 0`) are skipped.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_kfd_properties(text: &str) -> Option<KfdNode> {
    let get = |key: &str| -> u64 {
        text.lines()
            .find_map(|l| {
                let (k, v) = l.split_once(' ')?;
                (k == key).then(|| v.trim().parse().ok())?
            })
            .unwrap_or(0)
    };
    let simd = get("simd_count");
    let per_cu = get("simd_per_cu").max(1);
    if simd == 0 {
        return None;
    }
    Some(KfdNode {
        compute_units: (simd / per_cu) as u32,
        max_clock_mhz: get("max_engine_clk_fcompute") as u32,
        local_mem_mb: get("local_mem_size") / (1024 * 1024),
    })
}

/// `vendor:device` of every DRM card (`/sys/class/drm/cardN/device/{vendor,device}`).
#[cfg(target_os = "linux")]
fn drm_cards() -> Vec<String> {
    let Ok(dir) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let read = |f: &str| std::fs::read_to_string(entry.path().join("device").join(f)).ok();
        if let (Some(v), Some(d)) = (read("vendor"), read("device")) {
            out.push(pci_id_from_sysfs(&v, &d));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `0x8086` + `0x5916` → `8086:5916`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn pci_id_from_sysfs(vendor: &str, device: &str) -> String {
    let clean = |s: &str| s.trim().trim_start_matches("0x").to_ascii_lowercase();
    format!("{}:{}", clean(vendor), clean(device))
}

#[cfg(target_os = "linux")]
fn intel_max_freq() -> Option<u32> {
    let dir = std::fs::read_dir("/sys/class/drm").ok()?;
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let path = entry.path();
        let vendor = std::fs::read_to_string(path.join("device/vendor")).unwrap_or_default();
        if vendor.trim() != "0x8086" {
            continue;
        }
        // i915: gt_max_freq_mhz; xe: device/tile0/gt0/freq0/max_freq
        for file in ["gt_max_freq_mhz", "device/tile0/gt0/freq0/max_freq"] {
            if let Some(mhz) = std::fs::read_to_string(path.join(file))
                .ok()
                .and_then(|t| t.trim().parse().ok())
            {
                return Some(mhz);
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_details(_gpus: &mut [GpuInfo]) {
    // Windows: NVML covers NVIDIA; AMD/Intel details come from the lookup table
    // (DXGI, already used by sysinfo, reports memory only).
}

pub(crate) fn vendor_from(text: &str) -> GpuVendor {
    let t = text.to_lowercase();
    if t.contains("nvidia") || t.contains("geforce") || t.contains("quadro") {
        GpuVendor::Nvidia
    } else if t.contains("amd") || t.contains("radeon") || t.contains("advanced micro") {
        GpuVendor::Amd
    } else if t.contains("intel") {
        GpuVendor::Intel
    } else if t.contains("apple") {
        GpuVendor::Apple
    } else {
        GpuVendor::Other
    }
}

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_cmd: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvidia_smi_csv() {
        let gpus = parse_nvidia_smi("NVIDIA GeForce RTX 3090, 24576, 581.57\n\n");
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].vram_mb, 24576);
        assert_eq!(gpus[0].driver.as_deref(), Some("581.57"));
    }

    #[test]
    fn nvidia_bandwidth_and_cores() {
        // RTX 3090: 384-bit bus, NVML memory clock 9751 MHz → ~936 GB/s
        assert!((bandwidth_gbs(384, 9751) - 936.1).abs() < 0.5);
        assert_eq!(cores_per_sm(8, 6), 128);
        assert_eq!(cores_per_sm(7, 5), 64);
    }

    #[test]
    fn parses_ioreg_and_kfd() {
        let ioreg = "+-o AGXAcceleratorG14X  <class AGXAcceleratorG14X>\n    {\n      \"gpu-core-count\" = 19\n    }";
        assert_eq!(parse_ioreg_cores(ioreg), Some(19));
        assert_eq!(parse_ioreg_cores("nothing"), None);

        let kfd = "cpu_cores_count 0\nsimd_count 240\nsimd_per_cu 4\nmax_engine_clk_fcompute 2430\nlocal_mem_size 17163091968\n";
        let node = parse_kfd_properties(kfd).unwrap();
        assert_eq!(node.compute_units, 60);
        assert_eq!(node.max_clock_mhz, 2430);
        assert_eq!(node.local_mem_mb, 16_368);
        assert!(parse_kfd_properties("cpu_cores_count 8\nsimd_count 0\n").is_none());
    }

    #[test]
    fn pci_ids_from_sysfs() {
        assert_eq!(pci_id_from_sysfs("0x8086\n", "0x5916\n"), "8086:5916");
    }

    #[test]
    fn detect_does_not_panic() {
        for g in detect(16_384) {
            assert!(!g.name.is_empty());
        }
    }

    #[test]
    fn recognises_vendors() {
        assert_eq!(vendor_from("AMD Radeon RX 7900 XTX"), GpuVendor::Amd);
        assert_eq!(vendor_from("Intel(R) Arc(TM) A770"), GpuVendor::Intel);
        assert_eq!(vendor_from("Apple M3 Max"), GpuVendor::Apple);
        assert_eq!(vendor_from("Nvidia RTX 3090"), GpuVendor::Nvidia);
    }
}

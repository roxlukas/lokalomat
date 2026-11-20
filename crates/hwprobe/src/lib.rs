//! Hardware probe (PRD §5): GPU/VRAM, RAM, CPU features and disks, plus the
//! model fit scoring and speed estimate used by the catalog and the first-run wizard.
//!
//! Detection order for GPUs: NVML (NVIDIA, exact VRAM and driver), then
//! `nvidia-smi`, then the OS GPU list from `sysinfo` (AMD/Intel/Apple).
//! Compute details (cores, clocks, memory bandwidth) come from NVML, `ioreg`
//! (macOS), KFD/DRM sysfs (Linux) and the GPU catalogue `catalog/hardware.json`.
//! See docs/adr/0006-hardware-probe.md and docs/adr/0016-speed-tiers.md.

mod cpu;
mod fit;
mod gpu;
mod gpu_db;
mod speed;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};

pub use fit::{
    BudgetKind, ESTIMATE_CTX_TOKENS, Fit, FitReport, MemoryBudget, budget, fit, kv_per_1k_mb,
    llm_compact_estimate_mb, llm_fit, music_fit,
};
pub use gpu_db::{GpuSpec, HardwareCatalog, catalog as hardware_catalog};
pub use speed::{
    Speed, SpeedBasis, SpeedDevice, SpeedReason, SpeedReport, SpeedUnit, WorkKind, Workload,
    estimate, params_from_size, speed_basis,
};

/// Bumped when the profile gains fields that older saved profiles lack, so
/// the app re-probes instead of using a stale profile from the database.
pub const PROBE_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize)]
pub struct BasicSystemInfo {
    pub os: &'static str,
    pub os_family: &'static str,
    pub arch: &'static str,
    pub cpu_logical_cores: usize,
}

pub fn basic_info() -> BasicSystemInfo {
    BasicSystemInfo {
        os: std::env::consts::OS,
        os_family: std::env::consts::FAMILY,
        arch: std::env::consts::ARCH,
        cpu_logical_cores: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    #[default]
    Other,
}

/// Compute backends an engine build can target on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Cuda,
    Metal,
    Vulkan,
    Cpu,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub name: String,
    /// Dedicated VRAM, or the whole RAM for unified-memory GPUs.
    pub vram_mb: u64,
    pub driver: Option<String>,
    /// Apple Silicon style unified memory shared with the CPU.
    pub unified: bool,
    /// Where the data came from: `nvml`, `nvidia-smi` or `os`.
    pub source: String,
    /// Vendor compute units: NVIDIA SMs, AMD CUs, Intel EUs (Xe-cores on Arc), Apple GPU cores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compute_units: Option<u32>,
    /// Shader ALUs (CUDA cores, stream processors, …), comparable across vendors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader_cores: Option<u32>,
    /// Maximum (boost) graphics clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_mhz: Option<u32>,
    /// Peak bandwidth of the GPU's memory (unified GPUs: the system RAM;
    /// integrated graphics: none, they share the CPU's RAM).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bandwidth_gbs: Option<f32>,
    /// Peak FP32 throughput: shader cores × clock × 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fp32_tflops: Option<f32>,
    /// Integrated graphics according to the driver or the lookup table
    /// (`None` = unknown, then the name heuristics decide).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrated: Option<bool>,
    /// PCI `vendor:device` in lower-case hex (e.g. `10de:2204`) when the driver reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pci_id: Option<String>,
    /// Where the compute details came from: `nvml`, `ioreg`, `kfd`, `sysfs`, `table`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CpuInfo {
    pub brand: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    /// `avx2`, `avx512f` (x86-64) or `neon` (ARM).
    pub features: Vec<String>,
    /// Performance cores on hybrid CPUs (Apple P-cores, Intel P-cores); `None` when not hybrid or unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance_cores: Option<usize>,
    /// Efficiency cores on hybrid CPUs (Apple E-cores, Intel E-cores).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efficiency_cores: Option<usize>,
}

impl CpuInfo {
    /// Cores llama.cpp/whisper.cpp can use at full speed: efficiency cores
    /// count as half a core; without the physical count, half the threads.
    pub fn effective_cores(&self) -> f32 {
        match (self.performance_cores, self.efficiency_cores) {
            (Some(p), e) if p > 0 => p as f32 + e.unwrap_or(0) as f32 * 0.5,
            _ => self
                .physical_cores
                .map(|c| c as f32)
                .unwrap_or((self.logical_cores as f32 / 2.0).max(1.0)),
        }
    }

    pub fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: PathBuf,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub removable: bool,
    /// `ssd`, `hdd` or `unknown`.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub os: String,
    pub os_version: Option<String>,
    pub arch: String,
    pub cpu: CpuInfo,
    pub ram_mb: u64,
    pub gpus: Vec<GpuInfo>,
    pub disks: Vec<DiskInfo>,
    pub backends: Vec<Backend>,
    /// Unix seconds.
    pub probed_at: u64,
    /// Peak system RAM bandwidth when known (Apple Silicon chip table).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bandwidth_gbs: Option<f32>,
    /// [`PROBE_VERSION`] of the build that probed this profile (0 = before speed tiers).
    #[serde(default)]
    pub probe_version: u32,
}

impl HardwareProfile {
    /// The GPU used for budgeting: the one with the most usable memory.
    pub fn primary_gpu(&self) -> Option<&GpuInfo> {
        self.gpus
            .iter()
            .filter(|g| g.unified || (!g.is_integrated() && g.vram_mb >= fit::MIN_USEFUL_VRAM_MB))
            .max_by_key(|g| g.vram_mb)
    }
}

impl GpuInfo {
    /// Integrated graphics (Intel UHD/HD/Iris, AMD Radeon in APUs) have no
    /// memory of their own: the OS reports a share of system RAM, which must
    /// not be counted as a separate budget (PRD §15 item 14). Apple Silicon is
    /// handled as `unified` instead.
    pub fn is_integrated(&self) -> bool {
        if self.unified {
            return false;
        }
        if let Some(integrated) = self.integrated {
            return integrated;
        }
        let name = self.name.to_ascii_lowercase();
        match self.vendor {
            // Intel Arc cards are discrete; everything else from Intel is integrated.
            GpuVendor::Intel => !name.contains("arc") || name.contains("arc(tm) graphics"),
            // APUs report "Radeon Graphics", "Radeon Vega …" or "Radeon 780M"-style names.
            GpuVendor::Amd => {
                !name.contains(" rx ")
                    && !name.contains("radeon pro")
                    && !name.contains("instinct")
                    && (name.ends_with("graphics")
                        || name.contains("vega")
                        || name.contains("780m")
                        || name.contains("760m")
                        || name.contains("890m")
                        || name.contains("880m"))
            }
            _ => false,
        }
    }
}

/// Full probe. Takes up to a few hundred milliseconds (NVML init, disk
/// enumeration) – call it off the UI thread.
pub fn probe() -> HardwareProfile {
    let started = std::time::Instant::now();
    let (ram_mb, brand) = match System::new() {
        Ok(mut sys) => {
            sys.refresh_memory();
            sys.refresh_cpu_all();
            let brand = sys
                .cpus()
                .first()
                .map(|c| c.brand().trim().to_owned())
                .unwrap_or_default();
            (sys.total_memory() / (1024 * 1024), brand)
        }
        Err(e) => {
            tracing::warn!(error = %e, "cannot read memory/CPU information");
            (0, String::new())
        }
    };
    let (performance_cores, efficiency_cores) = cpu::hybrid_cores();
    let cpu = CpuInfo {
        brand,
        physical_cores: System::physical_core_count().ok(),
        logical_cores: basic_info().cpu_logical_cores,
        features: cpu_features(),
        performance_cores,
        efficiency_cores,
    };
    let gpus = gpu::detect(ram_mb);
    let backends = backends_for(&gpus);
    let disks = disks();
    let memory_bandwidth_gbs = gpus
        .iter()
        .find(|g| g.unified)
        .and_then(|g| g.memory_bandwidth_gbs);

    let profile = HardwareProfile {
        os: std::env::consts::OS.to_owned(),
        os_version: System::long_os_version().ok(),
        arch: std::env::consts::ARCH.to_owned(),
        cpu,
        ram_mb,
        gpus,
        disks,
        backends,
        probed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        memory_bandwidth_gbs,
        probe_version: PROBE_VERSION,
    };
    tracing::info!(
        ram_mb = profile.ram_mb,
        gpus = ?profile.gpus.iter().map(|g| format!(
            "{} {} MB ({}), units {:?}, {:?} TFLOPS, {:?} GB/s ({})",
            g.name, g.vram_mb, g.source, g.compute_units, g.fp32_tflops,
            g.memory_bandwidth_gbs, g.details_source.as_deref().unwrap_or("-")
        )).collect::<Vec<_>>(),
        cpu_cores = profile.cpu.physical_cores,
        cpu_p_cores = profile.cpu.performance_cores,
        cpu_e_cores = profile.cpu.efficiency_cores,
        cpu_features = ?profile.cpu.features,
        backends = ?profile.backends,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "hardware probed"
    );
    tracing::debug!(?profile, "hardware profile");
    profile
}

fn cpu_features() -> Vec<String> {
    #[allow(unused_mut)]
    let mut features: Vec<String> = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            features.push("avx2".into());
        }
        if std::arch::is_x86_feature_detected!("avx512f") {
            features.push("avx512f".into());
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            features.push("neon".into());
        }
    }
    features
}

fn backends_for(gpus: &[GpuInfo]) -> Vec<Backend> {
    let mut out = Vec::new();
    if gpus.iter().any(|g| g.vendor == GpuVendor::Nvidia) {
        out.push(Backend::Cuda);
    }
    if gpus.iter().any(|g| g.vendor == GpuVendor::Apple) {
        out.push(Backend::Metal);
    }
    if cfg!(not(target_os = "macos"))
        && gpus.iter().any(|g| {
            matches!(
                g.vendor,
                GpuVendor::Nvidia | GpuVendor::Amd | GpuVendor::Intel
            )
        })
    {
        out.push(Backend::Vulkan);
    }
    out.push(Backend::Cpu);
    out
}

fn disks() -> Vec<DiskInfo> {
    let disks = match Disks::new_with_refreshed_list() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "cannot list disks");
            return Vec::new();
        }
    };
    let mut out: Vec<DiskInfo> = disks
        .iter()
        .filter(|d| d.total_space() > 0)
        .map(|d| DiskInfo {
            name: d.name().to_string_lossy().into_owned(),
            mount_point: d.mount_point().to_path_buf(),
            total_bytes: d.total_space(),
            available_bytes: d.available_space(),
            removable: d.is_removable(),
            kind: match d.kind() {
                sysinfo::DiskKind::SSD => "ssd".into(),
                sysinfo::DiskKind::HDD => "hdd".into(),
                _ => "unknown".into(),
            },
        })
        .collect();
    out.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    out.dedup_by(|a, b| a.mount_point == b.mount_point);
    out
}

/// Free space on the volume that holds `path` (the path does not need to exist yet).
pub fn available_space(path: &Path) -> Option<u64> {
    let target = existing_ancestor(path)?;
    let target = std::fs::canonicalize(&target).unwrap_or(target);
    let target = strip_verbatim(&target);
    let disks = Disks::new_with_refreshed_list().ok()?;
    disks
        .iter()
        .filter(|d| target.starts_with(strip_verbatim(d.mount_point())))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut p = Some(path);
    while let Some(dir) = p {
        if dir.exists() {
            return Some(dir.to_path_buf());
        }
        p = dir.parent();
    }
    None
}

/// `canonicalize` on Windows returns `\\?\C:\…`; mount points are `C:\`.
fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_at_least_one_core() {
        let info = basic_info();
        assert!(info.cpu_logical_cores >= 1);
        assert!(!info.arch.is_empty());
    }

    #[test]
    fn probe_finds_ram_and_a_disk() {
        let p = probe();
        assert!(p.ram_mb > 0);
        assert!(!p.disks.is_empty());
        assert!(p.backends.contains(&Backend::Cpu));
    }

    #[test]
    fn available_space_for_missing_subdir() {
        let dir = std::env::temp_dir()
            .join("lokalsi-not-created-yet")
            .join("models");
        assert!(available_space(&dir).unwrap_or(0) > 0);
    }

    #[test]
    fn strips_windows_verbatim_prefix() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\Users")),
            PathBuf::from(r"C:\Users")
        );
        assert_eq!(strip_verbatim(Path::new("/tmp")), PathBuf::from("/tmp"));
    }
}

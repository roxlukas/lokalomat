//! Model fit scoring (PRD §5 "Ocena dopasowania modelu").
//!
//! * **Fits** – at least 15% headroom in the main budget,
//! * **Tight** – 0–15% headroom (warning, possible slowdown),
//! * **Offload** – does not fit in VRAM, but fits with RAM offload (slower),
//! * **WontRun** – not enough memory in total.

use serde::{Deserialize, Serialize};

use crate::HardwareProfile;
use crate::speed::{SpeedBasis, speed_basis};

/// GPUs with less dedicated memory (typical integrated graphics) are ignored.
pub const MIN_USEFUL_VRAM_MB: u64 = 2048;
/// Kept free on a discrete GPU for the desktop and driver.
pub const GPU_RESERVE_MB: u64 = 512;
/// Kept free in system RAM for the OS and the application itself.
pub const RAM_RESERVE_MB: u64 = 4096;
/// Minimum headroom for the "fits" label.
pub const FITS_HEADROOM: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    /// Dedicated GPU memory, RAM available for offload.
    Gpu,
    /// Apple Silicon unified memory (RAM × factor).
    Unified,
    /// No usable GPU: everything runs from RAM.
    Cpu,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryBudget {
    pub kind: BudgetKind,
    /// Memory a model should fit in to run at full speed.
    pub main_mb: u64,
    /// Extra system RAM usable for offloading layers (0 when not applicable).
    pub offload_mb: u64,
    pub device: Option<String>,
    /// Effective throughput used for the speed estimate (`None` in tests
    /// and for budgets built without a hardware profile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<SpeedBasis>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Fits,
    Tight,
    Offload,
    WontRun,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitReport {
    pub fit: Fit,
    pub estimate_mb: u64,
    pub budget_mb: u64,
    pub offload_mb: u64,
    /// `(budget - estimate) / budget`; negative when the model does not fit.
    pub headroom: f32,
}

fn usable_ram_mb(ram_mb: u64) -> u64 {
    ram_mb.saturating_sub(RAM_RESERVE_MB).max(ram_mb / 2)
}

/// Memory budget for `profile`. `unified_percent` is the share of RAM a
/// unified-memory GPU may use (PRD default 70%).
pub fn budget(profile: &HardwareProfile, unified_percent: u8) -> MemoryBudget {
    let usable_ram = usable_ram_mb(profile.ram_mb);
    let mut b = match profile.primary_gpu() {
        Some(gpu) if gpu.unified => MemoryBudget {
            kind: BudgetKind::Unified,
            main_mb: profile.ram_mb * u64::from(unified_percent.clamp(10, 95)) / 100,
            offload_mb: 0,
            device: Some(gpu.name.clone()),
            speed: None,
        },
        Some(gpu) => MemoryBudget {
            kind: BudgetKind::Gpu,
            main_mb: gpu.vram_mb.saturating_sub(GPU_RESERVE_MB),
            offload_mb: usable_ram,
            device: Some(gpu.name.clone()),
            speed: None,
        },
        None => MemoryBudget {
            kind: BudgetKind::Cpu,
            main_mb: usable_ram,
            offload_mb: 0,
            device: None,
            speed: None,
        },
    };
    b.speed = Some(speed_basis(profile, b.kind));
    b
}

/// KV cache (f16) per 1 000 tokens of context: ~2% of the model's memory
/// estimate (ADR 0014). Measured: Bielik 11B Q6_K 207 MB, heuristic 212 MB;
/// models with sliding-window attention (Gemma 4) need far less.
pub fn kv_per_1k_mb(model_mb: u64) -> u64 {
    (model_mb / 50).max(16)
}

/// Context the catalog's LLM memory estimate already covers (f16 KV cache).
pub const ESTIMATE_CTX_TOKENS: u64 = 8192;

/// LLM memory with the compact q8_0 KV cache (ADR 0021): the estimate covers
/// an 8k-token f16 cache, q8_0 needs half of it.
pub fn llm_compact_estimate_mb(estimate_mb: u64) -> u64 {
    let kv = kv_per_1k_mb(estimate_mb) * ESTIMATE_CTX_TOKENS / 1024;
    estimate_mb.saturating_sub(kv / 2)
}

/// Fit of an LLM: when it does not fit comfortably with the default cache,
/// the engine switches to the compact KV cache (ADR 0021), so the better of
/// the two reports is shown.
pub fn llm_fit(estimate_mb: u64, budget: &MemoryBudget) -> FitReport {
    let full = fit(estimate_mb, budget);
    if full.fit == Fit::Fits {
        return full;
    }
    let compact = fit(llm_compact_estimate_mb(estimate_mb), budget);
    if rank(compact.fit) < rank(full.fit) {
        compact
    } else {
        full
    }
}

/// Fit of a music model (ADR 0022). ACE-Step loads every model through RAM,
/// and when VRAM is short the models stay there (CPU offload, or the Windows
/// driver's shared memory), so running past VRAM needs the whole estimate in
/// usable RAM. Measured on an RTX 2070 with 16 GB of RAM: the LM 0.6B variant
/// (≈ 11 GB) ran, the LM 1.7B one (≈ 14 GB) ran out of memory while loading.
pub fn music_fit(estimate_mb: u64, budget: &MemoryBudget) -> FitReport {
    let mut report = fit(estimate_mb, budget);
    if report.fit == Fit::Offload && estimate_mb > budget.offload_mb {
        report.fit = Fit::WontRun;
    }
    report
}

fn rank(f: Fit) -> u8 {
    match f {
        Fit::Fits => 0,
        Fit::Tight => 1,
        Fit::Offload => 2,
        Fit::WontRun => 3,
    }
}

pub fn fit(estimate_mb: u64, budget: &MemoryBudget) -> FitReport {
    let main = budget.main_mb;
    let headroom = if main == 0 {
        -1.0
    } else {
        (main as f64 - estimate_mb as f64) as f32 / main as f32
    };
    let fit = if estimate_mb <= main && headroom >= FITS_HEADROOM {
        Fit::Fits
    } else if estimate_mb <= main {
        Fit::Tight
    } else if budget.offload_mb > 0 && estimate_mb <= main + budget.offload_mb {
        Fit::Offload
    } else {
        Fit::WontRun
    };
    FitReport {
        fit,
        estimate_mb,
        budget_mb: main,
        offload_mb: budget.offload_mb,
        headroom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Backend, CpuInfo, GpuInfo, GpuVendor};

    fn profile(ram_mb: u64, gpus: Vec<GpuInfo>) -> HardwareProfile {
        HardwareProfile {
            os: "windows".into(),
            os_version: None,
            arch: "x86_64".into(),
            cpu: CpuInfo {
                brand: "test".into(),
                physical_cores: Some(8),
                logical_cores: 16,
                features: vec!["avx2".into()],
                ..Default::default()
            },
            ram_mb,
            gpus,
            disks: vec![],
            backends: vec![Backend::Cpu],
            probed_at: 0,
            memory_bandwidth_gbs: None,
            probe_version: 0,
        }
    }

    fn gpu(vendor: GpuVendor, vram_mb: u64, unified: bool) -> GpuInfo {
        GpuInfo {
            vendor,
            name: "gpu".into(),
            vram_mb,
            driver: None,
            unified,
            source: "test".into(),
            ..Default::default()
        }
    }

    /// The development machine: RTX 3090 (24 GB) + 64 GB RAM.
    fn rtx3090() -> HardwareProfile {
        profile(65_536, vec![gpu(GpuVendor::Nvidia, 24_576, false)])
    }

    #[test]
    fn compact_kv_lets_gemma_12b_fit_an_8_gb_card() {
        let b = MemoryBudget {
            kind: BudgetKind::Gpu,
            main_mb: 8192 - GPU_RESERVE_MB,
            offload_mb: 16_000,
            device: None,
            speed: None,
        };
        // Gemma 4 12B IQ4_XS: 7 713 MB with an f16 cache (measured 7 181 MiB),
        // ≈7 097 MB with q8_0 (measured 6 927 MiB).
        assert_eq!(fit(7_713, &b).fit, Fit::Offload);
        assert_eq!(llm_compact_estimate_mb(7_713), 7_097);
        let r = llm_fit(7_713, &b);
        assert_eq!(r.fit, Fit::Tight);
        assert_eq!(r.estimate_mb, 7_097);
        // Q4_K_M (8 496 MB) still needs offloading.
        assert_eq!(llm_fit(8_496, &b).fit, Fit::Offload);
        // Comfortable fits keep the default estimate.
        assert_eq!(llm_fit(4_000, &b).estimate_mb, 4_000);
    }

    #[test]
    fn rtx3090_labels() {
        let b = budget(&rtx3090(), 70);
        assert_eq!(b.kind, BudgetKind::Gpu);
        assert_eq!(b.main_mb, 24_064);
        // 9B Q4 (~6.8 GB) fits easily
        assert_eq!(fit(6_800, &b).fit, Fit::Fits);
        // ~21.5 GB leaves < 15% headroom
        assert_eq!(fit(21_500, &b).fit, Fit::Tight);
        // 27B Q8 (~30 GB) only with offload
        assert_eq!(fit(30_000, &b).fit, Fit::Offload);
        // more than VRAM + usable RAM
        assert_eq!(fit(90_000, &b).fit, Fit::WontRun);
    }

    #[test]
    fn music_past_vram_needs_the_estimate_in_ram() {
        // RTX 2070 8 GB, 16 GB RAM: budget 7 680 MB of VRAM, ~12 GB of usable RAM.
        let b = MemoryBudget {
            kind: BudgetKind::Gpu,
            main_mb: 7_680,
            offload_mb: 12_279,
            device: None,
            speed: None,
        };
        assert_eq!(fit(14_018, &b).fit, Fit::Offload);
        assert_eq!(music_fit(14_018, &b).fit, Fit::WontRun); // turbo + LM 1.7B
        assert_eq!(music_fit(10_950, &b).fit, Fit::Offload); // turbo + LM 0.6B
        assert_eq!(music_fit(6_000, &b).fit, fit(6_000, &b).fit);
    }

    #[test]
    fn integrated_gpu_with_shared_memory_is_ignored() {
        // Ubuntu mini PC: the OS reports 11.8 GB for Intel HD Graphics 620 – that is RAM.
        let mut igpu = gpu(GpuVendor::Intel, 11_802, false);
        igpu.name = "HD Graphics 620".into();
        let p = profile(15_736, vec![igpu]);
        assert_eq!(budget(&p, 70).kind, BudgetKind::Cpu);

        let mut apu = gpu(GpuVendor::Amd, 8_192, false);
        apu.name = "AMD Radeon(TM) Graphics".into();
        assert!(apu.is_integrated());
        let mut rx = gpu(GpuVendor::Amd, 16_384, false);
        rx.name = "AMD Radeon RX 7800 XT".into();
        assert!(!rx.is_integrated());
        let mut arc = gpu(GpuVendor::Intel, 16_384, false);
        arc.name = "Intel(R) Arc(TM) A770 Graphics".into();
        assert!(!arc.is_integrated());
    }

    #[test]
    fn integrated_gpu_is_ignored() {
        let p = profile(16_384, vec![gpu(GpuVendor::Intel, 128, false)]);
        let b = budget(&p, 70);
        assert_eq!(b.kind, BudgetKind::Cpu);
        assert_eq!(b.main_mb, 12_288);
        assert_eq!(fit(13_000, &b).fit, Fit::WontRun, "no offload on CPU");
    }

    #[test]
    fn apple_unified_memory_uses_factor() {
        let p = profile(32_768, vec![gpu(GpuVendor::Apple, 32_768, true)]);
        let b = budget(&p, 70);
        assert_eq!(b.kind, BudgetKind::Unified);
        assert_eq!(b.main_mb, 22_937);
        assert_eq!(b.offload_mb, 0);
        assert_eq!(fit(25_000, &b).fit, Fit::WontRun);
    }

    #[test]
    fn largest_gpu_is_primary() {
        let p = profile(
            32_768,
            vec![
                gpu(GpuVendor::Intel, 512, false),
                gpu(GpuVendor::Amd, 16_384, false),
            ],
        );
        assert_eq!(budget(&p, 70).main_mb, 15_872);
    }
}

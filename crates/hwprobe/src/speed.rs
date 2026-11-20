//! Speed estimate per model and mode (docs/adr/0016-speed-tiers.md).
//!
//! A deliberately small roofline model:
//!
//! * **Chat** – generating a token reads the active weights once, so tokens/s ≈
//!   effective memory bandwidth / active weight bytes; reading the prompt is
//!   compute-bound: tokens/s ≈ effective FLOPS / (2 × active parameters).
//!   With RAM offload the time per token is the sum of both memory pools.
//! * **Transcription** (whisper.cpp) – compute-bound encoder:
//!   seconds per audio second ≈ `STT_TFLOP_PER_B` × parameters / effective TFLOPS,
//!   plus a fixed decoding latency on GPUs.
//! * **Images** (stable-diffusion.cpp) – compute-bound: seconds per 1024² image ≈
//!   `IMAGE_TFLOP_PER_B` × parameters / effective TFLOPS.
//! * **Narration** – Piper always runs on the CPU (ONNX Runtime) and is
//!   compute-bound like Whisper. Autoregressive TTS (Chatterbox) generates ~50
//!   forward passes per audio second; on GPUs the per-token latency of the
//!   PyTorch loop dominates, on CPUs memory bandwidth and the vocoder do
//!   (measured on the RTX 3090 and the i5-11400F, ADR 0018).
//!
//! Efficiency factors and constants are calibrated with measurements listed in
//! the ADR (RTX 3090, i5-11400F, i5-7300U) and published llama.cpp Apple
//! Silicon results. Results are levels, not promises: numbers go to tooltips.

use serde::{Deserialize, Serialize};

use crate::fit::{BudgetKind, Fit, FitReport};
use crate::{GpuInfo, GpuVendor, HardwareProfile};

/// Ordered from worst to best, so `min` gives the weaker of two levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speed {
    /// Red: too slow to be useful, or does not run at all.
    TooSlow,
    /// Orange: works, but you wait.
    Slow,
    /// Yellow: usable.
    Usable,
    /// Green: fast.
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedDevice {
    Gpu,
    Cpu,
    /// Split between GPU memory and RAM (offload).
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedUnit {
    /// Chat: generated tokens per second.
    TokensPerSecond,
    /// Transcription/narration: audio seconds processed per wall-clock second.
    Realtime,
    /// Images: seconds per 1024×1024 image.
    SecondsPerImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedReason {
    /// Not enough memory in total (fit = won't run).
    NotEnoughMemory,
    /// Image models are not offered without a graphics card (PRD §15 item 14).
    NeedsGpu,
}

/// Effective throughput of this computer, derived once per hardware profile.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SpeedBasis {
    /// Effective GPU memory bandwidth (0 = no usable GPU).
    pub gpu_bandwidth_gbs: f32,
    /// Effective GPU matrix throughput.
    pub gpu_tflops: f32,
    pub cpu_bandwidth_gbs: f32,
    pub cpu_tflops: f32,
    /// whisper.cpp throughput on the CPU (it gains little from AVX-512, unlike
    /// llama.cpp prompt reading, so it has its own per-core rate).
    #[serde(default)]
    pub cpu_stt_tflops: f32,
    /// `cuda`, `vulkan` or `metal` when a GPU is used.
    pub accelerator: Option<String>,
    /// GPU numbers guessed from the VRAM size (model not recognised).
    pub estimated: bool,
}

impl SpeedBasis {
    pub fn has_gpu(&self) -> bool {
        self.gpu_bandwidth_gbs > 0.0 && self.gpu_tflops > 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Llm,
    Image,
    Stt,
    /// Non-autoregressive TTS (Piper).
    Tts,
    /// Autoregressive TTS (Chatterbox and similar).
    TtsAutoregressive,
    /// Song generation (ACE-Step: language model plan + diffusion, ADR 0022).
    Music,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workload {
    pub kind: WorkKind,
    /// Size of the model files.
    pub weights_bytes: u64,
    /// Total parameters in billions.
    pub params_b: f32,
    /// Active parameters per token for mixture-of-experts models.
    pub active_params_b: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeedReport {
    pub speed: Speed,
    /// In `unit`; 0 when the model does not run.
    pub value: f32,
    pub unit: SpeedUnit,
    /// Chat only: prompt reading speed in tokens per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_per_s: Option<f32>,
    pub device: SpeedDevice,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<SpeedReason>,
}

// --- calibration ------------------------------------------------------------

/// CPU memory bandwidth one core can drive in llama.cpp token generation
/// (measured 9.5 GB/s per core on i5-11400F, 8.9 on i5-7300U).
const CPU_BW_PER_CORE_SIMD: f32 = 9.0;
/// Without AVX2/NEON (older Celeron/Pentium/Atom) the kernels are much slower.
const CPU_BW_PER_CORE_PLAIN: f32 = 4.0;
/// Typical dual-channel DDR4/DDR5 desktop or laptop (measured 36.7 GB/s on
/// DDR4-3200 dual channel with 6 cores).
const CPU_RAM_BW_CAP: f32 = 36.0;
/// Effective GFLOPS per core in prompt processing (measured: AVX-512 142 on
/// i5-11400F, AVX2 55–69 on i5-7300U).
const CPU_GFLOPS_AVX512: f32 = 140.0;
const CPU_GFLOPS_AVX2: f32 = 60.0;
const CPU_GFLOPS_PLAIN: f32 = 20.0;

/// Share of peak bandwidth llama.cpp reaches in-app (RTX 3090: Bielik 11B
/// Q6_K at ≈46 tok/s → 421 of 936 GB/s).
const CUDA_BW_EFF: f32 = 0.45;
/// Prompt processing vs FP32 peak with tensor cores (RTX 3090: pp512 ≈ 3070,
/// 30k tokens in 16 s ≈ 1875 tok/s → ≈2400 tok/s at 11 B parameters).
const CUDA_TENSOR_EFF: f32 = 1.5;
const CUDA_NO_TENSOR_EFF: f32 = 0.7;
const VULKAN_BW_EFF: f32 = 0.40;
const VULKAN_COMPUTE_EFF: f32 = 0.6;
/// llama.cpp Apple Silicon benchmark (M1 8c: 7B Q4_0 tg ≈14, pp ≈117;
/// M2 Max 38c: tg ≈61, pp ≈670) → ≈0.6 of peak for both.
const METAL_BW_EFF: f32 = 0.62;
const METAL_COMPUTE_EFF: f32 = 0.62;

/// MoE: bytes read per token relative to the active share of the file (shared
/// layers, routing, less efficient expert kernels). Ryzen 5 7430U: Qwen3.6
/// 35B-A3B UD-Q3_K_M generates 10.7 tok/s at 36 GB/s → 3.4 GB per token, 2.4×
/// the active share (16.6 GB × 3/35).
const MOE_OVERHEAD: f32 = 2.4;
/// Prompt length used for the chat level (a new message with some pasted text;
/// the system prompt and tools are cached between turns).
const CHAT_PROMPT_TOKENS: f32 = 500.0;

/// whisper.cpp: TFLOP per billion parameters per audio second (i5-11400F,
/// large-v3-turbo q5_0: 16.5 s encoder + ≈2 s decoder per 30 s window).
const STT_TFLOP_PER_B: f32 = 0.66;
/// whisper.cpp effective GFLOPS per CPU core in the same units: i5-11400F
/// (AVX-512) ≈150, Ryzen 5 7430U (AVX2) ≈180 – AVX-512 does not help here.
const CPU_STT_GFLOPS_SIMD: f32 = 160.0;
const CPU_STT_GFLOPS_PLAIN: f32 = 50.0;
/// Decoding latency on GPUs per audio second (RTX 3090: 14 min in ≈26 s).
const STT_GPU_LATENCY: f32 = 0.02;
/// sd.cpp: TFLOP per billion parameters per 1024² image (RTX 3090:
/// Z-Image Turbo Q4_K in 10.6 s warm).
const IMAGE_TFLOP_PER_B: f32 = 51.0;
/// Piper: TFLOP per billion parameters per audio second, against the
/// Whisper-style CPU throughput (i5-11400F: 8.3 s of speech in 0.36 s ≈ 23×).
const TTS_TFLOP_PER_B: f32 = 1.33;
/// Autoregressive TTS: forward passes per audio second (25 Hz speech tokens × CFG).
const TTS_AR_STEPS_PER_S: f32 = 50.0;
/// Share of the files that is the autoregressive backbone.
const TTS_AR_BACKBONE_SHARE: f32 = 0.4;
/// GPU latency of the token loop per audio second (RTX 3090: Chatterbox V3
/// speaks 12 s in 11.5 s warm, of which ≈0.2 s is bandwidth and vocoder).
const TTS_AR_GPU_LATENCY: f32 = 0.74;
/// Vocoder (flow matching) TFLOP per audio second (i5-11400F: 4.4 s in 25 s).
const TTS_AR_TFLOP_PER_S: f32 = 3.4;
/// Music: TFLOP per GB of model files per audio second (ACE-Step 1.5 turbo +
/// LM 1.7B, 10.1 GB, PyTorch LM backend: RTX 3090 makes 60 s in 34 s warm-up
/// included, RTF ≈ 1.8; i5-11400F 10 s in 135 s, RTF ≈ 0.07). ADR 0022.
const MUSIC_TFLOP_PER_GB: f32 = 1.6;

// --- basis --------------------------------------------------------------------

/// Effective throughput for `profile` given the memory budget kind (which
/// already decides whether a GPU is used: integrated graphics are not).
pub fn speed_basis(profile: &HardwareProfile, kind: BudgetKind) -> SpeedBasis {
    let cpu = &profile.cpu;
    let cores = cpu.effective_cores();
    let simd = cpu.has("avx2") || cpu.has("neon") || cpu.has("avx512f");
    let per_core_bw = if simd {
        CPU_BW_PER_CORE_SIMD
    } else {
        CPU_BW_PER_CORE_PLAIN
    };
    let ram_cap = profile
        .memory_bandwidth_gbs
        .map(|bw| bw * 0.6)
        .unwrap_or(CPU_RAM_BW_CAP);
    let gflops = if cpu.has("avx512f") {
        CPU_GFLOPS_AVX512
    } else if simd {
        CPU_GFLOPS_AVX2
    } else {
        CPU_GFLOPS_PLAIN
    };
    let mut basis = SpeedBasis {
        cpu_bandwidth_gbs: (per_core_bw * cores).min(ram_cap),
        cpu_tflops: gflops * cores / 1000.0,
        cpu_stt_tflops: if simd {
            CPU_STT_GFLOPS_SIMD
        } else {
            CPU_STT_GFLOPS_PLAIN
        } * cores
            / 1000.0,
        ..Default::default()
    };
    if kind == BudgetKind::Cpu {
        return basis;
    }
    let Some(gpu) = profile.primary_gpu() else {
        return basis;
    };
    let (bw, tflops, estimated) = gpu_peak(gpu);
    let (bw_eff, compute_eff, accel) = match gpu.vendor {
        GpuVendor::Nvidia => {
            let tensor = !gpu.name.to_ascii_lowercase().contains("gtx");
            let eff = if tensor {
                CUDA_TENSOR_EFF
            } else {
                CUDA_NO_TENSOR_EFF
            };
            (CUDA_BW_EFF, eff, "cuda")
        }
        GpuVendor::Apple => (METAL_BW_EFF, METAL_COMPUTE_EFF, "metal"),
        _ => (VULKAN_BW_EFF, VULKAN_COMPUTE_EFF, "vulkan"),
    };
    basis.gpu_bandwidth_gbs = bw * bw_eff;
    basis.gpu_tflops = tflops * compute_eff;
    basis.accelerator = Some(accel.into());
    basis.estimated = estimated;
    basis
}

/// Peak bandwidth and FP32 TFLOPS; guessed from the memory size when the
/// GPU was not recognised (`true` in the third field).
fn gpu_peak(gpu: &GpuInfo) -> (f32, f32, bool) {
    let gb = gpu.vram_mb as f32 / 1024.0;
    let mut estimated = false;
    let bw = gpu.memory_bandwidth_gbs.unwrap_or_else(|| {
        estimated = true;
        if gpu.unified {
            100.0
        } else {
            (gb * 30.0).clamp(150.0, 1000.0)
        }
    });
    let tflops = gpu.fp32_tflops.unwrap_or_else(|| {
        estimated = true;
        if gpu.unified {
            3.5
        } else {
            (gb * 1.2).clamp(4.0, 40.0)
        }
    });
    (bw, tflops, estimated)
}

// --- estimate -------------------------------------------------------------------

/// Bits per weight of common quantisations (llama.cpp averages incl. scales).
fn bits_per_weight(label: &str) -> f32 {
    let l = label.to_ascii_lowercase();
    const TABLE: &[(&str, f32)] = &[
        ("iq1", 1.8),
        ("iq2", 2.4),
        ("q2_k", 2.9),
        ("iq3", 3.1),
        ("q3_k", 3.9),
        ("iq4", 4.4),
        ("q4_0", 4.5),
        ("q4_1", 5.0),
        ("q4_k", 4.85),
        ("q4", 4.85),
        ("q5_0", 5.5),
        ("q5_1", 6.0),
        ("q5_k", 5.7),
        ("q5", 5.7),
        ("q6_k", 6.56),
        ("q6", 6.56),
        ("q8_0", 8.5),
        ("q8", 8.5),
        ("bf16", 16.0),
        ("f16", 16.0),
        ("fp16", 16.0),
        ("f32", 32.0),
        ("fp32", 32.0),
    ];
    TABLE
        .iter()
        .find(|(k, _)| l.contains(k))
        .map(|(_, b)| *b)
        .unwrap_or(16.0)
}

/// Parameters (billions) implied by the file size and the quantisation in the
/// variant id/label (`q4_k_m`, `q8_0`, `f16`, …; unknown → 16 bit).
pub fn params_from_size(bytes: u64, quant_label: &str) -> f32 {
    bytes as f32 * 8.0 / bits_per_weight(quant_label) / 1e9
}

fn level(value: f32, fast: f32, usable: f32, slow: f32) -> Speed {
    if value >= fast {
        Speed::Fast
    } else if value >= usable {
        Speed::Usable
    } else if value >= slow {
        Speed::Slow
    } else {
        Speed::TooSlow
    }
}

/// Narration is not interactive: speaking as fast as it is heard is usable,
/// a minute of speech taking four minutes or more is too slow.
fn narration_level(rtf: f32) -> Speed {
    level(rtf, 2.0, 0.8, 0.25)
}

/// Lower is better (seconds).
fn level_secs(secs: f32, fast: f32, usable: f32, slow: f32) -> Speed {
    level(-secs, -fast, -usable, -slow)
}

fn unit_of(kind: WorkKind) -> SpeedUnit {
    match kind {
        WorkKind::Llm => SpeedUnit::TokensPerSecond,
        WorkKind::Image => SpeedUnit::SecondsPerImage,
        WorkKind::Stt | WorkKind::Tts | WorkKind::TtsAutoregressive | WorkKind::Music => {
            SpeedUnit::Realtime
        }
    }
}

/// Estimated speed of `work` on `basis`; `fit` is the memory fit of the same variant.
pub fn estimate(work: &Workload, basis: &SpeedBasis, fit: &FitReport) -> SpeedReport {
    let unit = unit_of(work.kind);
    // Piper runs on the CPU even when a graphics card is present.
    let gpu = basis.has_gpu() && work.kind != WorkKind::Tts;
    // Without a graphics card "needs a GPU" says more than "not enough memory".
    let needs_gpu = work.kind == WorkKind::Image && !gpu;
    // A model that fits but needs a GPU is still estimated (for the tooltip) below.
    if fit.fit == Fit::WontRun {
        return SpeedReport {
            speed: Speed::TooSlow,
            value: 0.0,
            unit,
            prompt_tokens_per_s: None,
            device: if gpu {
                SpeedDevice::Gpu
            } else {
                SpeedDevice::Cpu
            },
            reason: Some(if needs_gpu {
                SpeedReason::NeedsGpu
            } else {
                SpeedReason::NotEnoughMemory
            }),
        };
    }
    // Share of the work done from GPU memory.
    let gpu_share = match (gpu, fit.fit) {
        (false, _) => 0.0,
        (true, Fit::Offload) if fit.estimate_mb > 0 => {
            (fit.budget_mb as f32 / fit.estimate_mb as f32).clamp(0.0, 1.0)
        }
        (true, _) => 1.0,
    };
    let device = if gpu_share >= 1.0 {
        SpeedDevice::Gpu
    } else if gpu_share <= 0.0 {
        SpeedDevice::Cpu
    } else {
        SpeedDevice::Mixed
    };
    // Compute: GPU when used; with offload the GPU still does the heavy
    // batches but waits for weights from RAM, so take the weighted harmonic mean.
    let tflops = if gpu_share <= 0.0 {
        basis.cpu_tflops
    } else {
        1.0 / (gpu_share / basis.gpu_tflops + (1.0 - gpu_share) / (basis.gpu_tflops * 0.25))
    }
    .max(0.001);
    let params = work.params_b.max(0.001);
    let active = work.active_params_b.unwrap_or(params).clamp(0.001, params);

    let (speed, value, prompt) = match work.kind {
        WorkKind::Llm => {
            let mut bytes = work.weights_bytes as f32 / 1e9 * (active / params);
            if active < params {
                bytes *= MOE_OVERHEAD;
            }
            let secs_per_token = gpu_share * bytes / basis.gpu_bandwidth_gbs.max(0.001)
                + (1.0 - gpu_share) * bytes / basis.cpu_bandwidth_gbs.max(0.001);
            let tps = 1.0 / secs_per_token.max(1e-6);
            let pp = tflops * 1000.0 / (2.0 * active);
            let generation = level(tps, 15.0, 6.0, 2.0);
            let read = level_secs(CHAT_PROMPT_TOKENS / pp, 10.0, 30.0, 90.0);
            (generation.min(read), tps, Some(pp))
        }
        WorkKind::Stt => {
            let tflops = if gpu_share <= 0.0 {
                basis.cpu_stt_tflops.max(0.001)
            } else {
                tflops
            };
            let mut secs = STT_TFLOP_PER_B * params / tflops;
            if gpu_share > 0.0 {
                secs += STT_GPU_LATENCY;
            }
            let rtf = 1.0 / secs;
            (level(rtf, 8.0, 2.0, 0.2), rtf, None)
        }
        WorkKind::Image => {
            let secs = IMAGE_TFLOP_PER_B * params / tflops;
            (level_secs(secs, 20.0, 60.0, 240.0), secs, None)
        }
        WorkKind::Tts => {
            let rtf = basis.cpu_stt_tflops.max(0.001) / (TTS_TFLOP_PER_B * params);
            (narration_level(rtf), rtf, None)
        }
        WorkKind::TtsAutoregressive => {
            let gb_per_step = work.weights_bytes as f32 / 1e9 * TTS_AR_BACKBONE_SHARE;
            let bw =
                gpu_share * basis.gpu_bandwidth_gbs + (1.0 - gpu_share) * basis.cpu_bandwidth_gbs;
            let mut secs =
                gb_per_step * TTS_AR_STEPS_PER_S / bw.max(0.001) + TTS_AR_TFLOP_PER_S / tflops;
            if gpu_share > 0.0 {
                secs += TTS_AR_GPU_LATENCY;
            }
            let rtf = 1.0 / secs;
            (narration_level(rtf), rtf, None)
        }
        WorkKind::Music => {
            // A 3-minute song: fast ≤ 30 s, usable ≤ 2 min, slow ≤ 10 min.
            let secs = MUSIC_TFLOP_PER_GB * work.weights_bytes as f32 / 1e9 / tflops;
            let rtf = 1.0 / secs.max(1e-6);
            (level(rtf, 6.0, 1.5, 0.3), rtf, None)
        }
    };
    let reason = needs_gpu.then_some(SpeedReason::NeedsGpu);
    SpeedReport {
        speed: if reason.is_some() {
            Speed::TooSlow
        } else {
            speed
        },
        value,
        unit,
        prompt_tokens_per_s: prompt,
        device,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit::{budget, fit};
    use crate::{Backend, CpuInfo, gpu_db};

    const MIB: u64 = 1024 * 1024;

    fn cpu(brand: &str, physical: usize, logical: usize, features: &[&str]) -> CpuInfo {
        CpuInfo {
            brand: brand.into(),
            physical_cores: Some(physical),
            logical_cores: logical,
            features: features.iter().map(|f| f.to_string()).collect(),
            ..Default::default()
        }
    }

    fn gpu(vendor: GpuVendor, name: &str, vram_mb: u64, unified: bool) -> GpuInfo {
        let mut g = GpuInfo {
            vendor,
            name: name.into(),
            vram_mb,
            unified,
            source: "test".into(),
            ..Default::default()
        };
        gpu_db::enrich(&mut g);
        g
    }

    fn profile(cpu: CpuInfo, ram_mb: u64, gpus: Vec<GpuInfo>) -> HardwareProfile {
        let memory_bandwidth_gbs = gpus
            .iter()
            .find(|g| g.unified)
            .and_then(|g| g.memory_bandwidth_gbs);
        HardwareProfile {
            os: "test".into(),
            os_version: None,
            arch: "x86_64".into(),
            cpu,
            ram_mb,
            gpus,
            disks: vec![],
            backends: vec![Backend::Cpu],
            probed_at: 0,
            memory_bandwidth_gbs,
            probe_version: crate::PROBE_VERSION,
        }
    }

    /// Development machine: RTX 3090 24 GB, i5-11400F (6c/12t, AVX-512), 64 GB.
    fn rtx3090() -> HardwareProfile {
        profile(
            cpu("Intel Core i5-11400F", 6, 12, &["avx2", "avx512f"]),
            65_536,
            vec![gpu(
                GpuVendor::Nvidia,
                "NVIDIA GeForce RTX 3090",
                24_576,
                false,
            )],
        )
    }

    /// Ubuntu mini PC: i5-7300U (2c/4t, AVX2), Intel HD Graphics 620, 16 GB.
    fn mini_pc() -> HardwareProfile {
        profile(
            cpu("Intel Core i5-7300U", 2, 4, &["avx2"]),
            15_736,
            vec![gpu(
                GpuVendor::Intel,
                "Intel(R) HD Graphics 620 (KBL GT2)",
                11_802,
                false,
            )],
        )
    }

    /// Cheap 2-core laptop: Celeron N4020 (2c/2t, no AVX2), UHD 600, 8 GB.
    fn celeron_laptop() -> HardwareProfile {
        profile(
            cpu("Intel Celeron N4020", 2, 2, &[]),
            7_822,
            vec![gpu(
                GpuVendor::Intel,
                "Intel(R) UHD Graphics 600",
                128,
                false,
            )],
        )
    }

    /// MacBook Air M2: 8-core CPU (4P+4E), 10-core GPU, 16 GB unified memory.
    fn apple_m2() -> HardwareProfile {
        let mut c = cpu("Apple M2", 8, 8, &["neon"]);
        c.performance_cores = Some(4);
        c.efficiency_cores = Some(4);
        let mut g = GpuInfo {
            vendor: GpuVendor::Apple,
            name: "Apple M2".into(),
            vram_mb: 16_384,
            unified: true,
            source: "test".into(),
            compute_units: Some(10),
            ..Default::default()
        };
        gpu_db::enrich(&mut g);
        let mut p = profile(c, 16_384, vec![g]);
        p.arch = "aarch64".into();
        p
    }

    /// Ryzen 7 7840U laptop: 8c/16t (AVX-512), Radeon 780M (integrated), 32 GB.
    fn amd_apu() -> HardwareProfile {
        profile(
            cpu("AMD Ryzen 7 7840U", 8, 16, &["avx2", "avx512f"]),
            31_960,
            vec![gpu(
                GpuVendor::Amd,
                "AMD Radeon 780M Graphics",
                4_096,
                false,
            )],
        )
    }

    struct Case {
        kind: WorkKind,
        size_mb: u64,
        estimate_mb: u64,
        quant: &'static str,
        params_b: Option<f32>,
        active_b: Option<f32>,
    }

    // Catalog sizes (catalog/models.json).
    const BIELIK_11B_Q6: Case = Case {
        kind: WorkKind::Llm,
        size_mb: 8_739,
        estimate_mb: 10_638,
        quant: "q6_k",
        params_b: Some(11.0),
        active_b: None,
    };
    const BIELIK_45B_Q8: Case = Case {
        kind: WorkKind::Llm,
        size_mb: 4_826,
        estimate_mb: 6_334,
        quant: "q8_0",
        params_b: Some(4.5),
        active_b: None,
    };
    const LFM3_Q4: Case = Case {
        kind: WorkKind::Llm,
        size_mb: 1_048,
        estimate_mb: 2_178,
        quant: "q4_k_m",
        params_b: Some(1.7),
        active_b: None,
    };
    const QWEN_MOE_Q3: Case = Case {
        kind: WorkKind::Llm,
        size_mb: 15_831,
        estimate_mb: 18_439,
        quant: "ud_q3_k_m",
        params_b: Some(35.0),
        active_b: Some(3.0),
    };
    const QWEN_27B_Q8: Case = Case {
        kind: WorkKind::Llm,
        size_mb: 27_701,
        estimate_mb: 31_496,
        quant: "q8_0",
        params_b: Some(27.0),
        active_b: None,
    };
    const WHISPER_TURBO: Case = Case {
        kind: WorkKind::Stt,
        size_mb: 547,
        estimate_mb: 1_122,
        quant: "q5_0",
        params_b: None,
        active_b: None,
    };
    const WHISPER_SMALL: Case = Case {
        kind: WorkKind::Stt,
        size_mb: 252,
        estimate_mb: 679,
        quant: "q8_0",
        params_b: None,
        active_b: None,
    };
    const WHISPER_BASE: Case = Case {
        kind: WorkKind::Stt,
        size_mb: 77,
        estimate_mb: 417,
        quant: "q8_0",
        params_b: None,
        active_b: None,
    };
    const Z_IMAGE_Q4: Case = Case {
        kind: WorkKind::Image,
        size_mb: 6_386,
        estimate_mb: 8_688,
        quant: "q4_k",
        params_b: None,
        active_b: None,
    };
    const PIPER: Case = Case {
        kind: WorkKind::Tts,
        size_mb: 60,
        estimate_mb: 579,
        quant: "medium",
        params_b: None,
        active_b: None,
    };
    const CHATTERBOX: Case = Case {
        kind: WorkKind::TtsAutoregressive,
        size_mb: 3_060,
        estimate_mb: 4_479,
        quant: "v2",
        params_b: None,
        active_b: None,
    };

    fn run(p: &HardwareProfile, c: &Case) -> SpeedReport {
        let b = budget(p, 70);
        let basis = speed_basis(p, b.kind);
        let bytes = c.size_mb * MIB;
        let work = Workload {
            kind: c.kind,
            weights_bytes: bytes,
            params_b: c
                .params_b
                .unwrap_or_else(|| params_from_size(bytes, c.quant)),
            active_params_b: c.active_b,
        };
        estimate(&work, &basis, &fit(c.estimate_mb, &b))
    }

    #[test]
    fn params_from_quantised_size() {
        // Bielik 11B Q6_K: llama-bench reports 11.17 B parameters
        assert!((params_from_size(8_739 * MIB, "q6_k") - 11.17).abs() < 0.1);
        // Whisper large-v3-turbo has ≈809 M parameters
        assert!((params_from_size(547 * MIB, "q5_0") - 0.83).abs() < 0.05);
    }

    #[test]
    fn rtx3090_matches_measurements() {
        let p = rtx3090();
        let chat = run(&p, &BIELIK_11B_Q6);
        assert_eq!(chat.speed, Speed::Fast);
        assert_eq!(chat.device, SpeedDevice::Gpu);
        // measured ≈46 tok/s in the app
        assert!((40.0..=60.0).contains(&chat.value), "{chat:?}");
        // pp: 30k tokens in 16 s (≈1875 tok/s), pp512 ≈3070 tok/s
        let pp = chat.prompt_tokens_per_s.unwrap();
        assert!((1800.0..=3200.0).contains(&pp), "{pp}");

        let stt = run(&p, &WHISPER_TURBO);
        assert_eq!(stt.speed, Speed::Fast);
        // 14 min of audio in ≈26 s → ≈32× realtime
        assert!((25.0..=40.0).contains(&stt.value), "{stt:?}");

        let img = run(&p, &Z_IMAGE_Q4);
        assert_eq!(img.speed, Speed::Fast);
        assert!((8.0..=14.0).contains(&img.value), "{img:?}");

        // Piper on the CPU: 8.3 s of speech in 0.36 s (≈23×)
        let piper = run(&p, &PIPER);
        assert_eq!((piper.speed, piper.device), (Speed::Fast, SpeedDevice::Cpu));
        assert!((18.0..=30.0).contains(&piper.value), "{piper:?}");
        // Chatterbox V3 warm: 12 s of speech in 11.5 s (≈1.05×)
        let cbx = run(&p, &CHATTERBOX);
        assert_eq!(cbx.speed, Speed::Usable, "{cbx:?}");
        assert!((0.9..=1.2).contains(&cbx.value), "{cbx:?}");

        // 27B Q8 does not fit in 24 GB: offload makes it slow
        let big = run(&p, &QWEN_27B_Q8);
        assert_eq!(big.device, SpeedDevice::Mixed);
        assert_eq!(big.speed, Speed::Slow, "{big:?}");
    }

    #[test]
    fn rtx3090_cpu_side_matches_measurements() {
        // i5-11400F alone (6 threads): Bielik 11B Q6_K tg 4.0 tok/s, pp 38 tok/s
        let p = rtx3090();
        let basis = speed_basis(&p, BudgetKind::Cpu);
        let tg = basis.cpu_bandwidth_gbs / (8_739.0 * MIB as f32 / 1e9);
        assert!((3.5..=4.5).contains(&tg), "{tg}");
        let pp = basis.cpu_tflops * 1000.0 / (2.0 * 11.17);
        assert!((33.0..=43.0).contains(&pp), "{pp}");
        // Chatterbox V3 on the CPU (forced): 4.4 s of speech in 25 s (≈0.17×)
        let b = budget(&p, 70);
        let cbx = estimate(
            &Workload {
                kind: WorkKind::TtsAutoregressive,
                weights_bytes: CHATTERBOX.size_mb * MIB,
                params_b: 0.5,
                active_params_b: None,
            },
            &basis,
            &fit(CHATTERBOX.estimate_mb, &b),
        );
        assert_eq!(cbx.speed, Speed::TooSlow, "{cbx:?}");
        assert!((0.14..=0.21).contains(&cbx.value), "{cbx:?}");
    }

    #[test]
    fn mini_pc_with_hd620_runs_on_cpu() {
        let p = mini_pc();
        assert!(
            !speed_basis(&p, budget(&p, 70).kind).has_gpu(),
            "iGPU is not used"
        );
        // measured: LFM3 1.7B Q4_K_M tg 16.2 tok/s, pp512 39.8 tok/s
        let lfm = run(&p, &LFM3_Q4);
        assert!((14.0..=19.0).contains(&lfm.value), "{lfm:?}");
        assert_eq!(lfm.speed, Speed::Usable);
        // measured: Bielik 4.5B Q8_0 pp512 11.5 tok/s
        let bielik = run(&p, &BIELIK_45B_Q8);
        assert_eq!(bielik.speed, Speed::Slow, "{bielik:?}");
        let pp = bielik.prompt_tokens_per_s.unwrap();
        assert!((10.0..=16.0).contains(&pp), "{pp}");

        assert_eq!(run(&p, &WHISPER_TURBO).speed, Speed::Slow);
        assert_eq!(run(&p, &WHISPER_BASE).speed, Speed::Usable);

        let img = run(&p, &Z_IMAGE_Q4);
        assert_eq!(img.speed, Speed::TooSlow);
        assert_eq!(img.reason, Some(SpeedReason::NeedsGpu));
        // Piper warm: 8.3 s of speech in 0.6 s (≈14×); the model is conservative
        let piper = run(&p, &PIPER);
        assert_eq!(piper.speed, Speed::Fast);
        assert!((5.0..=20.0).contains(&piper.value), "{piper:?}");
        // Chatterbox V3 cloned voice: 5.2 s of speech in 74 s (≈0.07×)
        let cbx = run(&p, &CHATTERBOX);
        assert_eq!(cbx.speed, Speed::TooSlow);
        assert!(cbx.value < 0.1, "{cbx:?}");
        // the MoE model needs ~18 GB: does not fit in 16 GB
        assert_eq!(
            run(&p, &QWEN_MOE_Q3).reason,
            Some(SpeedReason::NotEnoughMemory)
        );
    }

    #[test]
    fn celeron_laptop_is_slow_everywhere() {
        let p = celeron_laptop();
        let chat = run(&p, &LFM3_Q4);
        assert_eq!(chat.speed, Speed::Slow, "{chat:?}");
        assert_eq!(run(&p, &BIELIK_45B_Q8).speed, Speed::TooSlow);
        assert_eq!(run(&p, &WHISPER_TURBO).speed, Speed::TooSlow);
        assert!(
            run(&p, &WHISPER_BASE).speed >= Speed::Slow,
            "small models still work"
        );
        assert_eq!(run(&p, &Z_IMAGE_Q4).reason, Some(SpeedReason::NeedsGpu));
        assert_eq!(run(&p, &CHATTERBOX).speed, Speed::TooSlow);
    }

    #[test]
    fn apple_m2_uses_metal() {
        let p = apple_m2();
        let basis = speed_basis(&p, budget(&p, 70).kind);
        assert_eq!(basis.accelerator.as_deref(), Some("metal"));
        assert!(!basis.estimated);
        assert_eq!(run(&p, &BIELIK_45B_Q8).speed, Speed::Usable);
        assert_eq!(run(&p, &WHISPER_TURBO).speed, Speed::Usable);
        assert!(run(&p, &WHISPER_SMALL).speed >= Speed::Usable);
        // diffusion runs on the GPU, but takes minutes on a base M2
        let img = run(&p, &Z_IMAGE_Q4);
        assert_eq!(img.reason, None);
        assert!(img.speed <= Speed::Slow, "{img:?}");
    }

    #[test]
    fn amd_apu_prefers_moe() {
        let p = amd_apu();
        assert_eq!(budget(&p, 70).kind, BudgetKind::Cpu, "780M is integrated");
        assert_eq!(run(&p, &BIELIK_45B_Q8).speed, Speed::Usable);
        // 3B active parameters: usable despite 35B in total, faster than dense 4.5B
        let moe = run(&p, &QWEN_MOE_Q3);
        assert_eq!(moe.speed, Speed::Usable);
        assert!(moe.value > run(&p, &BIELIK_45B_Q8).value);
        // 8 Zen 4 cores: about twice real time, on the yellow/orange border
        assert!(matches!(
            run(&p, &WHISPER_TURBO).speed,
            Speed::Slow | Speed::Usable
        ));
        assert_eq!(run(&p, &Z_IMAGE_Q4).reason, Some(SpeedReason::NeedsGpu));
    }

    /// Ubuntu 26.04 server: Ryzen 5 7430U (6c/12t, AVX2), Vega 7 (integrated),
    /// DDR4-2667 dual channel, 32 GB. Measured with llama-bench b12110 and
    /// whisper-bench b5454 (CPU, 6 threads).
    fn ryzen_7430u() -> HardwareProfile {
        profile(
            cpu("AMD Ryzen 5 7430U with Radeon Graphics", 6, 12, &["avx2"]),
            30_489,
            vec![gpu(
                GpuVendor::Amd,
                "AMD Radeon Graphics (renoir, LLVM, DRM 3.64)",
                512,
                false,
            )],
        )
    }

    #[test]
    fn ryzen_7430u_matches_measurements() {
        let p = ryzen_7430u();
        let within = |r: &SpeedReport, measured: f32, tol: f32| {
            assert!(
                (r.value / measured - 1.0).abs() <= tol,
                "{r:?} vs measured {measured}"
            );
        };
        // tg64: LFM3 31.7, Bielik 4.5B Q8 7.31, Qwen3.6 35B-A3B Q3 10.7 tok/s
        within(&run(&p, &LFM3_Q4), 31.7, 0.15);
        within(&run(&p, &BIELIK_45B_Q8), 7.31, 0.15);
        within(&run(&p, &QWEN_MOE_Q3), 10.7, 0.15);
        assert_eq!(run(&p, &LFM3_Q4).speed, Speed::Fast);
        assert_eq!(run(&p, &BIELIK_45B_Q8).speed, Speed::Usable);
        // Whisper turbo: 13.6 s encode + ≈0.5 s decode per 30 s → ≈2.1× real time
        let turbo = run(&p, &WHISPER_TURBO);
        within(&turbo, 2.1, 0.25);
        // on the yellow/orange border (2×), like the i5-11400F (1.6×)
        assert!(matches!(turbo.speed, Speed::Slow | Speed::Usable));
        // base ≈40×, small ≈11× real time: the model is conservative for small Whispers
        assert_eq!(run(&p, &WHISPER_BASE).speed, Speed::Fast);
        assert!(run(&p, &WHISPER_SMALL).speed >= Speed::Usable);
        assert_eq!(run(&p, &Z_IMAGE_Q4).reason, Some(SpeedReason::NeedsGpu));
    }

    #[test]
    fn unknown_gpu_is_estimated_from_vram() {
        let p = profile(
            cpu("x", 8, 16, &["avx2"]),
            32_768,
            vec![gpu(
                GpuVendor::Amd,
                "AMD Radeon RX 9999 Future",
                16_384,
                false,
            )],
        );
        let basis = speed_basis(&p, budget(&p, 70).kind);
        assert!(basis.estimated);
        assert_eq!(basis.accelerator.as_deref(), Some("vulkan"));
        assert!(basis.has_gpu());
    }

    #[test]
    fn wont_run_is_red() {
        let p = celeron_laptop();
        let r = run(&p, &BIELIK_11B_Q6);
        assert_eq!(r.speed, Speed::TooSlow);
        assert_eq!(r.reason, Some(SpeedReason::NotEnoughMemory));
        assert_eq!(r.value, 0.0);
    }

    #[test]
    fn levels_are_ordered() {
        assert!(
            Speed::Fast > Speed::Usable
                && Speed::Usable > Speed::Slow
                && Speed::Slow > Speed::TooSlow
        );
        assert_eq!(Speed::Fast.min(Speed::Slow), Speed::Slow);
        let json = serde_json::to_string(&Speed::TooSlow).unwrap();
        assert_eq!(json, "\"too_slow\"");
    }
}

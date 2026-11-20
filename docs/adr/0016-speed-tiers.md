# ADR 0016: Speed tiers per model and mode; GPU/CPU compute detection

- Status: accepted
- Date: 2026-11-11

## Context

The fit labels of PRD §5 (ADR 0006) only answer "does it fit in memory". The owner
asked for an answer to "how fast will it run here", per mode: a laptop with a
dual-core CPU and no graphics card fits a 4.5B chat model in RAM, but it answers at
about 3 tokens per second, and Whisper large is slower than real time. Users should
see that before downloading, and the starter pack should prefer faster models on
weak computers (PRD §15 item 14).

## Decision

### Detection (crate `lokalsi-hwprobe`, no bundled SDKs)

| What | Windows | Linux | macOS |
|---|---|---|---|
| NVIDIA cores, SM clock, bus width, memory clock | NVML (`nvml-wrapper`, loaded at runtime) | NVML | – |
| GPU list, dedicated memory | DXGI via `sysinfo` | DRM sysfs + Vulkan heaps via `sysinfo` | IOKit via `sysinfo` |
| AMD compute units, max clock | lookup table | KFD topology `/sys/class/kfd/kfd/topology/nodes/*/properties` (`simd_count / simd_per_cu`, `max_engine_clk_fcompute`) | – |
| Intel max clock | lookup table | `gt_max_freq_mhz` (i915) / `tile0/gt0/freq0/max_freq` (xe) | – |
| Apple GPU cores | – | – | `ioreg -rc AGXAccelerator` → `gpu-core-count` |
| Hybrid CPU (P/E cores) | `GetLogicalProcessorInformationEx` → `EfficiencyClass` (`windows-sys`) | `/sys/devices/cpu_core/cpus`, `/sys/devices/cpu_atom/cpus` | `sysctl hw.perflevel0/1.physicalcpu` |
| AVX2/AVX-512/NEON | `is_x86_feature_detected!` | same | same |

**Hardware catalogue** (owner: "like GPU-Z does"): `catalog/hardware.json`, a
versioned, maintained GPU catalogue that is a first-class source next to the
drivers, embedded at build time like `models.json`. Detection fills what it can;
the catalogue supplies the rest (compute units and unit name, shader ALUs, boost
clock, peak memory bandwidth – the number that matters most for chat speed – and
integrated vs dedicated). Entries are matched by PCI `vendor:device` ID first (read
from NVML and `/sys/class/drm`), then by lower-case name patterns. Starter set:
119 GPUs – NVIDIA GTX 10/16 and RTX 20–50, AMD RX 500/5000–9000, Intel Arc A/B,
Apple M1–M5, common Intel UHD/Iris/Arc and AMD Radeon 6xxM–890M/Vega iGPUs.

Rules: values come from vendors' public specification pages (facts, no licence
restriction); third-party databases such as TechPowerUp's (GPU-Z) are not copied,
their terms do not allow it. Unknown values stay `null` – nothing is guessed. PCI
IDs are only added when read from a real machine (so far `10de:2204` RTX 3090 and
`8086:5916` HD Graphics 620); other entries match by name until IDs are verified.
Memory type and size are not stored: the OS reports the size, and the bandwidth
already reflects the type. A "relative speed" figure is not stored either: FP32
TFLOPS (ALUs × clock × 2) and bandwidth are derived in code, so the file has one
source of truth per fact. CPUs are not catalogued: cores, hybrid layout and SIMD
features are read reliably from the OS on all three systems. A unit test checks that
the file parses, patterns are lower-case, PCI IDs are unique and well formed, and
integrated GPUs have no bandwidth of their own.

GPUs missing from the catalogue get bandwidth and FLOPS guessed from the VRAM size
and are marked `estimated`. Every external tool is optional: a missing `ioreg`, KFD
or NVML only lowers precision and is logged at `debug`.

Considered and rejected: a Vulkan instance through `ash` (unsafe FFI, slow driver
start-up, no core counts on Intel/NVIDIA anyway) and parsing `vulkaninfo`/`lspci`
(not installed by default). DXGI gives no core counts, so Windows AMD/Intel rely on
the catalogue. The catalogue also marks integrated graphics (Intel UHD/Iris/Arc-iGPU,
AMD 680M/780M/890M/Vega) more reliably than the name heuristics of ADR 0006.

`HardwareProfile.probe_version` (now 2) makes the app re-probe profiles saved by
older builds, which lack the new fields.

### Speed model (`speed.rs`)

A small roofline model with calibrated efficiency factors:

- **Chat**: tokens/s = effective bandwidth / active weight bytes (MoE: × active/total
  × 2.4 overhead for shared layers and routing, measured; with RAM offload the times of both pools add up). Prompt reading
  is compute-bound: tokens/s = effective FLOPS / (2 × active parameters), judged for a
  500-token message (the system prompt and tools are cached between turns).
- **Transcription**: seconds per audio second = 0.66 TFLOP × parameters (billions) /
  effective TFLOPS, + 0.02 s decoding latency on GPUs. On the CPU whisper.cpp has its
  own per-core rate (160 GFLOPS with AVX2/NEON/AVX-512, 50 without): unlike llama.cpp
  prompt reading it gains nothing from AVX-512 (i5-11400F ≈150, Ryzen 7430U ≈180).
- **Images**: seconds per 1024² image = 51 TFLOP × parameters / effective TFLOPS.
  Without a usable GPU always red with reason `needs_gpu`.
- **Narration** (recalibrated in M4, ADR 0018): Piper always runs on the CPU and is
  compute-bound (1.33 TFLOP × parameters per audio second against the Whisper CPU
  throughput). Autoregressive TTS (Chatterbox) reads ~40% of its weights 50 times per
  audio second, plus 3.4 TFLOP of vocoder work, plus 0.74 s of token-loop latency per
  audio second on a GPU (RTX 3090 ≈ 1.05×, i5-11400F CPU ≈ 0.17×, both measured).

Parameters come from the catalog (`params_b`, `active_params_b`) or from the file
size and the quantisation in the variant id (`q4_k_m` 4.85 bits, `q8_0` 8.5, `f16`, …).

Effective throughput (`SpeedBasis`, part of `MemoryBudget`):

| Device | Bandwidth | Compute |
|---|---|---|
| NVIDIA (CUDA) | 0.45 × peak | 1.5 × FP32 peak (tensor cores), 0.7 for GTX |
| AMD/Intel (Vulkan) | 0.40 × peak | 0.6 × FP32 peak |
| Apple (Metal) | 0.62 × peak | 0.62 × FP32 peak |
| CPU | min(9 GB/s × cores, 36 GB/s); 4 GB/s per core without AVX2/NEON; Apple: 0.6 × chip bandwidth | prompt: 140 GFLOPS/core AVX-512, 60 AVX2/NEON, 20 otherwise; Whisper: 160 / 50 |

Cores = P-cores + ½ E-cores (else physical cores). Integrated graphics are not used
for speed either: measured on the HD 620 mini PC, llama.cpp Vulkan generates *slower*
on the iGPU (8.5 tok/s) than on its two CPU cores (16.2 tok/s).

### Calibration data

| Machine | Measurement | Model says |
|---|---|---|
| RTX 3090 (in-app, owner) | Bielik 11B Q6_K ≈ 46 tok/s; 30k-token prompt in 16 s | 46 tok/s; ≈2400–3000 tok/s prompt |
| RTX 3090 (llama-bench b12110) | same model tg64 76.5, pp512 3070 | (bench has no chat context; app number used) |
| RTX 3090 (M3 acceptance) | Whisper large-v3-turbo: 14 min audio in ≈26 s; Z-Image Turbo Q4_K 1024² in 10.6 s | ≈33× real time; ≈8.5–11 s |
| i5-11400F, 6 threads, DDR4-3200 | Bielik 11B Q6_K tg 4.0, pp256 38.2 tok/s; Whisper turbo q5_0 encoder 16.5 s per 30 s | 3.9 / 38 tok/s; 1.5× real time |
| i5-11400F, 2 threads | tg 2.07, pp 19.8 tok/s; Whisper encoder 39 s per 30 s | – (per-core constants) |
| i5-7300U, 2c/4t, DDR4-2133 | LFM3 1.7B Q4_K_M tg 16.2, pp512 39.8; Bielik 4.5B Q8_0 pp512 11.5 tok/s | 16.4 / 35; 13 tok/s |
| Ryzen 5 7430U, 6 threads, DDR4-2667 (Ubuntu 26.04, CPU build) | tg64 / pp512: LFM3 31.7 / 167, Bielik 4.5B Q8 7.31 / 38.8, Qwen3.5 9B Q4_K_M 6.49 / 36.0, Qwen3.6 35B-A3B UD-Q3_K_M 10.7 / 60.5 tok/s | 32.7 / 106, 7.1 / 40, 6.3 / 20, 10.6 / 60 |
| Ryzen 5 7430U, Whisper (whisper-bench, encode + ≈100 decoded tokens per 30 s) | turbo q5_0 13.6 s + 0.5 s → 2.1×; small q8_0 1.8 s + 0.9 s → 11×; base q8_0 0.44 s + 0.3 s → ≈40× | 1.7×; 5.9×; 19× |
| Ryzen 5 7430U, 2 threads | LFM3 tg 24.2, Bielik 4.5B tg 6.16 tok/s | 18, 3.6 (Zen 3 drives ≈14 GB/s per core; 9 kept for older dual-cores) |
| Apple M1 8c / M2 Max 38c (llama.cpp Apple Silicon benchmark, 7B Q4_0) | tg 14 / 61, pp 117 / 670 tok/s | Metal factors 0.62 |

### Thresholds (green / yellow / orange / red)

| Mode | Fast | Usable | Slow | Too slow |
|---|---|---|---|---|
| Chat: generation | ≥ 15 tok/s | ≥ 6 (≈ reading speed) | ≥ 2 | < 2 |
| Chat: 500-token prompt | ≤ 10 s | ≤ 30 s | ≤ 90 s | > 90 s |
| Transcription | ≥ 8× real time | ≥ 2× | ≥ 0.2× | < 0.2× |
| Images (1024²) | ≤ 20 s | ≤ 60 s | ≤ 240 s | > 240 s / no GPU |
| Narration | ≥ 2× real time | ≥ 0.8× | ≥ 0.25× | < 0.25× |

Chat takes the worse of its two rows. "Won't fit in memory" stays red.

Resulting examples: RTX 3090 – everything green. i5-7300U mini PC – LFM3 yellow,
Bielik 4.5B orange, Whisper turbo orange, Whisper base yellow, images red ("needs a
graphics card"). 2-core Celeron without AVX2 – chat orange at best, Whisper turbo red.
Ryzen 7840U (780M is integrated) – the 35B-A3B MoE model and Bielik 4.5B yellow,
the MoE one faster. Ryzen 5 7430U – LFM3 green, Bielik 4.5B and 9B yellow, Whisper
turbo on the yellow/orange border, base green.
Apple M2 – chat and Whisper yellow, Z-Image red (minutes per image).

### UI

One badge per model row (`SpeedBadge`) replaces the memory badge in Basic mode: the
level in plain words, numbers only in the tooltip; Expert mode adds the number
inline and keeps the memory badge in the variant table. The wizard summary shows the
best level per capability and a short note when chat is slow. The starter pack skips
red variants and ranks green/yellow above orange before the Polish/licence
preferences, so a weak computer gets e.g. LFM3 1.7B instead of Bielik 4.5B.

## Consequences

- Estimates are coarse (±50%); they rank models and colour badges, they are not
  promises. Constants live in `crates/hwprobe/src/speed.rs` with their sources and
  should be re-calibrated when engines change (e.g. Vulkan coopmat on RDNA, Metal
  whisper.cpp builds, sd.cpp step counts differing between image models).
- Not measured: AMD/Intel discrete GPUs, Apple Silicon (Metal factors from public
  llama.cpp results only; macOS is verified only in CI), Chatterbox speed, and the
  Vega 7 iGPU of the 7430U (that server has no Vulkan loader, the Vulkan build fell
  back to the CPU).
- The model is conservative for prompt reading with Q4_K on Zen 3 (≈1.6–1.8× faster
  than predicted) and for small Whisper models (decoder parameters counted as
  encoder work); both only err towards a lower colour.
- Found while measuring: the upstream llama.cpp and whisper.cpp Linux builds need
  `libgomp.so.1` (package `libgomp1`), which a minimal Ubuntu 26.04 server lacks;
  the binaries then do not start at all. Plug and play on Linux needs a check or a
  bundled copy (follow-up, engines crate).
- `catalog/hardware.json` needs new rows (and verified PCI IDs) for new GPUs; until then they are estimated from VRAM.
- `MemoryBudget` gained an optional `speed` field and `VariantView`/`StarterPick` an
  optional `speed`; old saved profiles are re-probed via `probe_version`.

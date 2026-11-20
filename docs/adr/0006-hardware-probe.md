# ADR 0006: Hardware probe via NVML and sysinfo; fit thresholds

- Status: accepted
- Date: 2026-11-11

## Context

PRD §5 lists NVML or `nvidia-smi` for NVIDIA, `system_profiler`/Metal for Apple and
Vulkan for AMD/Intel. Writing three platform paths (including a Vulkan instance just
to read heap sizes) is a lot of unsafe/FFI code for M1.

## Decision

- NVIDIA: `nvml-wrapper` (loads `nvml.dll`/`libnvidia-ml` at runtime, exact VRAM and
  driver version), falling back to `nvidia-smi --query-gpu=…`.
- Everything else (AMD, Intel, Apple) and RAM, CPU, disks: `sysinfo` 0.40, whose
  GPU list covers Windows, macOS and Linux. Apple GPUs are treated as unified memory
  (budget = RAM × configurable share, default 70%).
- GPUs with less than 2 GiB of dedicated memory (typical iGPUs) are ignored for budgeting.
- Vulkan is listed as an available backend whenever an NVIDIA/AMD/Intel GPU is found
  (not on macOS); the engine manager (M2) verifies the actual Vulkan loader.

Budgets: discrete GPU = VRAM − 512 MiB; offload pool = RAM − 4 GiB (at least half of
RAM); CPU-only = the same RAM pool. Labels follow PRD §5: *fits* ≥ 15% headroom,
*tight* 0–15%, *offload* fits in VRAM + offload pool, otherwise *won't run*.

Memory estimates (`catalog::estimate_memory_mb`) are derived from file sizes by
`catalog-sync`: LLM weights × 1.1 + 1 GiB (≈8k context KV cache), image × 1.2 + 1 GiB,
STT × 1.5 + 300 MiB, TTS × 1.3 + 500 MiB. They are deliberately coarse and will be
refined with measurements once engines run (M2/M3).

## Consequences

No Vulkan/Metal FFI in M1. AMD/Intel VRAM accuracy depends on what the OS reports
through `sysinfo`; if it proves wrong on real machines a Vulkan heap query can be added
behind the same `GpuInfo` type.

Update: ADR 0016 adds compute details (cores, clocks, bandwidth) and speed tiers on top of the memory fit; its hardware catalogue (`catalog/hardware.json`) also marks integrated graphics.

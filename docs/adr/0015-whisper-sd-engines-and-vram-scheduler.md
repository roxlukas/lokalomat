# ADR 0015: whisper.cpp and stable-diffusion.cpp engines, VRAM scheduler

- Status: accepted
- Date: 2026-11-11

## Context

M3 adds transcription (whisper.cpp) and image generation (stable-diffusion.cpp) next to
llama.cpp, plus the VRAM scheduler with LRU unloading (PRD §4.4, §13). The PRD sketches
both engines as servers. Upstream releases checked on 2026-11-11:

- **whisper.cpp** `v1.9.5` → build tag `b5454`. Assets: Windows x64 CPU, CUDA 12.4 and
  CUDA 11.8; Ubuntu x64 CPU (needs glibc 2.34). **No macOS command-line build** (only an
  iOS/macOS xcframework) and no Vulkan build. `whisper-server` has no API key option.
- **stable-diffusion.cpp** has no versioned releases, only `master-<n>-<sha>` builds; we pin
  `master-1088-32d516c`. Assets: Windows CUDA 12 (+ `cudart` archive), Vulkan, CPU; macOS
  universal (Metal, **needs macOS 26**); Linux Vulkan and CPU built on Ubuntu 26.04
  (**need glibc 2.43**, so they do not run on Ubuntu 24.04). `sd-cli` embeds the generation
  record (JSON + WebUI infotext) in every PNG.

## Decision

- **Command line per job, not servers**, for both engines (`whisper-cli`, `sd-cli`):
  - memory is released as soon as a job ends, which is what the scheduler wants for
    heavy, occasional jobs; nothing listens on a port, so no token is needed;
  - progress is parsed from the output (`progress = N%`, `step/steps`); cancelling kills
    the process; a crash only fails that job;
  - cost: the model loads for every job (whisper turbo ≈ 1 s, Z-Image ≈ 5–15 s). A resident
    `sd-server` mode can follow when repeated image jobs need it.
- Manifests `catalog/engines/whisper-cpp.json` and `sd-cpp.json`; sizes and sha256 come from
  GitHub asset digests via `catalog-sync`. Build choice reuses the llama.cpp rules
  (CUDA by driver → Metal → Vulkan → CPU). CUDA 12 builds need driver ≥ 528 (Windows minor
  version compatibility). New optional build fields `min_glibc` and `min_macos` hide builds
  the system cannot run (checked at runtime).
- **Platform gaps (follow-up for the owner):** transcription is not available on macOS until
  whisper.cpp ships a macOS binary or we build one in our CI; image generation needs macOS 26
  on Mac and Ubuntu 26.04 on Linux. The UI says "not available on this computer yet".
- Audio input: any common format (WAV, MP3, FLAC, OGG, M4A/AAC) is decoded in Rust
  (`symphonia`) to a mono 16-bit WAV that `whisper-cli` reads. Results are stored as
  segments; SRT/VTT/TXT/JSON are produced by the app (`lokalsi_core::transcript`).
- Images: `sd-cli` writes PNGs with its generation record; the app adds UTF-8 `iTXt` chunks
  `Software`, `Model` and `Licence` (PRD §11). Files go to `<data_dir>/outputs/YYYY-MM/`.
- **Jobs**: table `jobs` (migration 4) with kind, status, progress, params, outputs, error and
  times (PRD §8.1). Jobs left running by a killed app are marked failed at start. The Gallery
  (V3) lists finished jobs.
- **VRAM scheduler** (`lokalsi_engines::scheduler`): knows the memory budget, the resident
  model (llama.cpp) and leases of running jobs.
  - Before a job or a model load it makes room: least recently used resident models are
    unloaded, except the chat model when **"Keep the chat model in memory"** is on, and except
    a model that is generating right now.
  - Jobs that still do not fit wait for other jobs to finish; if nothing can be freed, the
    image job runs with `--offload-to-cpu` (slower, uses RAM).
  - Transcription on a CPU build takes no GPU memory, so it runs next to anything; jobs of the
    same engine run one at a time.
  - The header meter shows the scheduler's view (resident models + running jobs); "Release"
    unloads a model or cancels a job.

## Consequences

Switching from chat to image generation unloads the chat model (unless kept), and the image
engine leaves nothing behind, so memory is free again for the next mode. Engine updates are a
tag bump plus `catalog-sync`. The macOS/Linux gaps above stay open until upstream or our own CI
provides binaries.

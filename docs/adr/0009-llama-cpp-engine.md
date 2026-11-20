# ADR 0009: llama.cpp engine – builds, supervision and chat jobs

- Status: accepted
- Date: 2026-11-11

## Context

M2 needs the engine manager (V5), a llama.cpp adapter and streaming chat (PRD §4.3,
§8.1, §10). llama.cpp now publishes a stable release (`v0.9.0`) that points to a
nightly build tag (`b12110`) holding the binaries; GitHub reports a sha256 digest for
every release asset.

## Decision

- `catalog/engines/llama-cpp.json` pins `v0.9.0` / `b12110` and lists builds per
  platform and backend. `catalog-sync` fills `size` and `sha256` from the GitHub API
  (asset `digest`); a unit test fails the build when anything is unsynced.
- Build choice: CUDA 13.4 when the NVIDIA driver major version is ≥ 580, otherwise
  CUDA 12.4; then Metal, Vulkan, CPU. The 580 threshold is our assumption for CUDA 13
  driver compatibility – verify on a machine with an older driver. Windows CUDA builds
  include the matching `cudart` archive.
- Install: download → sha256 check → unpack into a staging folder (path traversal
  refused) → atomic rename to `<data_dir>/engines/llama-cpp/<tag>-<build>`.
  The binary is located after unpacking (macOS archives have a top-level folder).
- Supervision: `llama-server` binds 127.0.0.1 on a random free port and requires a
  random token (`--api-key`); stdout/stderr go to `logs/engine-llama-cpp.log` (the
  token is masked by `redact`). Defaults of build b12110 are used for GPU offload
  (`-ngl auto`, `--fit on`), plus `-c 8192 -np 1 --no-webui --reasoning-format deepseek`
  so reasoning arrives separately from the answer.
- One loaded model at a time; a chat for another model restarts the server with it
  (the VRAM scheduler with LRU arrives in M3). Generations are serialised (one slot).
- A chat job is the assistant message row (status, error code, tokens/s). Cancelling
  drops the HTTP stream, which stops generation in `llama-server`; the partial text is
  kept. A crash marks the job failed with `engine_crashed`, the engine state becomes
  `crashed`, and the next request starts it again; the app keeps running. Jobs left
  running by a killed app are marked failed at the next start.
- Model output is rendered as Markdown and sanitised with DOMPurify; links open in the
  system browser.

## Consequences

Updating llama.cpp = bump the tag in the manifest, run `catalog-sync`, release.
Context size and sampling parameters are fixed for now; they move to the Inspector later.
On tight cards the KV cache is compact (flash attention + q8_0), see ADR 0021.

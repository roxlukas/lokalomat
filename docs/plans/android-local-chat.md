# Plan: local chat on the Android phone (MediaPipe / LiteRT-LM)

Status: steps 1–3 done (spike, catalog and download, UI incl. several computers, settings and welcome screen); step 4 open: PR, more measurements (Gemma 4 E4B, clean Qwen run) (ADR 0038). Date: 2026-11-24. Will become ADR 0038 when step 1 is decided (0037 is taken by phone parity part 2).

## Goal

The phone runs a chat model (max ~4B parameters) on its own, with no computer.
At the top left of the phone UI there is a source dropdown: **Ten telefon** (local)
or a paired computer (now: Niagara). A **+** next to it appears when no computer is
paired and starts pairing (QR).

## Decisions so far

- Engine: **LiteRT-LM** preferred. The MediaPipe LLM Inference API (`com.google.mediapipe:tasks-genai`)
  is deprecated on Android (maintenance only). Keep the engine behind one small Kotlin
  interface so it can be swapped. Exact artifact coordinates and version: check the LiteRT-LM
  GitHub releases in step 1 (not verified yet).
- Models: `.litertlm` from `litert-community` on Hugging Face (Apache-2.0: Gemma 4 E2B / E4B,
  Qwen3 4B, Qwen3.5 4B, Phi-4-mini). **Default for 8 GB phones: Gemma 4 E2B** (spike result below).
  4B only as an option for phones with more RAM.
- Local mode is chat only, no tools, no MCP. STT, TTS, image, music and Decision need a computer.
- Target: arm64-v8a, devices with 8 GB RAM or more (Pixel 8a is borderline). Context 2–4k tokens.
- Pairing, TLS pinning and the gateway client stay as they are (ADR 0020).

## Spike result (2026-11-24, Pixel 8a, 7.5 GB RAM, Android 17)

Branch `feat/mobile-local-llm`; plugin `crates/tauri-plugin-localllm`, LiteRT-LM 0.20.0, Kotlin 2.4.0.
One prompt ("what is photosynthesis", Polish), no long context. A "chunk" is what the library
returns per step, close to a token.

| Model | Backend | Load | Speed | Memory |
|---|---|---|---|---|
| Gemma 4 E2B `gemma-4-E2B-it.litertlm` (2.59 GB) | CPU | 4.8 s | ~14 chunks/s, first token 1.0 s | PSS 1.6 GB |
| Gemma 4 E2B (same file) | GPU | 30.8 s (first load) | ~10.7 chunks/s, first token 0.9 s | PSS 2.5 GB |
| Qwen3 4B `qwen3_4b_mixed_int4.litertlm` (2.27 GB) | CPU | 16.6 s | 83 chunks in 170 s (~0.5/s) | PSS 1.8 GB |

- Gemma 4 E2B answers well in Polish. On GPU it once added LaTeX (`$	ext{CO}_2$`): the UI must not show raw LaTeX.
- Qwen3 4B ran under memory pressure (21 low-memory kills, swap nearly full), so its speed is a
  worst case, not a clean measurement. A second run was not finished. Not tested: Gemma 4 E4B,
  Qwen after a reboot with other apps closed, the `-gpu` file variant of Gemma.
- GPU is slower than CPU on the Tensor G3 with the generic file; start with CPU.
- Build notes: LiteRT-LM 0.20 needs Kotlin 2.4 (project was on 2.2.10; `buildSrc` also pins it);
  `kotlin.incremental=false` is needed because Cargo sources are on C: and the project on E:.
  Test models go to the app's private dir with `adb push` + `run-as … cp`.
- Decision: go ahead with a CPU Gemma 4 E2B default; step 2 (catalog and download) is next.

## Steps

### 1. Spike (own branch, needs the phone over adb)
- Kotlin Tauri mobile plugin (`tauri-plugin-localllm` or a module in `apps/mobile`):
  `load(path)`, `generate(prompt)` streaming tokens through a `Channel`, `cancel`, `unload`.
- Gradle: dependency in `apps/mobile/src-tauri/gen/android/app/build.gradle.kts`; check the
  library `minSdk` (now 26) and `jvmTarget` (now 1.8, may need 11/17). arm64 only.
- Manifest: `android:largeHeap="true"`; `<uses-native-library android:name="libOpenCL.so" android:required="false"/>` for GPU.
- One hard-coded model pushed with adb to the app's private dir.
- **Measure**: RAM peak, load time, tok/s on CPU and GPU, Polish answer quality for Gemma 3n E2B and one 4B model.
- Done when: a prompt typed in a debug screen streams an answer on the Pixel 8a, numbers written down.
- Go/no-go: if GPU/CPU speed or RAM is unusable, stop and write ADR 0038 as "rejected".

### 2. Models: catalog and download
- Catalog entries for phone models (new section or file next to `catalog/models.json`);
  sizes, sha256 and revision from the Hugging Face API only – never invented (`catalog-sync`).
- Download in Rust: resume, progress, sha256 check, free-space check, delete model.
  Reuse the download code of `lokalsi_core` where it fits.
- Device check: read total RAM, hide or warn about models that will not fit.

### 3. UI: source dropdown (ui/src/mobile)
- `MobileApp.svelte`: the app always starts on the main screen. `PairScreen` becomes a sheet
  opened by **+** (today it is a full-screen gate at the `!status.paired` branch).
- Replace the `.who` block in the header with the dropdown: *Ten telefon* + paired computers.
  Online/offline icon and "Odłącz" move to the computer entry.
- **+** shown only when no computer is paired (the store holds one computer; multi-computer is out of scope).
- State `source: 'local' | 'computer'` (persisted); `api.ts` routes `chat` to the plugin or the gateway.
- With *Ten telefon*: only the Chat tab; other modes hidden with the hint "Wymaga komputera".
- First start without a computer and without a model: Chat screen with a "Pobierz model" step.
- Texts short and plain, `t()` keys in both `pl.json` and `en.json`; icons Phosphor; tokens only.

### 4. Quality and docs
- ADR 0038 (engine choice, deviation from "thin client only", model formats, limits).
- Tests: vitest for source routing and tab visibility; Rust tests for download/sha256;
  logs through `tracing` without prompt contents (`loggable_content`).
- `npm test`, `npm run check`, cargo test/clippy/fmt, `sh scripts/quality-gate.sh pr`.
- Manual check on the Pixel 8a (build flags: see memory "Android phone testing").

## Risks

- RAM: 4B int4 is ~2.5–3.5 GB file and ~4–5 GB at run time; Android may kill the app.
- LiteRT-LM API and packaging still moving; pin the version and record it in the ADR.
- Model licences (Gemma terms) – check before offering a download in the app.
- Download size 2–4 GB on mobile data: ask before downloading, Wi-Fi hint.

## Open questions

- Keep **+** also when one computer is already paired? (assumed: no)
- Which model is the default on the phone? (decided by step 1 numbers)

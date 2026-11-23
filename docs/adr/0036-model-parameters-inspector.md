# ADR 0036: Model parameters in the Inspector and a choice of send key

- Status: accepted (owner, 2026-11-23)
- Date: 2026-11-23

## Context

ADR 0009 kept context size and sampling fixed and said they move to the Inspector later.
The Inspector (`ui/src/lib/components/Inspector.svelte`) is still empty. The chat always
sends `ChatParams::default()`, `llama-server` starts without `-ngl`, and the Pro image and
speech knobs sit in an "advanced" block inside each Workshop panel (Expert only). The
composer sends with Ctrl/⌘+Enter only (PRD V2).

## Decision

1. **Send key (Settings, Free, Basic and Expert).** `AppSettings.send_key` is `ctrl_enter`
   (default, as before: Enter adds a new line, Ctrl/⌘+Enter sends) or `enter` (Enter sends,
   Shift+Enter adds a new line). Ctrl/⌘+Enter sends in both schemes. A key press while an
   input method is composing (`isComposing`) never sends. The phone keeps its keyboard's
   Enter as a new line. Long-text fields that are not a chat prompt (song lyrics in Pro)
   keep Ctrl/⌘+Enter.
2. **Inspector frame.** The Inspector shows sections for the active view and Workshop mode.
   Editions register them in `ui/src/lib/edition/registry.ts`
   (`registerInspectorSection({ mode, load })`, like `registerWorkshopMode`): Free registers
   chat (`views/workshop/ChatInspector.svelte`), Pro registers image and audio. Every knob
   has "Auto" (`null` = the model's or engine's own default) and a reset.
3. **Chat parameters are defaults of the model** (not of a conversation): a new table
   `model_settings(model_id, variant_id, params JSON)` holds `temperature`, `top_p`,
   `top_k` and `gpu_layers` (`lokalsi_core::model_params`); a missing row or a `null`
   field means Auto. `EngineService` fills them into every chat and completion
   (`ChatParams::with_model_defaults`, values set by the caller win), so the desktop chat
   and the phone's chat through the gateway use them; the local OpenAI-compatible server keeps the
   client's own parameters.
   - Basic: one control "response style" — precise / balanced (Auto) / creative
     (`temperature` 0.2 / Auto / 1.0).
   - Expert: temperature 0–2, top_p 0–1, top_k 0–200, each with Auto.
4. **GPU offload (Expert only).** `gpu_layers`: Auto (no `-ngl`, as today), 0 (CPU only),
   1…N, or all (stored as 999, which llama.cpp caps at the model's layers, output layer
   included); N is `<arch>.block_count` read from the GGUF header (`lokalsi_core::gguf`).
   It becomes `-ngl N`; changing it reloads the model (the loaded value is kept inside
   `EngineService`, not on `LoadedModel`, which other crates build), and the VRAM scheduler
   scales the memory estimate by the offloaded share. It applies to every load of the model,
   the local OpenAI-compatible server included. When the file does not give N, the
   Inspector offers only "all" and "processor only".
5. **Image (Pro).** Steps, seed and CFG move from `ImagePanel` to the Inspector. Basic:
   quality (fast / detailed, a steps preset) and "repeat the same image" (fixed seed).
   Expert: steps 1–150, CFG 0–30, seed, negative prompt.
6. **Audio (Pro).** TTS — Basic: voice, language, speed; Expert: exaggeration, cfg_weight,
   temperature, seed. STT — Basic: language; Expert: translate to English (beam size and VAD
   later). Music — Basic: duration; Expert: seed (steps and guidance if the model exposes
   them).

## Order

1. Send key (done, #32).
2. Inspector frame and seam, chat parameters per model (done).
3. GPU offload (done).
4. Pro image and audio sections, after the submodule bump in `lokalomat-pro`.

## Consequences

- ADR 0009's "fixed sampling" no longer holds once step 2 lands.
- Parameters per model mean two conversations with the same model share them; a per-chat
  override can be added later without changing the table.

## Acceptance on real hardware (2026-11-23)

Example `crates/engines/examples/gpu_offload.rs` (`gpu_offload <work dir> <model.gguf> <estimate MB>`):
for each GPU setting it reloads the model, prints GPU memory (nvidia-smi) and tok/s, then
checks that temperature 0 repeats the answer, a high temperature does not, and top_k 1
repeats again. Result: **0 checks failed** with LFM3 on an RTX 3090 and on the 8 GB card below.

8 GB card: Windows 10, RTX 2070 8 GB, 16 GB RAM; llama.cpp `b12110-win-cuda12`
(CUDA 12, chosen automatically and installed on the first start). Baseline GPU memory 145 MiB.

LFM3-1.7B Q4_K_M (24 layers), `TOKENS=96`:

| GPU setting | GPU MiB | tok/s |
|---|---|---|
| Auto | 1472 | 141.0 |
| Processor only (0) | 302 | 4.1 |
| Half (12 of 24) | 1210 | 6.9 |
| All (999) | 1472 | 141.8 |

Qwen3.5-9B Q4_K_M (32 layers, estimate 6984 MB, budget 7680 MB, `TOKENS=48`; the memory
check only: that run came before the example read `reasoning`, so its answers were
empty at that token limit):

| GPU setting | GPU MiB | tok/s |
|---|---|---|
| Auto | 5428 | 50.3 |
| Processor only (0) | 382 | 0.9 |
| Half (16 of 32) | 3182 | 1.7 |
| All (999) | 5428 | 53.3 |

Findings:

- `-ngl` reaches `llama-server`: GPU memory grows with the setting (302 < 1210 < 1472 MiB
  for LFM3, 382 < 3182 < 5428 MiB for Qwen).
- Auto equals "all layers" in memory and speed on both models: both fit in 8 GB.
- Partial offload does not scale linearly: half the layers is close to processor only
  (LFM3 6.9 vs 4.1 tok/s, Qwen 1.7 vs 0.9) and far below all layers (141.8 and 53.3).
  Expert users should pick "all" or "processor only" unless the model does not fit.
- LFM3 and Qwen3.5 are reasoning models: with `--reasoning-format deepseek` the text arrives
  in `reasoning_content` and `content` stays empty until thinking ends. Checks that read
  an answer must compare `reasoning` + `content`.
- llama.cpp `b12110` no longer prints "offloaded N/M layers to GPU" at default verbosity,
  so the example checks GPU memory instead of that log line (partial offload only leaves
  `layer N is assigned to device CPU` as a warning).

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

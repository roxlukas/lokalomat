# ADR 0038: Chat on the phone itself (LiteRT-LM)

- Status: accepted
- Date: 2026-11-24

## Context

Until now the Android client (ADR 0020) only drives a paired computer. The owner
wants a chat that also works on the phone alone, with models of at most ~4B
parameters. The plan and the spike numbers are in
`docs/plans/android-local-chat.md`.

The MediaPipe LLM Inference API is deprecated on Android (maintenance only);
Google points to LiteRT-LM. Its models are `.litertlm` files, not GGUF, so the
llama.cpp engine of the computer is not used on the phone.

## Decision

- **Engine:** LiteRT-LM (`com.google.ai.edge.litertlm:litertlm-android` 0.20.0),
  wrapped by the Tauri plugin `crates/tauri-plugin-localllm` (Kotlin part
  `LocalLlmPlugin`: `status`, `load`, `generate` streaming over a `Channel`,
  `cancel`, `unload`). Prompt and answer texts are never logged.
- **Models:** the separate catalog `catalog/phone-models.json`, read by the new
  crate `lokalsi-phone-models` (catalog, resumable download with sha256, storage,
  RAM check). It does not depend on `lokalsi-core`, so the phone build gets no
  database or credential store. Sizes, sha256 and revisions are filled by
  `cargo run -p catalog-sync` (same rule as the desktop catalog); `--check` now
  also reports unsynced phone models.
- **Default model:** Gemma 4 E2B on the **CPU** (Apache-2.0). On the Pixel 8a
  (7.5 GB RAM) it loads in 4.8 s, writes ~14 chunks/s and uses ~1.6 GB. The GPU
  backend was slower (~10.7 chunks/s, 31 s first load, 2.5 GB), so it is not used
  by default.
- **4B models:** Gemma 4 E4B is in the catalog for phones with more memory
  (`min_ram_mb` 10000 – an **estimate, not measured**; Gemma 4 E2B uses 5000, a
  reasoned estimate from the 1.6 GB it used on a phone that reports 7.5 GB, kept low
  enough that phones sold as "6 GB", which report about 5.6 GB, are not warned). Qwen3 4B was too slow on
  the 8 GB phone (0.5 chunks/s, under memory pressure) and is not offered.
  `min_ram_mb` is compared with the phone's total RAM; a phone below it gets
  `fit: low` and the UI warns instead of hiding the model.
- **Local mode is chat only:** no tools, no MCP; the other modes need a computer.
- **TLS for the download:** `lokalsi-phone-models` uses reqwest with rustls (`ring`
  provider) and the bundled Mozilla roots (`webpki-roots`) through
  `use_preconfigured_tls`. The workspace reqwest uses native-tls, which pulls in
  `openssl-sys` – not buildable for Android – and the platform verifier needs JNI
  set-up. Checked with a live range request to Hugging Face
  (`cargo test -p lokalsi-phone-models -- --ignored`) and an Android build.
- Commands in `apps/mobile` (`phone_llm.rs`): `phone_models`, `phone_model_download`
  (progress over a `Channel`), `phone_model_cancel`, `phone_model_delete`,
  `phone_model_path`. Errors use codes translated by the UI (`download_space`,
  `download_checksum`, …).

## Consequences

- LiteRT-LM 0.17+ is built with Kotlin metadata 2.4, so the Android project moved
  from Kotlin 2.2.10 to 2.4.0 (`gen/android` build files and `buildSrc`).
- Kotlin's incremental compilation is off (`kotlin.incremental=false`): Cargo
  sources are on a different drive than the project and the incremental caches fail.
- The app's size does not change much (the native runtime is a few tens of MB);
  models are downloaded on demand into `<app data>/phone-models/<id>/`.
- Not measured yet: Gemma 4 E4B, Qwen3 4B on a clean phone, long contexts and
  several turns, the `-gpu` file variant of Gemma. The spike screen
  (`LlmSpike.svelte`, built only with `VITE_LLM_SPIKE=1`) is removed when the real
  chat UI lands.
- The phone UI (done): a source dropdown in the header (This phone and every paired
  computer), a settings menu (gear) with the computers (add, forget) and "Manage
  models", a welcome screen with two tiles on the first start ("Run on this phone" /
  "Pair with a computer"), and model cards with download, resume, use and delete.
- The phone now keeps **several computers** (`pairing.json`: `computers` + `active`,
  the certificate fingerprint is the id); an older file with one computer migrates
  on load. The tile says "Pair with a computer", not "Pro": Pro texts do not belong in
  this repo (ADR 0035).

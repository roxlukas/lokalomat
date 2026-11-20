# ADR 0020: Pipelines, minimal LAN gateway and the Android client (M5)

- Status: accepted
- Date: 2026-11-12

## Context

M5 (PRD §13) brings pipelines (§8.2, V7), the local API for phones (§4.2,
§15 item 12) and the Android client (V9). PRD §15 (note under item 11) narrows
the API in M5 to what the phone needs; the full OpenAI-compatible server panel
is M5c. Library versions were checked on crates.io/npm on 2026-11-12.

## Decision

### Pipelines

- Definitions, type validation, the three templates and storage live in
  `lokalsi_core::pipelines` (migration 7: `pipelines`, `pipeline_runs`).
  Ports: `text`, `audio`, `image`, `subtitles`; subtitles feed text steps.
- A step reads the pipeline input (`from: 0`) or any earlier step (`from: n`,
  default = previous). This keeps pipelines linear (no branches, PRD §8.2) yet
  lets template 3 ("Tekst → ilustracja + narracja") use the input twice.
- `lokalsi_engines::modes::ModeRunner` runs one mode to completion; media steps
  are ordinary jobs (queue, VRAM scheduler, Gallery) tagged with
  `inputs.source = {pipeline_run, step}`; chat steps use the new one-shot
  `EngineService::complete` (no session, no tools).
- `lokalsi_engines::pipeline::PipelineRunner` stores every step change and
  publishes `pipelines://run`; cancelling cancels the running job.
- The UI mirrors the validation in `ui/src/lib/pipelines.ts` for live red
  arrows; the backend validates again before a run.
- Template 2 translates Polish → English (UI in English: English → Polish) and
  reads it with the default TTS model in the target language.

### Gateway (`crates/api-gateway`)

- axum 0.8.9 routes served by hyper 1 over tokio-rustls 0.26.6 / rustls 0.23.46
  with the **ring** provider only (no aws-lc toolchain, same reason as the
  native-tls choice for reqwest). Self-signed certificate from rcgen 0.14.10
  (ECDSA P-256), key in the OS credential store, certificate DER in
  `<home>/gateway/cert.der`. QR matrix from qrcode 0.14.1, drawn by the UI with
  design tokens (`--qr-dark`/`--qr-light`).
- The gateway listens **only when LAN sharing is on** (`lan_enabled`, default
  off; port `lan_port`, default 47821) on `0.0.0.0` with TLS. There is no
  plain-HTTP listener in M5; binding to `127.0.0.1` stays possible
  (`GatewayConfig.bind`, used by tests) for the M5c server.
- Pairing exactly as PRD §15 item 12: QR = `lokalomat://pair?h&p&f&t&n` (host,
  port, SHA-256 certificate fingerprint, 128-bit one-time token valid 120 s,
  computer name). The phone pins the fingerprint, sends its Ed25519 public key
  with a signature over the token + fingerprint; both sides show
  `comparison_code = base32(SHA-256("lokalomat-pair-v1" ‖ fingerprint ‖ phone key ‖ token))[:30 bits]`
  as `ABC-DEF`. `POST /pair` waits (≤ 120 s) for "Zgadza się" / "Nie zgadza się"
  on the computer; a mismatch, a new QR code or a timeout aborts and burns the
  token. Five wrong tokens cancel the pairing.
- After confirmation the computer issues a random 256-bit access key per device,
  stored in the OS credential store (`device:<id>`); the database keeps only the
  device list (`paired_devices`). Requests carry
  `Authorization: Bearer <device id>.<key>`; keys are compared as SHA-256 in
  constant time. "Odłącz" deletes the key and the row; it stops working at once.
- Every request with an `Origin` header is refused (browsers, DNS rebinding);
  every `/v1/*` request needs a device key.
- Routes (OpenAI shapes where they exist, so M5c can extend them):
  `POST /pair`, `GET /v1/info`, `POST /v1/chat/completions` (non-streaming),
  `POST /v1/audio/transcriptions` (raw body, ≤ 200 MB), `POST /v1/audio/speech`
  (WAV), `POST /v1/images/generations` (`b64_json`), `GET /v1/voices`,
  `GET /v1/gallery`, `GET /v1/gallery/{job}/files/{n}` (only files under `outputs`).
- Each phone request runs in its own task with a cancel token; a phone that
  disconnects cancels its job.
- Added after the first phone test (2026-11-14): `GET /v1/models?mode=` lists
  the installed models of a mode (`ModelResolver::installed`, default marked)
  and chat/transcription/speech/image take an optional `model`
  (`<model>:<variant>`); `GET /v1/gallery/{job}/files/{n}/thumb` returns a
  384 px JPEG made once and cached under `<data>/cache/thumbs` (crate `image`
  0.25.10, PNG + JPEG only). Results stay private without signed links: every
  `/v1` route needs the device key, the phone fetches files in its Rust layer
  and hands the page object URLs (the key never reaches page code), answers
  carry `Cache-Control: no-store`, and a device may send at most 600 requests
  a minute (`429 rate_limited`).

### Android client (`apps/mobile`)

- Tauri 2 app (`pl.lokalsi.mobile`) whose UI is the second Vite entry
  `ui/mobile.html` (same tokens, i18n and markdown sanitising as the desktop).
  Rust side is thin; pairing, TLS pinning and calls are
  `lokalsi_api_gateway::client` (feature `client`, no axum/engines).
- QR scanning: `tauri-plugin-barcode-scanner` 2.5.3 (mobile targets only);
  a pasted link works too (Expert "Kopiuj link" on the computer).
- **Deviation:** on Android the identity seed and the access key are kept in
  the app's private storage (`pairing.json` in the app data dir, readable only
  by the app), not in the Android Keystore – there is no Keystore binding in the
  Tauri ecosystem we could verify for this release. Desktop builds of the client
  use the OS credential store. Keystore encryption is a follow-up before 1.0.
- Recording uses `MediaRecorder` in the WebView (`RECORD_AUDIO` in the manifest).
  The WebView records WebM/Opus, which symphonia on the computer cannot decode,
  so the phone decodes it with Web Audio and sends WAV (as the desktop voice
  recorder does). Picked files (M4A, MP3, WAV…) are sent as they are.
- Android 17 (API 37) blocks connections to LAN addresses unless the user
  allows "devices on the local network" (`ACCESS_LOCAL_NETWORK`, runtime). The
  manifest declares it and `MainActivity` asks for it at start; a refused
  connect (`PermissionDenied`) maps to its own error code `local_network`, so
  the phone does not wrongly say the computer is on another network.
  Checked on a Pixel 8a (Android 17), 2026-11-14.

## Consequences

- `CmdError.code` became `Cow<'static, str>` so mode errors keep their codes.
- `AppSettings` gained `lan_enabled`, `lan_port` (serde defaults keep old files valid).
- The VRAM scheduler only knows memory used by this app; another program on the
  same GPU (e.g. LM Studio) can push CUDA into slow shared memory. The acceptance
  run used `BUDGET_MB`; reading free VRAM from NVML is a follow-up.
- Windows asks for a firewall exception the first time LAN sharing starts.

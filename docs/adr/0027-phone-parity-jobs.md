# ADR 0027: Phone parity – images, music and decisions as jobs

- Status: proposed (owner tests on the Pixel 8a)
- Date: 2026-11-17

Numbering: 0026 is taken by the M5c agents branch (`docs/adr/0026-agents.md`); this is
the next free number on 2026-11-17.

## Context

PRD V9 asks for the phone to offer "tryby jak na desktopie", run on the paired computer.
Since M5 (ADR 0020) the desktop gained music (M5d, ADR 0022) and decision models
(M5b, ADR 0024). The phone had chat, transcription, speech, images and the gallery.

The M5 routes are one request per result. `POST /v1/images/generations` keeps the
connection open until the picture is ready and cancels the job when the phone
disconnects. That fits a short chat answer, not a song that takes minutes. Android drops
the connection when the screen locks or the app goes to the background, so long work
would be lost.

## Decision

1. **Jobs the phone starts and polls** (gateway, same TLS, per-device key, rate limit and
   `Cache-Control: no-store` as every `/v1/*` route):
   - `POST /v1/jobs/image` (body as `/v1/images/generations`),
     `POST /v1/jobs/music` (`caption`, `lyrics`, `language`, `duration`, `model`),
     `POST /v1/jobs/decision` (`state`, `questions` in the `lokalsi_core::decisions`
     JSON, `model`) → a `JobView`;
   - `GET /v1/jobs/{id}` → status, progress, error code, then the files (index and
     content type) or the decision answers with every probability;
   - `POST /v1/jobs/{id}/cancel`;
   - files download through the existing `GET /v1/gallery/{id}/files/{index}`; songs now
     list their WAV and MP3 there (the phone plays and saves the MP3).

   A job runs on its own (`ModeRunner::create_image` / `create_music` /
   `create_decision` + `MediaService::run_*`), like a Workshop job. It is a normal
   Gallery entry, tagged `{"phone": true, "device": "<id>"}`. **Only the phone that
   started a job can see or cancel it**: another device gets `404`. The phone polls every
   1.5 s while the app is on screen, every 5 s while the computer is away, and not at all
   in the background. The job id is kept on the phone, so a screen picks its job up again
   after a tab switch or an app restart. The old synchronous image route stays for older
   phone builds.
2. **Decisions follow the computer's Basic/Expert mode.** The phone has no Basic/Expert
   setting of its own. The gateway lists `decision` in `/v1/info` and `/v1/models` only
   while the desktop is in Expert mode. `POST /v1/jobs/decision` answers
   `403 expert_only` otherwise. The phone shows the Decisions tab only when the computer
   offers it. One switch on the computer controls both screens, and Basic users see no
   new tab. A phone-side switch would duplicate the setting and drift from it.
3. **Saving a result** uses the system save dialog (`tauri-plugin-dialog`) and writes the
   picked file through `tauri-plugin-fs`. No storage permission is needed (Android's
   document picker grants access to the one file). The file is fetched by the phone's
   Rust side, so the device key never reaches the web view.
4. **Same limits as the Workshop**: song style ≤ 2,000 characters, lyrics ≤ 10,000,
   length 10–600 s; decisions are validated by `lokalsi_core::decisions::validate`.
   Prompts and lyrics are never logged, only their lengths.

## Consequences

- The phone does not need a long-lived connection; a locked screen does not cancel work.
- The phone UI reuses the desktop's decision editor (`DecisionQuestions.svelte`) and
  probability bars; strings come from the existing `workshop.*`, `jobs.*` and
  `decisions.*` keys plus a few `mobile.*` ones.
- `tauri-plugin-dialog` and `tauri-plugin-fs` are new dependencies of the phone app
  (both were already in the lockfile through the desktop app).
- Not done: push notifications when a job ends in the background (needs a foreground
  service or FCM; the phone checks again when it is opened).

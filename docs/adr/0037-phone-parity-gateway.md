# ADR 0037: Phone parity, part 2 – the Workshop's settings through the gateway

- Status: accepted (owner asked for it on 2026-11-24; tested on the Pixel 8a)
- Date: 2026-11-24

## Context

ADR 0027 brought images, music and decisions to the phone as jobs. Since then the PC
Workshop gained settings per mode (ADR 0036). The phone could not send them:

- images: count, negative prompt, seed, steps and CFG;
- songs: seed;
- speech: language and tempo;
- the Gallery: "use again" and delete.

Speech was still one blocking request, which a locked screen cancels. The phone also did
not know whether the computer is in Expert mode, so it could not show Expert settings the
same way. An audit on 2026-11-24 listed these gaps. Each one needs a gateway change and
a phone change.

## Decision

Part 1 (this ADR). All new fields are optional with serde defaults, so old phones and old
computers keep working:

1. **`GET /v1/info`** adds `expert` (the computer's Expert mode, `Backend::expert`).
   - The phone shows Expert-only settings when this is `true`, the same rule as on the PC
     (ADR 0013).
   - The computer still applies only what it receives.
2. **Images.** `ImageRequest` adds `negative_prompt`, `count` (1–4), `seed`, `steps`
   (1–150) and `cfg_scale` (0–30).
   - `MediaModes::create_image_with` takes them as `ImageOptions` and clamps them.
   - Missing values are the Workshop's defaults (random seed, model defaults).
3. **Songs.** `MusicJobRequest` adds `seed`. It is stored as the Workshop stores it: an
   f32 setting, kept exact below 2^24.
4. **Speech as a job.** `POST /v1/jobs/speech` takes a `SpeechRequest` and returns a
   `JobView` of kind `tts`; the WAV comes through the gallery file route.
   - `SpeechRequest` adds `speed`, `exaggeration`, `cfg_weight` and `temperature`, with
     the Workshop's ranges.
   - `MediaModes::speak` is split into `create_speech` and the run, as images and songs
     already were.
   - `POST /v1/audio/speech` stays for older phone builds.
5. **Gallery.**
   - `GalleryItem.params` carries what "use again" needs: the job's stored settings, plus
     the full spoken text or lyrics (`text` is cut to 500 characters) and the `model` id.
   - `DELETE /v1/gallery/{job_id}` deletes a finished media result and its files through
     `JobQueue::delete`, the same as on the PC. It answers `404` for anything the gallery
     does not show (running jobs, chats, decisions).
   - Any paired phone may delete, just as any paired phone sees the whole gallery. The
     phone asks for confirmation; the computer logs the device and job.
6. **Phone.**
   - Image: count chips. In Expert mode, a negative prompt under the prompt and
     seed/steps/CFG behind "Więcej ustawień"; all result images are shown.
   - Voice: a job with language and tempo.
   - Music: seed (Expert).
   - Gallery: "Użyj ponownie" opens the mode's screen with the settings; "Usuń" asks
     first.
   - The Tauri commands `start_image`, `start_music` and `start_speech` take the
     request object as it is, so new fields need no new parameters.

Part 2 (later, not decided here):
- chat sessions and history on the phone;
- the chat's response style and ADR 0036 parameters;
- streaming answers with a stop button;
- a jobs list;
- pipelines for the device key.

## Consequences

- The phone produces the same results as the Workshop for the same settings and seed.
- Speech survives a locked screen.
- The gallery list is larger: up to 100 items with their settings and full texts.
- The free crate (`lokalsi-api-gateway`) changes its wire types; Pro implements the new
  `MediaBackend` methods. The free code must reach the public repository before Pro's
  `vendor/lokalomat` can point at it (ADR 0035).

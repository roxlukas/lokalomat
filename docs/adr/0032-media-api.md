# ADR 0032: Images, speech and transcription in the local API (M7, part 1)

- Status: accepted (owner, 2026-11-18: MCP over HTTP plus a small stdio bridge; the
  user allows each new client once; all modes are tools; three PRs).
- Date: 2026-11-18

## Context

PRD §15 point 15 turns Lokalomat into a tool for agents: REST endpoints compatible with
OpenAI where possible, and an MCP server (ADR 0033). The M5c server (ADR 0025) already
serves chat and decisions. The phone gateway (ADR 0020) already runs images, speech and
transcription through the app's modes. The agent API reuses both.

## Decision

1. **Three endpoints on the M5c server** (same port, same keys, same limits, same
   request log without content):
   - `POST /v1/images/generations`: `prompt`, `n` (1 to 4), `size` (mapped to the nearest
     aspect the image engine knows: 1:1, 4:3, 3:4, 16:9, 9:16), `model`. The answer is
     `{created, data: [{b64_json}]}`. `response_format: "url"` is refused with a clear
     error; files are not served over HTTP.
   - `POST /v1/audio/speech`: `input`, `model`, `voice`, `language` (our extension).
     `voice` is a voice from the library by name or id; the OpenAI names (`alloy`...)
     mean the default voice. The answer is always WAV, whatever `response_format` says.
   - `POST /v1/audio/transcriptions`: multipart `file`, `model`, `language`,
     `response_format` = `json` (default), `text`, `srt`, `vtt` or `verbose_json`. The
     upload is put in a private folder for the duration of the request and removed.
2. **Same modes as the app**: the endpoints call `ModeRunner`, so the engine installs
   itself on the first request, the scheduler makes room (ADR 0014), the job appears in
   the Gallery tagged `server`, and a client that disconnects cancels the job.
3. **Models**: `/v1/models` also lists image, speech and transcription models, with a
   `kind` (`image`, `stt`, `tts`). A model id from another kind, or an unknown one, is a
   404 that names `/v1/models`. Chat requests for a media model are refused.
4. **Not in this part**: music, decisions as a tool, pipelines and the MCP server itself
   (ADR 0033); consent per client and the client list (ADR 0034).

## Verification

NIAGARA (RTX 3090), clean folder on drive E, `m6_media_server` with Piper (speech),
Whisper tiny (transcription) and FLUX.2 klein 4B Q4_0 (image):

- Unit tests (fake backend): the key is checked on every endpoint; image `n`, `size` and the
  refused `url` format; WAV speech; every transcription format, a missing file, and the
  upload is removed again; the log has no content.
- Real engines: no key gave 401 on all three; `/v1/models` listed the speech and
  transcription models; speech gave 123 KB of WAV (44 s on the first call, which installed
  the engine; 5 s afterwards); that WAV sent back to the transcription endpoint gave
  "dzień dobry, to jest krótki test." and valid SRT; an unknown model gave 404; an image
  came back as a 2.4 MB PNG in 197 s (first call: engine and model loading).
- Not run here: other machines; music, decisions and pipelines are not in this part.

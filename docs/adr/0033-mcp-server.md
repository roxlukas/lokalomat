# ADR 0033: Lokalomat as an MCP server (M7, part 2)

- Status: accepted (owner, 2026-11-18: MCP over Streamable HTTP on the existing server plus a
  small bundled stdio bridge; the user allows each new client once (ADR 0034); every mode is a
  tool and files come back as resources).
- Date: 2026-11-18

## Context

PRD §15 point 15: agents such as Claude Code should use Lokalomat's models as tools. The
M5c server (ADR 0025) and the media endpoints (ADR 0032) already authenticate agents with
access keys and run every mode through `ModeRunner`. MCP clients start servers either as a
program (stdio) or reach them over HTTP (Streamable HTTP). The existing MCP crate
(`crates/mcp`, ADR 0012) is a *client* for third-party servers and is not reused.

## Decision

1. **Streamable HTTP on the existing server.** `POST /mcp` on the same port and with the
   same keys (`Authorization: Bearer lk-…`), the same rate limit and the same request log
   (one line per tool call, `/mcp <tool>`, no content). Answers are plain JSON; there is no
   event stream and no session: `GET` and `DELETE` are a 405, which the specification
   allows. Batches are accepted. Protocol versions 2025-11-25, 2025-06-18, 2025-03-26 and
   2024-11-05 are answered with the client's version. A request with an `Origin` header that
   is not localhost is refused (a web page must not drive the server).
2. **Tools** (`crates/api-gateway/src/mcp_server.rs`), each a thin call into the same
   backend trait as the REST endpoints, so the engine installs itself on first use, the
   scheduler makes room, jobs reach the Gallery tagged `server`, and a client that goes
   away cancels the work:
   - `list_models`, `list_pipelines`;
   - `chat` (a prompt or messages; one non-streamed answer from the chat slot, thinking
     removed), `decide` (typed questions, answers with probabilities as structured content),
     `generate_image`, `speak`, `transcribe` (a `path`, `audio_base64` or a `resource`;
     text, SRT or VTT), `generate_music`, `run_pipeline` (by id or name; waits for the run).
   A tool's own failure is a result with `isError` so the model can read it and try again;
   an unknown tool is a protocol error.
3. **Files are resources.** Images, speech, songs and pipeline outputs are registered as
   `lokalomat://file/<id>` (the last 100, per key: another key cannot list or read them),
   with name, title, MIME type, size and `_meta` (kind, model, creation time). A tool result
   carries a `resource_link`; an image up to 4 MB also comes back inline so the client can
   look at it. `resources/read` returns the bytes as a blob (up to 128 MB). A resource can be
   fed to `transcribe`, so speech and transcription chain without moving bytes by hand.
4. **No progress notifications in this version.** Long calls (an image takes minutes, a
   song longer) simply answer when they are done; this keeps the server stateless.
   Progress over an event stream can be added later without changing the tools.
5. **Stdio bridge `lokalomat-mcp`** (`crates/mcp-bridge`): a small program that reads
   JSON-RPC lines from stdin, posts each one to `/mcp` and writes the answer to stdout. It
   reads `LOKALOMAT_API_KEY` and optionally `LOKALOMAT_URL` (default
   `http://127.0.0.1:47822`). Messages run concurrently, so a ping is answered while an
   image is made. If the app is not running or the key is wrong, the request gets a
   JSON-RPC error that says so. The app has to be running with its server switched on.
   Bundling the program with the installers and the one-click "Add to Claude Code"
   instructions are part 3 (ADR 0034).
6. **Not in this part**: the consent dialog for a new client, the client list and the UI
   (ADR 0034). Until then a client is whoever holds an access key.

## Consequences

- Any MCP client can use Lokalomat, over HTTP directly or through the bridge.
- A key holder can pass a local file `path` to `transcribe` and `run_pipeline`. That is
  already the power of a key (it can run any mode); `path` must be absolute and an existing
  file, and the file is only read by the audio engines.
- The `chat` tool does not stream; agents that want streaming use `/v1/chat/completions`.

## Verification

- Unit tests with a fake backend and a fake engine over real HTTP: key required, GET is a 405,
  web origins refused, handshake and version negotiation, notifications get 202, batches,
  every tool, `isError` for bad arguments, files private to their key, resources list and
  read, the log has one line per call and no content. The bridge has tests against a fake
  server (answers, notifications, wrong key, app not running).
- Real engines: NIAGARA (RTX 3090), `m7_mcp_server` driven through the `lokalomat-mcp` bridge
  (the way Claude Code starts a server), 12 of 12 checks passed: a wrong key got an error
  for the request; `initialize` and the nine tools; `list_models` (chat, decision, image,
  stt, tts); `chat` ("Warszawa.", 68 s on the first call, which loaded the model); `speak`
  → resource → `resources/read` (127 KB WAV); `transcribe` of that resource ("dzień dobry,
  to jest krótki test."); `decide` (yes, 2 s); `list_pipelines` and `run_pipeline` (a
  one-step chat pipeline, 25 s); `generate_image` (inline 2.4 MB PNG plus a resource, 117 s).
- Not run here: music through MCP (same path as the Workshop, covered by a fake backend),
  other machines, Claude Code itself as the client (part 3 adds the one-click setup).

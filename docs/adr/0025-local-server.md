# ADR 0025: The local OpenAI-compatible server (M5c, part 1)

- Status: accepted (owner, 2026-11-17: M5c in three PRs; this is the first)
- Date: 2026-11-17

## Context

PRD §15 point 9 asks for an Expert-only **Server** panel: an API compatible with OpenAI's,
on 127.0.0.1 by default and on 0.0.0.0 once enabled, with access keys, so third-party
agents (point 11, ADR 0026) can use local models "like Ollama".

The M5 gateway (`crates/api-gateway`, ADR 0020) only serves the paired phone:
- TLS with a pinned self-signed certificate, and per-device keys.
- Non-streaming chat with string content only; `tools`, `stream` and `temperature` are
  dropped.
- Its own error and model-list shapes.

Agents need standard clients (OpenAI SDKs, LiteLLM, the AI SDK) to work unchanged:
- plain HTTP;
- bearer keys;
- SSE streaming, `tools` / `tool_choice`, `response_format`;
- `/v1/models` in OpenAI's list shape and OpenAI's error shape.

## Decision

1. **A separate plain-HTTP listener** (`lokalsi_api_gateway::openai`, port **47822**). The
   phone gateway (TLS, 47821) is unchanged. The listener binds to `127.0.0.1`, or to
   `0.0.0.0` with the "Udostępnij w sieci lokalnej" switch. It is off by default
   (`server_enabled`). In M5c part 2, starting an agent turns it on.
2. **A proxy in front of llama-server, not our own OpenAI implementation.** For
   `/v1/chat/completions`, `/v1/completions` and `/v1/systemone` the server does three
   things:
   - checks the key;
   - resolves `model`;
   - asks the engine for a loaded slot (`EngineService::upstream`), which installs
     llama.cpp, makes room through the scheduler and loads the model, as the Workshop
     does.

   The body then goes to llama-server **unchanged**, and the answer (status,
   `content-type`, SSE stream) comes back unchanged. Agents therefore get llama.cpp's own
   OpenAI compatibility: streaming, tool calls (Jinja templates are on by default since
   llama.cpp PR #17911, so b12110 has them), `usage` and `response_format`. There is
   nothing for us to keep in sync.
3. **The slot is reserved for the whole answer.** `Upstream` owns the turn (`generation`
   lock, now in an `Arc`) and lives inside the response stream. llama-server runs with one
   slot (`-np 1`), so requests from agents, the Workshop and pipelines queue instead of
   failing. A client that disconnects drops the stream: the turn is released, and
   llama-server stops generating when its connection closes. Decisions use the decision
   slot (ADR 0024), so they never wait for chat.
4. **Context for server loads** is the one set in the settings, else the largest that fits
   free memory (`ctx_plan`, at most 32k, ADR 0014/0021). Agents send long system prompts
   and tool lists. A request that does not fit gets llama-server's own 400, passed through.
5. **Models**: `GET /v1/models` lists installed chat models (`<model>:<variant>`, the same
   ids as the phone) and decision models (`kind: "decision"`). The default chat model is
   also listed as `default`. A request without `model`, or with `default`, uses it. An
   unknown id is a 404 `model_not_found`.
6. **Access keys** (`lokalsi_core::api_keys`, migration 9):
   - Format `lk-<id>.<secret>`, sent as `Authorization: Bearer …`.
   - The database keeps the SHA-256 and a usage count. The whole key also goes to the OS
     credential store (`secrets::api_key`), so "Kopiuj" works later and agents can be
     set up again.
   - A key is **always** required, also on 127.0.0.1: other programs and web pages on
     the same computer must not use the models unasked.
   - 600 requests per minute per key. `GET /health` is the only open route.
7. **Errors in OpenAI's shape**: `{"error":{"message","type","code"}}`.
   - 401 `invalid_api_key`, 404 `model_not_found`, 429 `rate_limited`.
   - 503 while the engine cannot start, with a short "try again".
   - 502 when llama-server does not answer.
8. **Request log**: the last 200 requests in memory, shown in "Ostatnie zapytania". Each
   entry has the time, key name, path, model, status, duration and whether it streamed.
   **No prompt or answer content**, following the logging rules (ADR 0004); `tracing`
   logs the same metadata.
9. **Not in the server (yet)**:
   - `/v1/embeddings` (no embedding models in the catalogue).
   - `/v1/responses` (Codex is not one of the chosen agents).
   - Audio and image endpoints: PRD §15 point 15, after M6.
   - CORS headers. Browsers cannot read answers from another origin, which is wanted.

## Measurements

`crates/api-gateway/examples/m5c_server.rs` on 2026-11-17, NIAGARA (Windows 11, RTX 3090,
llama.cpp b12110 CUDA 13), a clean folder, Qwen3.5 4B Q4_K_M + Basal 2.0 mini Q8_0:

| Check | Result |
|---|---|
| No key | 401 `invalid_api_key` |
| `/v1/models` | `default`, `qwen3.5-4b:q4_k_m`, `basal-2.0-mini:q8_0` |
| First streamed request (llama.cpp installed by itself, model loaded) | 55.7 s, "Warszawa" |
| Warm stream | 1 284 SSE chunks in 9.3 s (thinking + answer, ≈ 150 tok/s) |
| `tools` + `tool_choice: "required"` | `get_weather {"city":"Kraków"}` in 0.8 s, `usage` passed through |
| `/v1/systemone` | P(spam) 0.98 in 1.7 s (first load of the decision slot) |
| Workshop chat and a server request at once | both answered, queued on the one slot |
| LAN address while bound to 127.0.0.1 | connection refused |

The same example on the other two lab machines (fresh folders, llama.cpp installed by
itself):

| Check | srwpodbiurkiem2 (Windows 10, RTX 2070 8 GB) | NUC (Ubuntu 24.04, no GPU, Vulkan iGPU) |
|---|---|---|
| No key / models / not on the LAN | ok | ok |
| First streamed request (install + load) | 200.5 s | 42.8 s (6.7 s on the second run, engine reused) |
| Tool call `get_weather {"city":"Kraków"}` | 1.6 s | 29 s |
| `/v1/systemone`, P(spam) | 0.98 in 1.9 s | 0.98 in 3.9 s |
| Workshop chat + server request at once | both answered, 76 s | both answered, 364 s; the Workshop answer was empty (the 1 024-token limit went to thinking) |

In the real app (NIAGARA, `npm run tauri dev`, driven through WebView2 CDP), the
following worked:
- switching the server on and creating a key;
- curl with streaming and a tool call (`{"city":"Gdańsk"}`);
- the request log in the view;
- LAN sharing, with requests from the NUC: `/health` 200, no key 401, with key 200;
- with sharing off, the NUC cannot connect;
- revoking the key in the view, after which the key gets 401.

One bug was found and fixed: `set_server` saves the settings, but the view did not reload
the UI's copy. A later settings save could switch the server off again.

**A request without `max_tokens` can hold the chat slot for minutes.** Qwen3.5 4B (a
thinking model) answered "one sentence about mountains" with 32 748 tokens, until the
32k window was full (≈ 4 min at 135 tok/s). The Workshop waited behind it. The proxy does
not change bodies, so it does not add a limit. Agents send `max_tokens`, and the request
log shows such requests. A default limit, or more slots (`-np 2`), can come later if
real use needs it.

The same happens on the app's own side. The one-shot chat used by pipelines and the phone
(`EngineService::complete` with `ChatParams::default()`) sends no `max_tokens` either. On
the NUC, at 3 tok/s, the first run of the example spent almost 2 hours on 20 000 thinking
tokens before it was stopped. This is older M5 behaviour, not part of the server, and it is
fixed in this PR:
- Without `max_tokens`, `complete()` now limits the answer to 4 096 tokens plus about the
  length of the prompt (≈ 3 characters per token), so a translation of a long text
  still fits. The limit never goes past what is left of the context window
  (`default_answer_tokens`).
- An answer that is still empty, because a thinking model spent the whole limit, ends as
  `no_result` with the detail "the answer limit was used up before the answer". It no
  longer looks like a successful empty answer.

The server keeps passing bodies unchanged: there the client decides.

## Consequences

- One new migration (`api_keys`), new settings (`server_enabled`, `server_port`,
  `server_lan`), and new Tauri commands: `get_server`, `set_server`, `create_api_key`,
  `api_key_secret`, `revoke_api_key`, `server_log`.
- `EngineService::generation` is an `Arc`, so a request can own its turn. Behaviour is
  otherwise unchanged.
- The Server view replaces the M5c placeholder. It is still Expert-only.

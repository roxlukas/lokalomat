# ADR 0014: Context window sized to tools and GPU memory

- Status: accepted
- Date: 2026-11-11

## Context

`llama-server` was always started with an 8 192-token context. With the 80 tools of
mcp-grafana the request alone took ~30 000 tokens and was rejected
(`exceed_context_size_error`). The product owner asked for a configurable context size
that respects VRAM: large windows only on cards with a lot of memory, for chat and the
future Server mode (PRD §15 item 9).

## Decision

- Setting `context_size` (Expert, Settings → Hardware): Automatic (default) or a fixed
  4k–128k tokens. A fixed value is used as is (llama.cpp `--fit` offloads if needed).
- Automatic = what the request needs, capped by free memory:
  - need: 8 192 without tools; with tools ≈ tool JSON characters / 3 + 6 144, rounded up
    to a power of two, at most 32 768 (`context_for`);
  - cap: `ctx_limit(budget, model)` – the KV cache is estimated as ~2% of the model size
    per 1 000 tokens; free = memory budget − model − 512 MB; rounded down to a power of
    two, at least 2 048. A 24 GB card with a 9 GB model gets 32k, a 12 GB card 8k.
- The server restarts only when the loaded window is smaller than needed.
- A prompt that still does not fit fails with `context_full` and a plain message
  ("turn off some tools") instead of a generic error.
- The Server mode (M5c) will use the same `LoadRequest::context`.

## Consequences

The KV estimate is a heuristic (no per-model architecture data in the catalog); it errs
on the safe side. Many MCP tools make every request slow to process, so curated default
tool sets for built-in servers are a follow-up.

Update (ADR 0021): when the f16 cache cannot reach the largest window, the KV cache is
compact (q8_0) and the window is computed without the 8k cache the estimate includes.

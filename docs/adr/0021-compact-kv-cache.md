# ADR 0021: Compact KV cache on tight graphics cards

- Status: accepted
- Date: 2026-11-14

## Context

Users with 8 GB graphics cards ask what they can run locally (forum question on
agentsclub). The product owner's answer: Gemma 4 12B in IQ4_XS (~6.4 GB) with flash
attention and a q8_0 KV cache in llama.cpp – settings Ollama and LM Studio do not
expose. Lokalomat should apply this by itself, without the user knowing the flags.

Before this ADR `llama-server` always ran with the default f16 KV cache (flash
attention `auto`), the catalog had no Gemma 4 12B variant below Q4_K_M (7.1 GB file,
8 496 MB estimate), and the automatic context window (ADR 0014) gave 8 GB cards the
smallest window.

Measured on an RTX 3090 (llama.cpp b12110, CUDA 13, whole model on the GPU, MiB
above the idle card):

| Model | f16 8k | q8_0 8k | f16 32k | q8_0 32k | estimate |
|---|---|---|---|---|---|
| Gemma 4 12B IQ4_XS | 7 181 | 6 927 | 7 584 | 7 161 | 7 713 |
| Bielik 11B Q6_K | 10 628 | 10 043 | 15 609 | 12 669 | 10 638 |
| Qwen3.5 9B Q4_K_M | – | – | – | 6 107 | 6 983 |

The estimate (`size × 1.10 + 1 024`) matches an 8k f16 cache for dense models
(Bielik); the KV heuristic of ADR 0014 (2% of the estimate per 1k tokens: 212 MB)
matches Bielik's 207 MB; q8_0 halves it (109 MB). Gemma 4 uses sliding-window
attention, so its cache is far smaller than the heuristic – the heuristic stays safe.

## Decision

- Catalog: Gemma 4 12B gets the `iq4_xs` variant (`unsloth/gemma-4-12b-it-GGUF`,
  size/sha256 by `catalog-sync`). Q4_K_M stays the recommended variant.
- `ctx_plan(budget, model)` (crate `lokalsi-engines`) replaces the bare `ctx_limit`:
  - when the f16 cache already allows the largest automatic window (32k) nothing changes;
  - otherwise the KV cache is *compact*: `-fa on -ctk q8_0 -ctv q8_0`. The window is
    computed from the estimate without its 8k f16 cache, at half the heuristic per
    token. Gemma 4 12B IQ4_XS on an 8 GB card (7 680 MB budget) gets 8k with compact
    KV, where f16 left nothing (measured: even 32k fits in 7.2 GB).
- Fit labels of LLMs (`lokalsi_hwprobe::llm_fit`): when a variant does not fit
  comfortably, the compact estimate (estimate − half of the 8k f16 cache) is used if it
  gives a better label. Gemma 4 12B IQ4_XS on 8 GB: "offload" → "tight" (7 097 MB).
  The starter pack (60% of the budget for LLMs) is unchanged.
- `LoadRequest` and `LoadedModel` carry `kv_compact`; a change restarts the server.
  The user's fixed context size (Expert) is used as is; the KV format follows the
  memory budget either way.
- No new UI text: Basic users see a model that fits and a larger window; the flags are
  in the verbose log (`starting llama-server … kv_compact=true`).
- Check on real hardware: `cargo run --release -p lokalsi-engines --example kv_compact
  -- <dir> <model.gguf> <estimate MB>` with `BUDGET_MB` (default 7 680) – prints the
  plan, the GPU memory in use and one answer.

## Consequences

q8_0 keys and values cost a barely measurable quality loss (llama.cpp perplexity
tests) for half the KV memory; cards with a lot of memory keep f16. For dense models the
compact estimate is ~250 MB below the measurement (Bielik: 9 787 vs 10 043 MB), within
the GPU reserve; `--fit on` offloads layers if it is ever short. Flash attention must
work on every shipped backend (CUDA, Metal, Vulkan, CPU in b12110); a backend without
it fails to start with a quantised V cache and the log names the cause. Only CUDA was
measured; Metal and Vulkan are unverified.

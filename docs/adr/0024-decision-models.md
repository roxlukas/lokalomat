# ADR 0024: Decision models (Basal) on the llama.cpp engine

- Status: accepted (owner, 2026-11-17: a Workshop mode and a pipeline step, Expert only)
- Date: 2026-11-17
- Deviates from PRD §15 point 10 ("Basal uruchamiany silnikiem `basal-serve` (Python)").
  ADR 0023 is reserved by the media converters branch.

## Context

PRD §15 point 10 asks for decision models "in the style of Jev and Basal": instead of
text, the model returns a typed decision (a choice, yes/no or a score) with the
probability of every answer. Basal (Apache-2.0, Polish, fine-tuned from Bielik) was to
run on `basal-serve`, a Python server, in the M4 worker.

Checked on 2026-11-17, from the official repository and the Hugging Face and GitHub APIs:

- **basal-serve 2.8.1** (github.com/rkinas/basal) needs Python 3.12 and torch 2.11
  (cu128). Its fast path needs FlashAttention 3 (Hopper). Windows support is not stated.
  Its GGUF mode needs a CUDA build of llama-cpp-python compiled from source. None of this
  is plug and play.
- **Upstream llama.cpp serves `POST /v1/systemone` itself** (the TypeSafe System One API
  that Jev uses). It was added on 2026-10-02 in ggml-org/llama.cpp a4cb4c6, and it works
  for GGUF files that carry decision metadata (`<arch>.decision.*`, the
  `tokenizer.chat_template.systemone` template and the calibration temperatures). Our
  pinned build **b12110 already contains it**. `/v1/models` reports such a model with
  `output_modalities: ["decisions"]`.
- **Basal 2.0** publishes ready `-decision.gguf` files: `Remek/basal-2.0-{mini,4.5B,max}-GGUF`,
  Q8_0 and Q5_K_M. Weights are Apache-2.0; languages pl and en; base models Bielik 1.5B,
  4.5B and 11B v3. According to the model card, Q8_0 picks the same answer as bf16 in
  98.4% of the 4.5B's development decisions.
- **Jev** (TypeSafe AI) runs only in the cloud. Its API has the same shape. It is outside
  local-first and is not used.

## Decision

1. **Decision models run on the llama.cpp engine we already ship**, through llama-server's
   `/v1/systemone`. There is no Python environment and no basal-serve. This works with
   CUDA, Vulkan, Metal and CPU on Windows, macOS and Linux, and installs itself like chat
   (plug and play, PRD §15 point 1).
2. **New modality `decision`** (catalogue feature `decision`, job kind `decision`). Decision
   models never appear in chat pickers, and chat models never appear in decision pickers.
3. **A second llama-server slot** (`llama-cpp-decision`): `EngineService::second_slot`
   shares the installed build and installs through the main service. It has its own
   process, log (`engine-llama-cpp-decision.log`) and scheduler resident. The chat model
   stays loaded next to a decision model. On a small card the scheduler's LRU unloads one
   of them (ADR 0015). The slot does not publish engine status, so the Engines view still
   shows llama.cpp once. Context: the automatic plan, at most 16k tokens.
4. **Catalogue**: Basal 2.0 mini, 4.5B and max, each in Q8_0 (recommended) and Q5_K_M,
   only the `-decision.gguf` file. Sizes, sha256 and revisions come from `catalog-sync`.
   The memory estimate is as for chat models. No speed level is shown: a decision is one
   prompt pass, so tokens per second say nothing about it.
5. **Question types**: choice (2–26 options), yes/no (System One `noul`), score (2–10
   levels, lowest first); up to 12 questions per request, a text of up to 20 000
   characters. The engine's answers are turned into one shape: the most likely label,
   every probability in question order, and confidence. For yes/no, llama.cpp returns
   only P(yes); confidence is `|2p − 1|`.
6. **UI (Expert only, ADR 0013)**: Workshop mode "Decyzje" (Alt+6; Łańcuch moves to Alt+7).
   It has the text to judge, a question editor and a result card with probability bars,
   "Zapytaj ponownie" and "Kopiuj JSON". Decisions are not media: they stay in the
   Workshop history and are left out of the Gallery on the desktop and the phone
   (`JobFilter.media_only`). In Basic mode the mode, the decision models and the
   templates with a Decide step are hidden.
7. **Pipelines**: step `decide` (text → the chosen answer as text) with one question and
   an optional **gate** ("go on only when the answer is X", either the most likely answer
   or at least N percent likely). A closed gate ends the run as done and marks the
   remaining steps as skipped (`StepRun.skipped`). New template: "Wiadomość głosowa →
   temat".

### Limits of the llama.cpp route

These are the limits of the `openjev` readout in llama.cpp, from basal's GGUF.md:

- At most 26 options per question.
- Only choice, noul and score. There is no `multi`, `act`, `tool`, `rank`, `facts`,
  `ask_if`, and no certified thresholds or `coverage`. All of these need basal-serve.
- One option order. The option keys are always shown. The language is picked from the
  text.
- Probabilities are scaled by the temperatures stored in the file. They are not
  calibrated for the user's own data.

## Not in M5b

- `/v1/systemone` on the M5c Server and on the phone gateway.
- Images in the state (basal-omni needs `mmproj`).
- The question types that need basal-serve.
- Jev.

## Measurements

`crates/engines/examples/m5b_acceptance.rs` on 2026-11-17: Windows 11, RTX 3090 24 GB,
llama.cpp b12110 CUDA 13, a clean folder, `basal-2.0-4.5b/q8_0` (4.83 GB download, 248 s).
Three questions per text (choice, yes/no, score):

| Step | Result |
|---|---|
| First decision (llama.cpp installed by itself, model loaded) | 58.6 s |
| Next decisions, 3 questions, ≈ 300–340 prompt tokens | 0.55–0.70 s |
| Probabilities of every answer | sum 1.000 |
| Gemma 4 12B IQ4_XS chat, then a decision | chat stays loaded; both slots resident (6 334 + 7 713 MB estimate) |
| Gated pipeline, spam text | gate closed after 0.58 s; topic step skipped; run done |
| Gated pipeline, normal text | both steps done in 2.1 s ("praca") |
| Template "Wiadomość głosowa → temat", 30 s recording | 83 s, of which ≈ 80 s for the first whisper.cpp install and transcription; decision 0.47 s |

Sample answers (Polish bank messages):

- "nie mogę zalogować się… nikt nie odbiera infolinii": dział "logowanie i dostęp"
  0.92; zdenerwowany "tak" 0.93; pilność between "dzisiaj" (0.45) and "natychmiast"
  (0.46).
- A polite question about a card limit: "karty" 0.96; zdenerwowany "nie" 0.96; pilność
  "może poczekać" 0.64.

The RTX 3090 was shared with another GPU program during this run: 23 GB used and 93%
busy right after, measured with `nvidia-smi`. That explains why its decisions are slower
than the RTX 2070's below.

**8 GB card**: srwpodbiurkiem2 (Windows 10, RTX 2070 8 GB, driver 560.94, i7-870, 16 GB
RAM), the same binary, a clean folder, budget 7 680 MB:

| Step | Result |
|---|---|
| First decision (llama.cpp CUDA installed by itself, model loaded) | 160.5 s |
| Next decisions, 3 questions | 0.16–0.17 s |
| Gated pipeline | 0.07 s per decision; gate closed and open as on the RTX 3090 |
| Template with the 30 s recording | 106 s (first whisper.cpp install and transcription), decision 0.15 s |

**No dedicated graphics card**: NUC (Ubuntu 24.04, Intel NUC 7 with integrated
graphics, budget 11 640 MB from RAM). llama.cpp picked its Vulkan build by itself:

| Model | First decision (install + load) | Next decisions, 3 questions | One question (pipeline) |
|---|---|---|---|
| `basal-2.0-4.5b/q8_0` | 22.4 s | 14.1–15.5 s | 4.4–6.5 s |
| `basal-2.0-mini/q8_0` | 8.5 s | 4.6–5.1 s | ≈ 2.1 s |

All three machines gave the same top answers on the three texts, except one. The third
text's urgency came out as "w tym tygodniu" vs "dzisiaj"; those two are 0.36 / 0.36
apart, a tie. Every probability set summed to 1.000.

On computers without a graphics card, **Basal mini** is the better choice: about 1.5 s per
question instead of about 5 s. Nothing picks it automatically: decision models are never
in the starter set, and the user chooses the model in the Models view.

## Consequences

- New modality, job kind, Workshop mode, pipeline step and template; locale keys in pl
  and en. No DB migration: jobs and pipeline steps store their parameters as JSON.
- Basal's answers are model output. The UI shows them as data and never acts on them by
  itself; only a gate the user set acts on them.
- A Basal model that one day needs basal-serve features (`multi`, `act`) would be a new
  ADR: a Python engine next to this route, not instead of it.

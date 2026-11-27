# ADR 0041: Context window indicator under the chat input

- Status: accepted
- Date: 2026-11-27

## Context

The owner asked for a small ring under the chat input that shows how much of the
model's context window the conversation uses and how much the text being typed
will add (PRD §15 item 23). A similar indicator in another product
had known weak points: the numbers were lost when a conversation was reopened,
the window size came from an environment variable rather than the loaded model,
and the model's answer was not counted although it joins the next prompt.

## Decision

- **Numbers come from the engine.** `llama-server` reports `usage.prompt_tokens`
  in the last streamed chunk (`stream_options.include_usage`); when it is missing
  we take `timings.prompt_n + timings.cache_n`. There is no tokenizer in the app.
- **Used = prompt of the last round + tokens of the answer.** In a tool loop each
  round sends the whole conversation, so the last round is closest to the next
  turn. The answer is added because the next prompt contains it, without the
  model's reasoning, which is not sent back (its share of the answer's tokens is
  taken out by characters; a test answer was 3005 tokens, nearly all reasoning).
- **Window size = `ctx` of the loaded server** (`LoadedModel.ctx`), not the
  setting, so Automatic and Expert sizes both show the real window.
- **Breakdown** (`lokalsi_core::chats::context_breakdown`): prompt tokens split in
  proportion to characters of the system message, the JSON of the offered tools
  and the rest; messages are the remainder so the parts always add up. It is an
  estimate and shown as one ("ok.").
- **Stored with the assistant message** (`messages.context`, JSON, migration 11),
  so the ring is right after reopening a conversation. Old rows have `NULL`.
- **Pending estimate in the UI**: `ceil(chars / 3.5)` of the typed text.
- **UI** (`ContextRing.svelte`, the chat panel's `belowPrompt`): two arcs (used in
  the accent colour, yellow from 80%, red from 90%; pending in the info colour,
  starting where used ends). The tooltip says it in one plain sentence; from 90%
  it suggests a new chat. Expert mode adds the breakdown. Before the first answer
  the ring uses the loaded model's window; with no model loaded it is hidden.

## Consequences

- Cancelled answers have no usage, the ring keeps the previous value.
- The phone chat does not show the ring yet.

# ADR 0042: Context window choice "Max" and a separate one for the server

- Status: accepted
- Date: 2026-11-27

## Context

The context window could be fixed in Expert settings (ADR 0014) at 4k–128k, and the
same value also fixed the local server's window (ADR 0025/0028). The product owner
asked for the choice 8k / 16k / 32k / 64k / 128k / Max in Expert mode and on the
Server view. Agents and chat often need different windows.

## Decision

- Choices: Automatic, 8k, 16k, 32k, 64k, 128k, Max. 4k is no longer offered; a saved
  4k stays valid and is still listed.
- Max is stored as `0` (`CTX_MAX`) and turned into tokens when a model is loaded
  (`service::chosen_ctx`): the model's trained window `<arch>.context_length` from its
  GGUF file (`core::gguf::context_length`); 128k when the file does not say.
- New setting `server_context_size` (Server view, same choices). Automatic keeps the
  ADR 0028 behaviour (the largest window that fits, up to 64k). Chat's
  `context_size` no longer affects the server.
- A fixed value or Max is used as is, like before: a large window can push layers to
  the processor and slow the model down. The tooltip says so.

## Consequences

Users who fixed the chat window to make the server bigger have to set it again on the
Server view. Max can mean 256k or more for some models, so it is an Expert choice.

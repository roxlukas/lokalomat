# ADR 0007: Catalog schema additions, app.db location, download details

- Status: accepted
- Date: 2026-11-11

## Context

PRD §6.1 shows the manifest of a single model; §7 describes the download manager;
§4.5 says the data folder is chosen in the wizard.

## Decision

**Catalog** (`catalog/models.json`, embedded in the binary):

- top level is `{ "version": 1, "models": [...] }`;
- each file gets `revision` (the commit its size/sha256 were read from; downloads use
  `…/resolve/<revision>/…`, so a later change upstream cannot break verification) and an
  optional `role` (`model`, `diffusion_model`, `vae`, `llm`, `config`…) for engines;
- variants get an optional `recommended` flag;
- `tools/catalog-sync` fills `size`, `sha256`, `revision` and `memory_estimate_mb` from
  the HF API. Small non-LFS files (configs, tokenizers) only have a git blob id there,
  so the tool downloads them (≤ 16 MiB) and hashes them. A unit test fails the build
  if any file is unsynced.

**app.db** lives in the application home (`%LOCALAPPDATA%\Studio Lokalsi`), not in the
data folder: it must exist before the user picks a folder, and moving the data folder
should not orphan the library. Model files go to `<data_dir>/models/<model>/<variant>/`.

**Downloads**: one transfer at a time (FIFO with manual reordering); network errors are
retried 3× with backoff before an item is marked failed; a checksum mismatch discards
the file and retries once (PRD). Files already in the Hugging Face cache are reused in
place (`models--org--repo/blobs/<sha256>` with matching size) and never deleted by the
app; the same applies to custom GGUF files added from disk. The HF token is stored in
the OS credential store (`keyring` 4) and only sent to `huggingface.co` URLs.

**Starter pack**: per modality the best catalog model whose recommended (or largest
fitting) variant uses at most a share of the budget (LLM 60%, image 80%, STT 35% – 15%
without a GPU, TTS 35%); Polish LLMs preferred when the UI is Polish, commercial
licences preferred for images, the light non-cloning TTS preferred.

## Consequences

Catalog edits must be followed by `cargo run -p catalog-sync`. Scanning user-chosen
model folders (PRD §7 "wskazany katalog") is not implemented yet; only the HF cache and
files already at the destination are detected.

# ADR 0008: Sorting models by the AI Transformation benchmark

- Status: accepted
- Date: 2026-11-11

## Context

The product owner asked to sort models by task quality (e.g. programming) using the
local-LLM benchmark published at <https://aitransformation.pl/llm-benchmark/browse.html>
(data: `llm-benchmark/results.json`, CC BY 4.0), and to simply ignore the benchmark
when the site does not respond. The PRD has no such feature, and §10 limits network
traffic to model/engine downloads and update checks – this request is an explicit
exception made by the product owner.

## Decision

- At startup the app fetches `results.json` once in the background (8 s timeout,
  8 MiB cap). On success the UI gets a `benchmark://updated` event and shows the sort
  menu; on any failure (offline, timeout, HTTP error, unexpected format) it logs one
  INFO line and benchmark sorting is hidden for that run. No cache, no retries.
- Mapping is explicit and exact: a catalog model's `benchmark` field lists benchmark
  names; only the same model in the same version gets a score (Bielik 11B **v3.1** has
  none, only v3.0 was benchmarked). For Bielik 4.5B the `hf.co/speakleash/…` run is
  used because it pulls exactly the catalog file. `cargo run -p catalog-sync` checks
  that every mapped name exists in the published results.
- Sort options: catalogue order, overall score, speed (tok/s) or any benchmark
  category. Models without a score keep their order at the end. Scores are per model
  (measured at Q4_K_M in Ollama), not per variant, and the UI says so.
- Attribution required by CC BY 4.0 is shown next to the sort with a link to the source.
- The author credit (Łukasz Poźniak, link to aitransformation.pl) sits in a thin status
  bar at the bottom right of the window, so it never overlaps the Workshop command bar.

## Consequences

One request to aitransformation.pl per start. Fresh results appear without an app
release. Custom GGUF models are never matched automatically.

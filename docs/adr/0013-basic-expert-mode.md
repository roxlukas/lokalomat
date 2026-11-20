# ADR 0013: Basic / Expert mode and self-installing MCP servers

- Status: accepted
- Date: 2026-11-11

## Context

The product owner wants a plug-and-play app for people who find Ollama or ComfyUI too
hard: automatic installs everywhere, and advanced UI hidden by default behind a
two-state **Basic / Expert** switch in the top-right corner.

## Decision

- Setting `expert` (default `false`) in `settings.json`, toggled from the header.
- Basic hides: the Engines view (rail and command palette), custom GGUF models,
  benchmark sorting, commercial/EU filter chips, the variants table and engine line in
  model details, chat speed statistics, the Hardware/Downloads/Advanced (logs,
  diagnostics) settings, MCP import/export, custom MCP servers and server logs. Basic
  offers only the Grafana template with two fields (address, token).
- Nothing is removed – Expert shows everything; support can ask the user to switch.
- Built-in MCP servers: a stdio server whose command is `builtin:<name>` is resolved on
  connect; the program is downloaded from its pinned GitHub release
  (`catalog/engines/mcp-grafana.json`, v2.3.0), checked against the GitHub sha256
  digest (synced by `catalog-sync`) and unpacked into `<data_dir>/tools/`, reusing the
  engine installer. The Grafana template uses `builtin:mcp-grafana`.

## Consequences

New advanced UI must be wrapped in `settings.value.expert` (or marked `expert` in
`views.ts`). Other MCP servers can become built-in by adding a manifest.

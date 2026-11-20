# ADR 0004: Diagnostic logging with normal and verbose modes

- Status: accepted
- Date: 2026-11-11

## Context

PRD §10 asks for rotated logs in the data directory without prompt contents. The
product owner additionally requires **two user-selectable levels – normal and
verbose** – so that problems on users' machines can be diagnosed.

## Decision

- `lokalsi_core::logging` on `tracing` + `tracing-subscriber` + `tracing-appender`.
- Modes: **Normal** = `info`; **Verbose** = `debug` globally and `trace` for our crates
  and the `ui` target, with chatty third-party crates (tao, wry, hyper, reqwest, rustls)
  kept at `info`.
- Switchable at runtime via `tracing_subscriber::reload` (Settings → Advanced), no
  restart. Forced at startup with `--verbose` or `STUDIO_LOKALSI_LOG=verbose|normal`
  (the UI shows that the level is forced).
- Daily files `app.YYYY-MM-DD.log`, 14 kept, in `<home>/logs` (home =
  `%LOCALAPPDATA%\Studio Lokalsi` until the data directory is chosen in M1). Future
  per-component files: `engine-<id>.log`, `mcp-<name>.log`.
  `tracing-appender` has no size-based rotation; if huge verbose logs become a
  problem we add a size cap in a custom writer.
- Frontend errors (`error`, `unhandledrejection`) are forwarded via the `log_from_ui`
  command (target `ui`).
- Panics are logged (with backtrace in verbose mode).
- Privacy: secrets are masked by `lokalsi_core::redact` (HF `hf_…`, Grafana `glsa_…`,
  bearer headers, `*token*/*secret*/*password*/*api_key*` fields) at the source for UI
  messages and again when exporting. User content goes through
  `logging::loggable_content`, which logs only its length unless the user enabled
  "log prompt contents" **and** verbose mode is on.
- "Export diagnostics bundle" writes a ZIP with README, `system.json`,
  `settings.json` and all logs, each redacted.

## Consequences

Every new module (hwprobe, downloads, supervisor, jobs, scheduler, gateway, MCP,
Python worker) logs through `tracing` with spans carrying `job_id` / `engine_id` /
`download_id`; reviewing log coverage is part of each milestone's acceptance.

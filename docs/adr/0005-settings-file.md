# ADR 0005: Bootstrap settings in settings.json

- Status: accepted
- Date: 2026-11-11

## Context

The plan for M0 said preferences would live in `localStorage` until SQLite arrives in
M1. The log mode, however, must be known in Rust before the WebView exists, and
`localStorage` is per-WebView and invisible to the core.

## Decision

Language, theme, log mode and the prompt-logging flag are stored by the core in
`<home>/settings.json` (atomic write, corrupt/missing file → defaults with a logged
warning) and exposed through `get_settings` / `update_settings`. `localStorage` is
used only for per-window UI conveniences (rail expanded, inspector open).

In M1 the remaining settings go to SQLite; `settings.json` stays as the small
bootstrap file (it also has to remember where the data directory is).

## Consequences

Settings survive restarts and WebView data resets, and are included (redacted) in the
diagnostics bundle.

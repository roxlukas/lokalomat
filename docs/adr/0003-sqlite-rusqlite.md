# ADR 0003: SQLite via rusqlite (bundled) with SQL migrations

- Status: accepted (implemented in M1)
- Date: 2026-11-11

## Context

PRD §4.5 requires a local SQLite database `app.db` for models, downloads, jobs,
results, conversations, voices, pipelines and settings.

## Decision

Use `rusqlite` with the `bundled` feature (no system SQLite dependency on any
platform). Migrations are an append-only list of SQL strings in
`crates/core/src/db.rs`, applied in order at startup in a transaction and tracked
by `PRAGMA user_version`; a database newer than the binary is refused instead of
being modified. WAL journal mode. The UI never touches the database directly.

M1 keeps a single connection behind a mutex: queries are short and the guard is
never held across an `.await`. A pool or a dedicated DB thread can replace it when
job/history volume grows (M3+).

## Consequences

Synchronous API on short critical sections; no ORM, queries are reviewed SQL.
Released migration entries must never be edited, only new ones appended.

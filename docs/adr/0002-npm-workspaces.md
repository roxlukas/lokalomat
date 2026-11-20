# ADR 0002: npm workspaces + Cargo workspace

- Status: accepted
- Date: 2026-11-11

## Context

The repository layout (PRD §12) has a frontend (`ui/`), a Tauri shell
(`apps/desktop/`) and Rust crates (`crates/*`). pnpm is not available on the
development machine; npm ships with Node.

## Decision

- npm workspaces: `ui` (`@lokalsi/ui`) and `apps/desktop` (`@lokalsi/desktop`, holds
  `@tauri-apps/cli`). Root scripts: `npm run tauri dev|build`, `check`, `test`, `dev:ui`.
- One Cargo workspace at the root with `crates/core`, `crates/engines`,
  `crates/hwprobe` and `apps/desktop/src-tauri`. Shared versions in
  `[workspace.dependencies]`.
- Logic lives in `crates/*` and is tested without Tauri; `src-tauri` only maps it to
  IPC commands.

## Consequences

`npm ci` + `cargo test --workspace` are the whole CI bootstrap. Moving to pnpm later
only changes the lockfile and scripts.

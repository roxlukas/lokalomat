# ADR 0019: Product name "Lokalomat AI Studio"

- Status: accepted
- Date: 2026-11-12

## Context

The product owner renamed the product for the release candidate from "Studio Lokalsi"
to **Lokalomat AI Studio** (short form in UI texts: **Lokalomat**). The old name is
spread over user-visible texts and over technical identifiers that existing
installations depend on.

## Decision

Only the visible name changes:

- window title, `productName` (installer names, Start menu entry), About, welcome
  screen and other UI strings (`app.name` in both locales), `index.html` title;
- metadata written into results (`Software` in PNG/MP3/voice files), the diagnostics
  bundle README, log lines, crate/worker descriptions, PRD and agent instructions;
- default file names offered when saving (`lokalomat-diagnostics-….zip`,
  `lokalomat-wyniki.zip`).

These stay unchanged so that data, settings, stored secrets and open work keep working:

- app home and data folder (`%LOCALAPPDATA%\Studio Lokalsi`, `<disk>/Studio Lokalsi`),
  app identifier `pl.lokalsi.studio`, environment variables `STUDIO_LOKALSI_*`;
- OS credential store service names (`Studio Lokalsi`, `Studio Lokalsi MCP`) –
  renaming them would lose the Hugging Face token and MCP secrets;
- voice file format id `studio-lokalsi-voice` / `.lokalsi-voice.zip`, browser storage
  keys `lokalsi.*`, HTTP User-Agent `StudioLokalsi/…`;
- crate, package and binary names (`lokalsi-*`, `@lokalsi/*`, `studio-lokalsi`),
  the repository name, and earlier ADRs (they record history).

## Consequences

- Windows: the new `productName` gives the installer a new name and install folder,
  so a previously installed "Studio Lokalsi" build is not upgraded in place; uninstall
  it by hand. Data is shared because the app home did not move.
- A later rename of the technical identifiers needs a migration of the app home and the
  credential store entries and is a separate decision.

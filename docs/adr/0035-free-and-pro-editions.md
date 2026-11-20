# ADR 0035: Free open-source Lokalomat and closed-source Lokalomat Pro

- Status: accepted (owner, 2026-11-19: two editions; public MIT repository plus a private Pro
  repository; Pro is a separate installer unlocked by a licence key).
- Date: 2026-11-19

## Context

The owner wants a free Lokalomat with the scope of LM Studio (text models, decision models,
local API server, Android app) and a paid Lokalomat Pro with every feature. The free edition
stays open source under MIT. Until now all modes live in the same crates and are always
compiled in; the only Cargo features are `server` and `client` in `lokalsi-api-gateway`.

## Decision

1. **Two repositories.** This one stays public and MIT. A private repository
   `lokalomat-pro` pins this one (submodule at a release tag), adds the Pro crates and builds
   its own installer. Pro code never enters the public history.
2. **Scope.** Free: chat, models, llama.cpp engine, decision mode (Expert), local OpenAI-compatible
   server with `/v1/systemone`, sharing in the network, the Android client (chat and decisions),
   updates. Pro adds images, speech and transcription, voices, music, gallery, pipelines, MCP
   client and server, agents, licence activation.
3. **Android stays one MIT app.** Its tabs follow the modes the computer offers
   (`api.info().modes`), so the free server shows chat and decisions and a Pro server unlocks
   the rest.
4. **Separate installer and licence.** Pro has its own product name, identifier
   (`pl.lokalsi.studio.pro`), updater endpoint and signing key, and shares the data folder
   with the free edition so users can upgrade without losing data. The licence is a file
   signed with Ed25519 and checked offline (Pro repository only).
5. **Edition seam.** `lokalsi_core::edition::Edition` tells which edition runs: the free app
   answers `Free`, the Pro extension `Pro` (`Extension::edition`). The UI reads it through
   `get_edition`. Until the answer
   arrives the UI assumes Pro, so the paid app never flashes the free banner.
6. **Pro banner.** The free edition shows one short line under the header with a link to
   `PRO_URL` (`ui/src/lib/external.ts`, now aitransformation.pl, later the purchase page).
   It can be closed and comes back after 30 days.

7. **UI registry.** An edition registers its views, Server tabs, start-up work, dialogs and
   tool-call cards in `ui/src/lib/edition/registry.ts`, from its entry module
   (`ui/src/edition/free.ts` or `pro.ts`). The `@edition` alias in `vite.config.ts` picks the
   entry (`VITE_EDITION=free`; the default is the full one until the Pro code has its own
   repository). Views are loaded with dynamic imports, so the free bundle has no Pro view.
   The Workshop is a frame (`WorkshopView.svelte`: tabs, output area, command bar) plus one
   panel per mode (`views/workshop/*Panel.svelte`) that registers itself in the frame through
   `workshop.attach`; chat and decisions belong to `core.ts`, the other modes to `pro.ts`.
   `npm run dev:ui -- --mode free` (or `VITE_EDITION=free`) runs the free edition.
   The Pro IPC commands and events are in `lib/ipc.pro.ts` (`proIpc`, `PRO_EVENTS`), their
   browser stand-ins in `lib/mock.pro.ts` (registered by the Pro entry), and the Pro job
   starters in `lib/jobs.pro.ts`; the shared `jobs` store keeps the list, memory meter and
   decisions. A `--mode free` bundle contains no Pro command name.

8. **Desktop shell.** The free shell (`apps/desktop/src-tauri`) knows only the `Extension`
   trait (`ext.rs`); Pro IPC commands are not a Tauri plugin (that would rename every
   `invoke`) but are routed by name: the free handler answers its own commands and hands the
   rest to `Extension::invoke`. The Pro extension lives in `src/pro/` behind the `pro` cargo
   feature (default on until the code moves to the private repository) and owns the media
   engines, pipelines, MCP client and agents; `--no-default-features` builds the free app
   without `lokalsi-mcp` and `lokalsi-agents`. `AppState` keeps the job queue and the free
   `ModeRunner`; the Pro state is reached with `state.pro()`.

9. **Crates.** The media engines (whisper.cpp, stable-diffusion.cpp, the Python workers,
   audio decoding, media jobs, media modes, pipelines) are in `lokalsi-engines-media`; the
   free `lokalsi-engines` keeps llama.cpp, the installer, the VRAM scheduler, the job queue and
   `ModeRunner`. `lokalsi-api-gateway` has a `pro` feature (default on) for the phone's media
   routes, the server's media endpoints, the MCP server, thumbnails and the Pro half of the
   backends (`backend_pro.rs`); without it the gateway serves chat and decisions only. The
   desktop `pro` feature turns on `lokalsi-engines-media` and the gateway `pro` feature, so
   `cargo tree -p studio-lokalsi --no-default-features` has no Pro crate. Tests of the
   gateway's server extension run with the `pro` feature (the default).

10. **Pro app and crates.** Pro code no longer needs the inside of the free crates: the
    gateway's Pro half is the crate `lokalsi-api-gateway-pro` (on the free gateway's public
    API), and the Pro desktop app is its own package `apps/desktop-pro` (`studio-lokalsi-pro`:
    product "Lokalomat Pro", identifier `pl.lokalsi.studio.pro`, UI from `ui/dist-pro`). Both
    apps call `studio_lokalsi_lib::run_with(context, extension)` with their own
    `tauri::generate_context!()`. The free UI is the default (`--mode pro` builds the Pro UI);
    the free desktop package has no `pro` feature any more. The Pro UI is `apps/desktop-pro/ui`
    (entry `src/edition/pro.ts`, views, panels, stores, mocks, tests, and its own texts in
    `locales/{pl,en}.json`, added with `registerMessages`); it reaches shared code through the
    `@free/*` alias and the free UI never imports it, so the free bundle and `ui/locales`
    contain no Pro view or Pro text (the error texts of Pro codes, `errors.*`, stay shared). Pro releases get their own
    updater endpoint (`roxlukas/lokalomat-pro`); the Pro signing key is still to be made.

11. **Two repositories.** The Pro code left this repository: `crates/{mcp,mcp-bridge,agents,
    engines-media,api-gateway-pro}`, `apps/desktop-pro` (app and UI), the Python worker,
    the agent catalog and the Playwright tests are in the private `lokalomat-pro`, whose
    Cargo workspace and UI project build on this repository as the submodule
    `vendor/lokalomat` (path dependencies on `vendor/lokalomat/crates/*`, Vite alias `@free`).
    This repository builds, tests and releases the free app on its own and has no Pro crate,
    view or text. The manifest data of the Pro engines (`catalog/engines/*.json` with their
    constants in `lokalsi-engines`) and the media models in `catalog/models.json` are still
    here; moving them is a later step.

12. **One catalog, filtered by edition.** `catalog/models.json` stays one large list of public
    models. When the app starts, the desktop shell keeps only the kinds of models of the
    running edition (`Edition::modalities`, `Catalog::retain_modalities`): the free app offers
    chat and decision models (17 of 32), Pro everything. The Models view and the wizard show
    only the kinds the catalog has, and the browser preview filters the same way. The model
    names of Pro are therefore still in this repository's data (public Hugging Face models);
    only the free app does not offer them.

## Steps

1. This change (milestone 1): edition seam and banner.
2. Extension seams without moving behaviour: a mode-provider trait in `lokalsi-engines`,
   Pro modules split into their own crates, Pro Tauri commands as a plugin, a view/mode
   registry in the UI (`WorkshopView` split into panels).
3. The private repository and the Pro build.
4. Licence keys and activation.

## Consequences

- Code that already is in this public history stays MIT. If the owner needs Pro code that was
  never public, a clean repository is required; the value of Pro is then new features,
  signed builds, updates and support.
- Selling Pro is commercial use: engines and models whose licence is non-commercial
  (`License`/`Commercial` in the catalogs) must be left out of Pro or checked first.
- The name "Lokalomat Pro" and the logo are not covered by the MIT licence.
- A check that this repository contains no Pro crate names would change the quality gate and
  needs its own pull request approved by the owner.

# ADR 0031: End-to-end tests (M6, part 3)

- Status: accepted (owner, 2026-11-18: M6 in three PRs).
- Date: 2026-11-18

## Context

PRD M6 asks for Playwright tests of the UI and integration tests of the engines on
small models. GitHub CI is off (no billing), so these tests run by hand and are **not**
part of the quality gate (changing the gate needs the owner's approval).

## Decision

1. **UI tests**: `@playwright/test` and `@axe-core/playwright` in `ui/e2e/`, run with
   `npm run e2e`. They open the real UI in Chromium against the browser-only mock backend
   (`npm run dev:ui`, started by the Playwright config):
   - the first-run wizard: keyboard focus stays inside, the language can be switched, it
     can be skipped;
   - Basic and Expert hide and show the advanced views; every view opens;
   - the skip link, and the command palette (Ctrl+K, Escape);
   - empty states and the update settings;
   - **axe (WCAG 2.2 AA and best practices) on every view, in both themes**, so the
     result of ADR 0030 cannot regress.
   Browsers are not committed: `npx playwright install chromium` once, with
   `PLAYWRIGHT_BROWSERS_PATH` to keep them off the system drive.
2. **Engine tests**: `scripts/e2e-engines.mjs <dir>` (`npm run e2e:engines -- <dir>`) runs
   the existing acceptance examples on a clean folder with the smallest models (chat
   LFM3 1.7B, decision Basal 2.0 mini):
   - `m5b_acceptance`: chat and decision side by side, gated pipeline;
   - `m5c_server`: the OpenAI-compatible server (key check, streaming, tool call).
   llama.cpp is installed by the app's own code and the models are downloaded and
   checked with sha256 (about 2.5 GB, reused between runs). An example's exit code is the
   result. We reuse the examples instead of a second copy of the setup code.
3. The tests are run before a release, with the results in the release notes. Adding them
   to the gate or to CI is the owner's decision.

## Findings

The first UI run found a real problem: the Engines view scrolls but, with no engine to
act on, had nothing a keyboard could reach (axe `scrollable-region-focusable`). The view is
now a focusable region with a label.

## Verification

NIAGARA (RTX 3090, 24 GB budget), clean folder on drive E, `npm run e2e:engines`:

- UI: 11 of 11 tests pass in Chromium (Playwright 1.65).
- `m5b_acceptance` passed: chat model (LFM3 1.7B Q4_K_M) and decision model (Basal 2.0
  mini Q8_0) loaded side by side, the gated pipeline ran.
- `m5c_server` passed on a fresh home: llama.cpp installed by the app, both models
  downloaded and sha256-checked (1.0 GB and 1.6 GB, about one minute together). No key gave
  401; the first streamed request took 27 s (it loads the model); a warm one took 1 s; a tool call
  (`get_weather`) returned in 0.7 s; a decision gave P(spam) = 0.98; the Workshop and the
  server answered at the same time; the server was not reachable from the LAN address.
- Not run here: the RTX 2070 and the NUC; the examples are the same ones those machines
  ran in M5b and M5c.

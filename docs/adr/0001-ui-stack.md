# ADR 0001: UI stack – Svelte 5 + Vite SPA, TypeScript 6

- Status: accepted
- Date: 2026-11-11

## Context

PRD §4.1 recommends Svelte 5 and leaves the choice open (§14.2). The UI runs in a
Tauri WebView, offline, with no server-side rendering.

## Decision

- Svelte 5 (runes) + Vite 8 as a plain SPA. No SvelteKit: no SSR/routing needs, and
  a hash router of a few lines (`ui/src/lib/router.svelte.ts`) is enough.
- TypeScript **6.0**, not 7.0: `svelte-check` 4.7 supports TypeScript `^5 || ^6` only.
  Revisit when svelte-check supports TS 7.
- Own i18n (`t()` over `ui/locales/{pl,en}.json`, dot keys, `{param}` placeholders)
  instead of a library – tiny, offline, and a test enforces identical keys in both files.
- Icons: `@lucide/svelte` (ISC), imported per icon (`@lucide/svelte/icons/<name>`).
- Tauri **2.12** (stable). Tauri 3 is in alpha and is not used.

## Consequences

Smallest bundle and dependency surface. Switching to SvelteKit later is possible
but not expected.

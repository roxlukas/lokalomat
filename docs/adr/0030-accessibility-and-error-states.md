# ADR 0030: Accessibility audit and error states (M6, part 2)

- Status: accepted (owner, 2026-11-18: M6 in three PRs).
- Date: 2026-11-18

## Context

PRD §9 asks for full keyboard use, WCAG AA contrast, ARIA labels and
`prefers-reduced-motion`. Most of it was built in with the views. M6 checks it and
closes the gaps.

## Method

axe-core 4.14 (`wcag2a`, `wcag2aa`, `wcag21aa`, `wcag22aa`, best-practice) run in the
browser on the UI with its mock backend: every view, Basic and Expert, light and dark
theme. Results are read once the theme change has finished; axe reads colours in the middle
of the theme transition and reports false contrast errors otherwise.

## Findings and changes

- axe: **no violations** in any view or theme. The only open items are contrast checks
  axe cannot compute over the gradient background (titles and the memory meter); the
  colours are the design tokens, checked by hand.
- **First-run wizard**: Tab moved focus into the page behind the full-screen wizard, where
  nothing is visible. The page (skip link and shell) is now `inert` while the wizard is
  open. Dialogs made with `<dialog>.showModal()` (command palette, voice editor) were
  already modal.
- **Crashes in a view**: errors were logged, but a failing view left a blank screen. A
  `<svelte:boundary>` around the views now shows "Coś poszło nie tak w tym widoku" with
  a "Spróbuj ponownie" button and writes the error to the log.
- **Empty states** were checked for every view (Gallery, Voices, Pipelines, Tools,
  Models, Workshop): each has a short explanation and, where it helps, the next step. No
  change needed.

## Not covered

- Screen readers (NVDA, VoiceOver) were not run; ARIA roles and labels are checked by axe
  only.
- The axe run becomes an automated check in the E2E tests (M6 part 3, Playwright).

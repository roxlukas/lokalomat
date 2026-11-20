# ADR 0010: Visual design from aitransformation.pl

- Status: accepted
- Date: 2026-11-11

## Context

The product owner asked to bring the design of <https://aitransformation.pl/controlled-ai.html>
into the app (PRD §14.1 left the visual identity open; §9.5 requires CSS tokens, a dark
variant and no external CDNs).

## Decision

- Palette taken from the site: dark slate (`#0F172A` background, `#1E293B` cards,
  `#334155` borders, `#F8FAFC` / `#94A3B8` text) with the purple accent `#C084FC`
  (dark text on purple); the light theme uses the site's light colours (`#7E22CE`
  accent, `#4F46E5` focus, `#F1F5F9` / `#FFFFFF` surfaces). Semantic colours
  (ok/busy/error/warn/info) stay separate from the accent, as PRD §9.1 requires.
- Shapes: pill buttons and chips, 12 px cards, translucent header with blur, a soft
  purple glow behind the content.
- Type: Caviar Dreams for page titles (`h1`) and the app name, Inter for text,
  JetBrains Mono for code – all bundled in the app (offline). Inter and JetBrains Mono
  come from `@fontsource` (SIL OFL 1.1). The two Caviar Dreams files are copied from
  aitransformation.pl (`ui/src/assets/fonts/`); **the owner confirms the font licence
  allows embedding it in a distributed app** – otherwise swap `--font-display` to Inter.
- The app keeps its own logo (Studio Lokalsi), recoloured to the accent.

## Consequences

All components already use tokens, so the change is confined to `tokens.css`,
`base.css`, `fonts.css` and a few component styles.

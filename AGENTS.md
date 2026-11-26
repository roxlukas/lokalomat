# Lokalomat AI Studio (formerly Studio Lokalsi)

Local-first desktop AI studio (Tauri 2 + Rust + Svelte 5) for Windows, macOS and Linux (Ubuntu 24.04/26.04, ADR 0011). Requirements: `PRD.md` (incl. §15 additions).
Work milestone by milestone (PRD §13); don't start the next one until the current
one's acceptance criteria pass. Deviations from the PRD → short ADR in `docs/adr/`.

Additions beyond the PRD requested by the product owner:
- diagnostic logging with Normal / Verbose modes (ADR 0004) – every new module logs via `tracing`;
- local (stdio) and remote (HTTP) MCP servers with a configuration UI, reference server `mcp-grafana.exe` (milestone M2b, ADR 0012, crate `crates/mcp`);
- **plug and play like LM Studio**: engines are downloaded, installed and started automatically when a mode needs them – the user never has to visit the Engines view first (it stays for advanced use);
- **Basic / Expert mode** (ADR 0013): Basic (default) hides everything too hard for an average user; new advanced UI goes behind `settings.value.expert` or `expert: true` in `views.ts`;
- **short, plain-language UI texts**: one short sentence, everyday words, no technical detail in explanations (e.g. "Silniki uruchamiają modele sztucznej inteligencji na Twoim komputerze."). Technical details go to tooltips, logs or ADRs.
- **Editions** (ADR 0035): this repo is the free MIT Lokalomat. Lokalomat Pro is closed source in the private repo `roxlukas/lokalomat-pro`, which includes this repo as the submodule `vendor/lokalomat`. `lokalsi_core::edition::Edition` says which one runs; the free edition shows a closable Pro banner (`ProBanner.svelte`, link `PRO_URL` in `ui/src/lib/external.ts`). Never add Pro-only code, Pro texts or Pro engine/agent code to this repo; extensions go through `Extension` (desktop), `GatewayExtension`/`ServerExtension` (gateway) and the registry in `ui/src/lib/edition/registry.ts`.

## Quality gate

Every change passes the quality gate before it reaches a pull request — details and
what to do when it fails: [docs/quality-gate.md](docs/quality-gate.md).

- Enable the git hook once per clone: `sh scripts/enable-hooks.sh` (`git config core.hooksPath` must print `.githooks`).
- Each commit runs `scripts/quality-gate.sh commit`; before a PR run `sh scripts/quality-gate.sh pr`.
- Never bypass it (`--no-verify`, disabling the hook, weakening checks). Changing the gate is a separate PR approved by the owner.
- `AGENTS.md`, `CLAUDE.md` and `GEMINI.md` must stay identical — change all three in the same commit.

## Commands

- `npm run tauri dev` – run the desktop app (starts Vite on :5173)
- `npm run dev:ui` – UI only in a browser (IPC falls back to localStorage + `ui/src/lib/mock.ts`)
- Android client (M5, ADR 0020): `apps/mobile`, UI entry `ui/mobile.html`. Build: set `JAVA_HOME` (Android Studio `jbr`), `ANDROID_HOME`, `NDK_HOME` (NDK 29), then in `apps/mobile`: `..\..
ode_modules\.bin	auri android build --debug --apk --target aarch64` (npm in PowerShell drops the flags).
- `cargo run --release -p lokalsi-engines --example kv_compact -- <dir> <model.gguf> <estimate MB>` – load a model with the plan for a small card (`BUDGET_MB`, default 7680 = 8 GB) and check the compact KV cache (ADR 0021).
- `cargo run --release -p lokalsi-engines --example gpu_offload -- <dir> <model.gguf> <estimate MB>` – GPU offload (`-ngl`) and the model's sampling defaults on real hardware: GPU memory grows with the layers, temperature 0 repeats (ADR 0036; `ENGINES`, `SEED_DB`, `BUDGET_MB`, `TOKENS`).
- `node scripts/release.mjs build` (on Windows and on Linux) then `node scripts/release.mjs publish <notes.md> <dirs>` – signed release builds and a draft GitHub release with `latest.json` for the updater (`TAURI_SIGNING_PRIVATE_KEY`, `_PASSWORD`; ADR 0029). Step by step, with the checks: [docs/releasing.md](docs/releasing.md).
- `cargo run -p catalog-sync` – fill sizes/sha256/revisions in `catalog/models.json` (HF API), sizes/sha256 of engine builds in `catalog/engines/*.json` (GitHub API digests) and check `benchmark` mappings (ADR 0008/0009); `-- --check` verifies offline
- `cargo test -p lokalsi-core --test hf_live -- --ignored` – live download + sha256 check against huggingface.co (~42 MB)
- `cargo run -p lokalsi-hwprobe --example probe` – print the detected hardware profile and budget
- Test data dir: set `STUDIO_LOKALSI_HOME` to a scratch folder to get a fresh `settings.json`/`app.db`/first-run wizard.
- `npm test` / `npm run check` – vitest / svelte-check (warnings fail)
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`
- Clippy/tests of `studio-lokalsi` need `ui/dist` (run `npm run build:ui` first).
- Verbose logs at startup: `--verbose` or `STUDIO_LOKALSI_LOG=verbose`. Logs: `%LOCALAPPDATA%\Studio Lokalsi\logs`.

## Conventions

- Code identifiers, file names, commits: English. UI strings only via `t()` with keys in both `ui/locales/pl.json` and `en.json` (a test enforces parity).
- Logic in `crates/*`, `apps/desktop/src-tauri` stays a thin IPC layer.
- Never invent sha256 sums, file sizes or model URLs – fetch from the Hugging Face API or leave a TODO.
- Never log prompt/result contents directly: use `logging::loggable_content`; secrets go through `redact`.
- Model output is untrusted: render it only through `ui/src/lib/markdown.ts` (DOMPurify) and open links in the system browser.
- Pin TypeScript to 6.x until svelte-check supports 7 (ADR 0001).
- Visual design follows aitransformation.pl (ADR 0010): colours, radii and fonts only via tokens in `ui/src/styles/tokens.css`; fonts are bundled, never loaded from a CDN.
- New DB schema → append a migration to `MIGRATIONS` in `crates/core/src/db.rs`; never edit released entries (ADR 0003).
- M1+ commands return `CmdError { code, message }`; the UI translates `code` via `errors.*` (`ui/src/lib/feedback.ts`).
- Product name (ADR 0019): "Lokalomat AI Studio", short "Lokalomat" in UI texts; technical ids (`lokalsi-*`, app home `Studio Lokalsi`, keyring service) keep the old name.
- Next free ADR number: 0040 (0039 phone chat list and red/yellow UI; 0038 chat on the phone itself; 0037 phone parity part 2 – gateway settings; 0036 model parameters and send key; 0035 free/Pro editions; 0023 used by the media converters branch; 0027 by phone parity; 0028 by M5c agents part 3).

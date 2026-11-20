# ADR 0028: Hermes Agent, OpenClaw and Droid (M5c, part 3)

- Status: accepted (owner, 2026-11-17: the seven agents from ADR 0026; Droid stays,
  marked closed source with a possible Factory login).
- Date: 2026-11-17

## Context

ADR 0026 added the agents catalogue, separate profiles and the "Agenci" tab with Goose,
OpenCode, Pi and Cline. Part 3 adds the last three agents. Each one differs from the
first four:
- **Hermes Agent** (Nous Research, MIT) is a Python program and needs a context window of
  at least **64,000 tokens**. Below that it stops at start with an error.
- **OpenClaw** (MIT) is an npm package with install scripts. It runs a local
  "gateway" with its own web panel ("Control UI").
- **Droid** (Factory) is **closed source**. It is one binary per platform, and its
  settings folder `~/.factory` cannot be moved.

## Decision

1. **A larger window for agents.** The server loaded models with at most 32k tokens
   (`MAX_TOOLS_CTX`, ADR 0014/0021). Server loads now use `ModeRunner::server_load_request`:
   the largest window that fits, up to **64k** (`MAX_SERVER_CTX = 65536`), with the compact
   KV cache when that is what makes it fit.
   - Chat in the Workshop keeps 32k.
   - A context size set in the settings still wins.
   - "Uruchom" refuses an agent whose `needs.min_context` is above the window this computer
     gives. The error is `agent_context_small`: "This agent needs a stronger graphics card
     or a smaller model."
   - The cards of agents that need more than 32k say they need a strong graphics card.
2. **Hermes Agent 0.23.2**: new install kind `uv`. Hermes runs from its source folder:
   - its PyPI package stops at 0.19;
   - building a wheel is blocked on purpose;
   - its own installer is a `curl | bash` script that also fetches Node.js, Chromium and
     other tools;
   - its direct dependencies only fit together through the project's `uv.lock`
     (a plain `uv tool install` fails on pillow).

   Lokalomat does what that lock is for:
   - It downloads GitHub's archive of the **release commit** (`ae2a81cc…`, tag `v0.23.2`),
     checks its sha256 from the catalogue, and unpacks it to `<data>/agents/hermes/src`.
   - It runs `uv sync --frozen --no-dev --python 3.14` there with the private uv
     (ADR 0018). uv checks every package against the lock's hashes.
   - The environment, Python and cache are in `<data>/agents/hermes`
     (`UV_PROJECT_ENVIRONMENT`, `UV_PYTHON_INSTALL_DIR`, `UV_CACHE_DIR`,
     `UV_PYTHON_PREFERENCE=only-managed`), never in the user's own uv. The cache is cleaned
     after the install, and a new version replaces the whole folder.
   - Profile: `HERMES_HOME=<data>/agents/profiles/hermes`. The provider is written with
     Hermes' own `hermes config set model.*` (provider `custom`, `base_url`, `default`,
     `context_length`), so the rest of its `config.yaml` stays as Hermes keeps it. The key
     goes in as `${LOKALOMAT_API_KEY}`, which Hermes reads from the environment. The five
     commands take about 6 s each before the window opens.
3. **OpenClaw 2026.11.5**: npm with the private Node.js 26 (ADR 0026). npm 11.20 runs a
   package's install scripts only when allowed. For a `--prefix` install it refuses the
   `--allow-scripts` flag and reads `allowScripts` from that folder's `package.json`, so
   Lokalomat writes `{"allowScripts": {"openclaw": true}}` there (new catalogue flag
   `allow_scripts`). Other packages' scripts stay off.
   - Profile: `OPENCLAW_STATE_DIR` and `OPENCLAW_CONFIG_PATH` point into its profile folder.
     `openclaw.json` gets:
     - provider `lokalomat` (`openai-completions`, key `${LOKALOMAT_API_KEY}`, the window
       and output limit);
     - the default model `lokalomat/<model>`;
     - `localModelLean` (fewer optional tools for local models);
     - the gateway: `mode: local`, loopback, token auth.
     The agent's other settings stay.
   - **Its own port 47823**, not OpenClaw's usual 18789, so the user's own OpenClaw keeps
     working.
   - Update checks and the usage count are off (`update.checkOnStart`,
     `telemetry.enabled`, `OPENCLAW_NO_AUTO_UPDATE`, `DO_NOT_TRACK`).
   - **Start**: the gateway runs in its own terminal window, as with the other agents.
     Lokalomat waits up to 2 minutes for its port, asks
     `openclaw dashboard --no-open --json` for the panel address, and opens it in the
     browser. The address carries a single-use sign-in. Only a `127.0.0.1` / `localhost`
     address is accepted.
   - The gateway token is made once (`getrandom`) and kept in the profile folder.
   - The catalogue describes this generally (`launch.web`: port, command, JSON field).
4. **Droid 1.13.0**: an engine manifest `catalog/engines/droid.json`.
   - Factory publishes a binary and a `.sha256` file per version and platform under
     `downloads.factory.ai/factory-cli/releases/<version>/`. `catalog-sync` reads those
     files (new `checksums: "per-file"` for `url` sources). The installer copies a file
     that is not an archive as a program.
   - About 320–360 MB per platform. Auto-update is off
     (`FACTORY_DROID_AUTO_UPDATE_ENABLED=false`), so the catalogue decides the version.
   - **Profile**: Droid has no variable for its folder. As the owner decided, Lokalomat
     **adds one model "Lokalomat"** to the user's `~/.factory/settings.json` (`customModels`,
     `generic-chat-completion-api`, key `${LOKALOMAT_API_KEY}`, output and input limits).
     - Before the first change it copies the file to `settings.json.lokalomat-backup`.
     - Other models and settings stay.
     - "Usuń" removes the Lokalomat model again; the backup stays.
     - The card says so (`needs.user_settings`).
   - The model is chosen as `custom:Lokalomat-<index>`.
   - The card shows **closed source** and that a Factory account may be needed. The
     public builds may ask to sign in even with custom models. Only Factory's enterprise
     "airgap" build is documented to skip it. Factory also sends usage metrics, with no
     documented switch to turn them off. Lokalomat does not try to get around either.
5. **Launch arguments** may use `{model}` (the agent's own name for the model), `{cwd}`,
   `{profile}` and `{prompt}`. The prompt is filled in last, so braces in it stay as they
   are.

## Measurements

`crates/api-gateway/examples/m5c_agents.rs` (ADR 0026): a clean folder, a key per agent,
and one task "create hello.txt with the word Lokalomat", run non-interactively. For the
test the home folder pointed into the test folder, so Droid's settings change landed
there.

NIAGARA (Windows 11, RTX 3090), Qwen3.5 9B Q6_K. The server now loads it with **65,536
tokens** (it was 32,768):

| Agent | Install | Task | Result |
|---|---|---|---|
| Hermes Agent | 151 s (source + `uv sync`, Python 3.14) | 38 s | ok |
| OpenClaw | 187 s (Node.js already there, npm) | 49 s | ok |
| Droid | 26 s (one 345 MB file) | 109 s | ok, **without a Factory sign-in** |

Found on the way:
- **Hermes**:
  - A plain `uv tool install` of the source fails: its own pins conflict on pillow without
    the lock. Building a wheel is blocked by its `setup.py`. Hence the source folder and
    `uv sync --frozen`.
  - With only the environment variable, the key reached the server as 401. When it runs a
    task, Hermes resolves `${VAR}` from **its own `.env`** in `HERMES_HOME`, not from the
    process. The key is now also written there: the line `LOKALOMAT_API_KEY=…`, mode
    `0600` on Unix; the file's other lines stay.
- **OpenClaw**:
  - npm 11.20 (Node 26.13.1) stops with `EALLOWSCRIPTS` when `--allow-scripts` is given
    with `--prefix`. Hence `package.json`.
  - With `reasoning: false`, OpenClaw wrote the file but then failed the turn: Qwen3.5
    "returned only reasoning". The Lokalomat model is now marked `reasoning: true`, as
    OpenClaw's own message advises.
  - **The gateway starts slowly on Windows**: about **4 minutes** the first time (plugin
    metadata, state), 46 s the next time. So "Uruchom" opens the window and returns at
    once. The app then waits in the background, up to 10 minutes, for `GET /` on the
    gateway to answer, asks for the panel address and opens the browser. The card text
    says the panel follows in a few minutes.

In the real app (`npm run tauri dev`, driven through WebView2 CDP), "Uruchom" on OpenClaw:
- installed it and opened its window;
- had the panel ready after 46 s and opened it in the browser, already signed in, with
  "Qwen3.5 9B Q6_K" as the model.

A message sent in the panel was answered through the Lokalomat server with key "Agent
OpenClaw". The prompt was 16,562 tokens of OpenClaw's own instructions and tools, which
is why it needs a long window. The first answer took 2 min 13 s, of which 125 s was
loading the model.

srwpodbiurkiem2 (Windows 10, RTX 2070 8 GB, 16 GB RAM), Qwen3.5 9B Q4_K_M, 900 s limit.
The server loaded the model with only **16,384 tokens** (the 8 GB card cannot hold a
longer window):

| Agent | Install | Task | Result |
|---|---|---|---|
| Hermes Agent | 130 s | 49 s | **FAIL, refused at start**: it needs 64,000 and the server serves 16,384 |
| OpenClaw | 193 s | 132 s | ok |
| Droid | 9 s | 79 s | ok, without a Factory sign-in |

NUC (Ubuntu 24.04, CPU only, about 3 tok/s), Qwen3.5 4B Q4_K_M, 1800 s limit. The server
loaded it with **65,536 tokens**:

| Agent | Install | Task | Result |
|---|---|---|---|
| Hermes Agent | 13 s | 782 s | ok |
| OpenClaw | 28 s | 639 s | ok |
| Droid | 9 s | 705 s | ok, without a Factory sign-in |

For both runs the home folder (`USERPROFILE` / `HOME`) pointed into the test folder; the
real `~/.factory` was not touched. No background helpers were left behind.

## Not in this ADR

- Hermes' and OpenClaw's own extras (messaging channels, browser tools, voice). They
  work if the user sets them up in the agent; Lokalomat only sets up the model.
- Droid's desktop app and Hermes' desktop packages: only the terminal programs are
  installed.

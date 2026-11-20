# ADR 0026: Third-party agents (M5c, part 2)

- Status: accepted (owner, 2026-11-17: Goose, OpenCode, Pi, Cline, Hermes Agent,
  OpenClaw and Droid; started in their own window; a separate profile per agent; three
  PRs). This PR covers Goose, OpenCode, Pi and Cline. Hermes, OpenClaw and Droid follow
  in part 3.
- Date: 2026-11-17

## Context

PRD §15 point 11: Lokalomat installs and starts third-party agents that use it as their
inference server (the local server, ADR 0025). It should be plug and play: download,
address and key set up by the app, start with one click.

The owner chose two things:
- **Separate profiles.** Lokalomat never reads or changes the user's own agent setup
  (`~/.config/goose`, `~/.pi`, `~/.cline`, OpenCode's config).
- **Each agent runs in its own window.** The agents are not embedded in Lokalomat.

## Decision

1. **Catalogue** `catalog/agents.json` (`lokalsi_agents::catalog`). For each agent:
   - id, name, publisher, homepage, licence (with an `open_source` flag) and short pl/en
     descriptions;
   - how it is installed and how it is started;
   - a non-interactive `check` command ("do one task and exit");
   - its needs: tools, smallest context window, whether a maker's account may be needed.
2. **Installation**:
   - **Binary agents (Goose 1.59.2, OpenCode 1.19.6)** are engine manifests
     (`catalog/engines/goose.json`, `opencode.json`). They take the CLI build of the
     official GitHub release, check sha256 (GitHub asset digests through
     `catalog-sync`), unpack it and record it like any engine, so an update means a new
     version in the catalogue.
   - **npm agents (Pi 1.5.2, Cline 3.0.74)** use a **private Node.js 26.13.1 LTS**
     (`catalog/engines/node.json`). It comes from nodejs.org and is checked against its
     `SHASUMS256.txt`, through a new `url` install source that `catalog-sync` also
     fills. Versions are pinned exactly and installed with `npm install --prefix
     <data>/agents/<id>`, **never globally**. npm checks package integrity against the
     registry.
3. **Separate profiles** (`lokalsi_agents::profile`). Every agent gets its own folder
   `<data>/agents/profiles/<id>`, selected through the agent's own variable:

   | Agent | Folder | Server address, key, model |
   |---|---|---|
   | Goose | `GOOSE_PATH_ROOT` | environment only: `GOOSE_PROVIDER=openai`, `OPENAI_HOST`, `OPENAI_BASE_PATH`, `OPENAI_API_KEY`, `GOOSE_MODEL`, `GOOSE_CONTEXT_LIMIT`; keyring and telemetry off |
   | OpenCode | `OPENCODE_CONFIG_DIR` | `opencode.json` with provider `lokalomat` (`@ai-sdk/openai-compatible`), key as `{env:LOKALOMAT_API_KEY}`, auto-update off |
   | Pi | `PI_CODING_AGENT_DIR` | `models.json` (provider `lokalomat`, key `${LOKALOMAT_API_KEY}`), `settings.json` default provider and model |
   | Cline | `CLINE_DIR` | `cline auth --provider openai --apikey … --modelid … --baseurl …` once before starting |

   - The key stays out of files wherever the agent can read it from the environment.
     Cline saves its own copy in its profile folder.
   - Settings the agent saved itself in its folder are kept. Only the Lokalomat provider
     and the default model are rewritten.
4. **Keys and model**:
   - Each agent gets **its own key** (`api_keys.agent = <id>`), made on its first start
     and kept in the OS credential store. Revoking it in the Server view cuts off only
     that agent.
   - The model is the one chosen in the "Agenci" tab. Only installed chat models with
     the catalogue feature `tools` are offered, else the default model.
   - The context window passed to the agent is what the server loads it with (ADR 0025).
5. **Starting** (`lokalsi_agents::launch`):
   - A start script in the profile folder sets the variables, the private Node.js in
     `PATH` and the working folder (chosen in the tab, default Documents), then runs the
     agent's text interface.
   - The window: `cmd /k` on Windows, Terminal on macOS (`.command`), and on Linux the
     first of `x-terminal-emulator`, gnome-terminal, konsole, xfce4-terminal and xterm.
   - On Unix the script is `0700`, because it holds the key. On Windows, values that
     cannot be quoted safely in a `.cmd` file (`"`, `%`, line breaks) are refused.
   - Starting an agent switches the server on (127.0.0.1) when it is off.
6. **UI**: the "Agenci" tab of the Server view (Expert).
   - It has the model for agents and the work folder.
   - Each agent has a card with its description, licence, version, install progress
     and the buttons Zainstaluj / Uruchom / Usuń. Removing an agent keeps its profile
     (its sessions).
   - A closed-source licence and a possible maker account are shown on the card.

## Measurements

`crates/api-gateway/examples/m5c_agents.rs`: the real server, a clean folder, a key per
agent and one task that needs a tool: "create hello.txt with the word Lokalomat". It is
run non-interactively with the agent's `check` command, and passes when the file exists.

NIAGARA (Windows 11, RTX 3090), Qwen3.5 9B Q6_K, context 32k:

| Agent | Install | Task | Result |
|---|---|---|---|
| Goose | 2 s | 109 s | ok – `write hello.txt` |
| OpenCode | 2 s | 101 s | ok (after the `PWD` fix below) |
| Pi | 83 s (Node.js + npm) | 3 s | ok |
| Cline | 112 s (npm, native binary) | 8 s | ok |

The first OpenCode run wrote `hello.txt` into the folder the test was started from, not
into its work folder. OpenCode (Bun) takes its folder from `PWD`, which a parent shell
had set. Both the start script and `check` now set `PWD` to the work folder.

srwpodbiurkiem2 (Windows 10, RTX 2070 8 GB), Qwen3.5 9B Q4_K_M, the binary built on
NIAGARA:

| Agent | Install | Task | Result |
|---|---|---|---|
| Goose | 3 s | 186 s | ok |
| OpenCode | 4 s | timed out after 900 s | file written correctly, but the agent did not end: 46 short requests (≈ 0.3 s each) after the file, until the timeout |
| Pi | 67 s | 4 s | ok |
| Cline | 90 s | 14 s | ok |

With the 4-bit quantisation, OpenCode loops after the task is done. With Q6_K on the RTX
3090 it ended after three requests. In the interactive window the user can stop it. A
model with steadier tool calling (Q6_K or larger) is the better choice for OpenCode.

NUC (Ubuntu 24.04, no graphics card, Vulkan iGPU, ≈ 3 tok/s), Qwen3.5 4B Q4_K_M,
30-minute limit per task:

| Agent | Result |
|---|---|
| Goose | wrote `hello.txt`, but did not end within 30 minutes |
| OpenCode | nothing within 30 minutes |
| Pi, Cline | **installation failed**: `node not found in the downloaded archives` |

- **Fixed:** Node.js keeps its program in `bin/` on Linux and macOS
  (`node-v26.13.1-linux-x64/bin/node`), and `install::find_binary` only looked one folder
  deep. It now also looks in `bin/` (tested).
- **Expected:** agents send long prompts with tool descriptions, which is too much for a
  computer without a graphics card. The "Agenci" tab warns about it there ("Bez karty
  graficznej agenci działają bardzo wolno").

In the real app (NIAGARA, `npm run tauri dev`, driven through WebView2 CDP), the
"Agenci" tab showed the four cards, and "Zainstaluj" on Pi showed progress and then
version 1.5.2.

"Uruchom" did the following:
- switched the server on by itself and created the key "Agent Pi";
- offered only the tool model (Qwen3.5 9B Q6_K);
- opened `cmd /k start.cmd` with Pi running from the private Node.js, its folder set
  through `PI_CODING_AGENT_DIR`, the key only in a variable, and Documents as the work
  folder.

Pi kept its own files (`sessions`, `auth.json`) in the profile folder, not in `~/.pi`.

The installs above are on a clean folder. The first agent that needs Node.js also
downloads the 41 MB Node.js archive. The times include the npm download of the agent and
its dependencies.

## Background helpers

Cline starts a **hub daemon** (`cline … --cline-hub-daemon`) that outlives the task and
the window. It keeps files in its profile and install folders open, so "Usuń" could not
delete them. Found after the test runs: hubs were left on NIAGARA and the NUC.

The catalogue now lists such helpers (`launch.background`):
- **"Usuń"** stops every process whose program lies in the agent's install folder before
  deleting it.
- **When Lokalomat exits**, it stops only the background helpers.

Agent windows the user opened keep running. The program is matched by the running image
or by the first command-line item, because Windows moves the image of a deleted program
to the Recycle Bin.

## Not in this PR

- Hermes Agent, OpenClaw and Droid: part 3.
- Updating an installed agent: a binary agent counts as installed whatever its version,
  so a new catalogue version needs "Usuń" and "Zainstaluj". npm agents install the new
  version on their next install, because the marker version differs.
- Telemetry switches other than Goose's.

# ADR 0029: Release builds and signed updates (M6, part 1)

- Status: accepted (owner, 2026-11-18: Windows and Linux for 1.0; updates through GitHub
  Releases; Windows installers unsigned for now; M6 in three PRs).
- Date: 2026-11-18

## Context

PRD §10 asks for:
- a Windows install per user, without administrator rights;
- updates through Tauri's updater, with a signature;
- update checks that can be switched off (privacy: the only network traffic is model and
  engine downloads, plus update checks);
- an installer under 30 MB.

Until 0.2.1 releases were built by hand, for Windows only. There was no updater, and
GitHub CI is off (no billing).

## Decision

1. **Platforms for 1.0**:
   - Windows x64: built on NIAGARA; NSIS `.exe` and MSI.
   - Linux x64: built on the NUC with Ubuntu 24.04; AppImage and `.deb`.
   - macOS waits for a Mac or for CI.
   - The installers are about 6.5 MB (NSIS), 8.8 MB (MSI) and 9.4 MB (`.deb`); engines
     and models are downloaded later.
   - **Deviation from the PRD's 30 MB limit**: the AppImage is about 86 MB, because it
     carries the GTK and WebKit libraries. We accept it, since only an AppImage can update
     itself on Linux; the `.deb` stays under the limit for users who prefer it.
2. **Per-user install**: NSIS with `installMode: currentUser` and Polish and English
   texts. The MSI stays for admins who deploy it per machine.
3. **Updater**: `tauri-plugin-updater`.
   - It reads `https://github.com/roxlukas/lokalomat/releases/latest/download/latest.json`.
   - The public key is in `tauri.conf.json`. **The private key is the owner's**, made
     with `tauri signer generate`, kept outside the repo and given to release builds
     through `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD`.
   - The updater refuses a file whose signature does not match.
   - The flow is driven from Rust (`updates.rs`), with no JS plugin and no extra
     permissions for the webview:
     - `check_update`: "Sprawdź teraz";
     - `install_update`: download with progress (`update://progress`), install, restart;
     - a quiet check 20 s after start, only when the new setting `update_check` is on
       (default on; switched off in Settings → O aplikacji). It sends
       `update://available`, which shows a one-line banner: "Jest nowa wersja X" with
       "Zainstaluj i uruchom ponownie" and "Później".
   - On Windows the NSIS update runs in `passive` mode (progress only, no questions).
   - On Linux only an AppImage can replace itself. A `.deb` install gets the banner with
     "Pobierz nową wersję", which opens the release page.
4. **Release script** `scripts/release.mjs`:
   - `build` (on each platform): `tauri build` with updater artifacts switched on for that
     run only (`createUpdaterArtifacts` stays `false` in the config, so the quality gate
     builds without the key). It then collects the installers and their signatures in
     `target/release-out/<version>/`. Files are renamed to `Lokalomat-AI-Studio_…`,
     because GitHub turns spaces in asset names into dots and the update URL would
     break.
   - `publish <notes.md> [dirs]`:
     - merges the Windows and Linux folders;
     - writes `latest.json`, whose `notes` is the first paragraph of the notes, and
       `SHA256SUMS.txt`;
     - creates a **draft** release with `gh`.
     Users get the update only when the owner publishes the draft, because
     `releases/latest` skips drafts.
   **Until the owner's key exists** (`pubkey` is `PUBKEY_PENDING`), the app does no update
   check at all: no startup check and no network request, and "Sprawdź teraz" answers
   "Aktualizacje nie są jeszcze włączone w tej wersji." (`update_unavailable`).
   `release.mjs build` refuses to build until the key is in place. Putting the key in
   `tauri.conf.json` turns updates on, with no other change.
   **The key exists since 2026-11-18** (generated with `tauri signer generate`, public key in
   `tauri.conf.json`; the private key and its password are kept by the owner outside the
   repository). Losing the private key means existing installs cannot be updated: a new
   installer with a new public key would be needed.
5. **Signing of installers** (Authenticode): not for 1.0. SmartScreen may warn as
   before, and the release notes say so. The release script is the place to add
   `signtool` later.

## Verification

Windows (NIAGARA), end to end, with a **throwaway test key** and a test app ("Lokalomat
Update Test", its own identifier, so it could not touch a real install). Versions 0.9.0
and 0.9.1 were built with the updater artifacts, and `latest.json` was served from
127.0.0.1 (`dangerousInsecureTransportProtocol` in the test config only):

1. The 0.9.0 NSIS installer (7.2 MB) installed silently **without an admin prompt** into
   `%LOCALAPPDATA%\Lokalomat Update Test`.
2. 20 s after start the quiet check fetched `latest.json` and the banner showed "Jest
   nowa wersja 0.9.1." with "Zainstaluj i uruchom ponownie".
3. The click downloaded and installed 0.9.1 (`passive`). The registry showed version
   0.9.1 within 3 s, and the app restarted by itself.
4. A tampered update (another file with 0.9.1's signature, offered as 0.9.2) was
   **refused**: "The signature verification failed". The installed version stayed 0.9.1.
5. With `update_check` off, the app made no request in 45 s after start.

Linux (NUC, Ubuntu 24.04), same method: a throwaway key made with `tauri signer generate`
in a test folder, the test app "Lokalomat Update Test" (`pl.lokalsi.updatetest`), versions
0.9.0 and 0.9.1 built with `--config`, `latest.json` for `linux-x86_64` served from
127.0.0.1:8765 by `python3 -m http.server`. The NUC has no desktop session, so the app ran
under Xvfb, and clicks were sent with XTEST.

1. The build needed no extra system packages (webkit2gtk 4.1, appindicator and librsvg
   were already there). The first build took 11 min, the second 6 min
   (4 min of it compiling).
2. Sizes: the AppImage is **85.9 MiB** (it carries the GTK/WebKit libraries), the `.deb`
   **9.4 MiB**. Both get a `.sig` file. The AppImage is over the PRD's 30 MB limit; the
   `.deb` is under it. The `.deb` depends only on `libgomp1`.
3. AppImage 0.9.0: the quiet check found 0.9.1 and logged "update available" (about 50 s
   after start under Xvfb, not 20 s; the software-rendered window is slow to come up).
   The banner showed "Jest nowa wersja 0.9.1." with "Zainstaluj i uruchom ponownie".
4. The click downloaded and rewrote the AppImage ("rewriting AppImage"), and 1 s later
   logged "update installed, restarting". The file's sha256 then matched the 0.9.1 file.
   The restarted copy started, and its next check said "no update available". (Under
   `xvfb-run` the restarted process dies, because `xvfb-run` stops the X server when its
   first child exits; with a permanent Xvfb the new copy kept running.)
5. `.deb` 0.9.0 (installed with `dpkg`): the check logged "update available" and the
   banner offered "Pobierz nową wersję" (the manual path); not clicked, because it opens
   the browser. The package was removed afterwards.
6. `node scripts/release.mjs build` (the test pubkey set only locally in `tauri.conf.json`
   and reverted; the test key in the env) built and collected
   `Lokalomat-AI-Studio_0.2.1_amd64.AppImage`, `…_amd64.deb` and `platforms-linux.json`
   in `target/release-out/0.2.1/`. `tauri build` with `targets: all` also builds an
   `.rpm`; the script does not collect it. `publish` was not run.

Found on the way: the app shows the Cargo version and the updater compares the Tauri
version. `release.mjs build` now stops when `tauri.conf.json`, `Cargo.toml` and
`package.json` differ.

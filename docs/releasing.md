# Releasing Lokalomat (free edition)

How a release is prepared, built, signed and published. Background and decisions: ADR 0029
(release builds and signed updates). GitHub CI is off, so releases are built by hand:
Windows on the owner's PC, Linux on the NUC (Ubuntu 24.04).

## What a release contains

| File | Platform |
|---|---|
| `Lokalomat-AI-Studio_<version>_x64-setup.exe` | Windows, per-user NSIS installer (the updater uses it) |
| `Lokalomat-AI-Studio_<version>_x64_pl-PL.msi` | Windows, for admins |
| `Lokalomat-AI-Studio_<version>_amd64.AppImage` | Linux (the only format that updates itself) |
| `Lokalomat-AI-Studio_<version>_amd64.deb` | Linux |
| `latest.json`, `SHA256SUMS.txt` | written by `publish` |

macOS is not released. The installers are not signed with a publisher certificate, so the
release notes say that Windows SmartScreen may warn.

## 1. Prepare

1. **Version.** Raise it in `Cargo.toml` (`[workspace.package]`), `package.json`,
   `apps/desktop/package.json`, `apps/mobile/package.json`, `ui/package.json`,
   `apps/desktop/src-tauri/tauri.conf.json` and `apps/mobile/src-tauri/tauri.conf.json`.
   Then refresh the lock files:

   ```sh
   cargo update --workspace --offline
   npm install --package-lock-only --offline --ignore-scripts
   ```

   `node scripts/release.mjs build` stops when `Cargo.toml`, `package.json` and
   `tauri.conf.json` disagree. Run `npm ci` and wait for it to finish before the first commit:
   the gate needs `node_modules`.
2. **Updater key.** `plugins.updater.pubkey` in `apps/desktop/src-tauri/tauri.conf.json` must be
   the public key of the private key you sign with. Compare the key ids; a wrong key makes every
   installed app refuse the update:

   ```sh
   node -e "const c=require('./apps/desktop/src-tauri/tauri.conf.json');console.log(Buffer.from(c.plugins.updater.pubkey,'base64').toString().split('\n')[0])"
   node -e "console.log(Buffer.from(require('fs').readFileSync(process.argv[1],'utf8').trim(),'base64').toString().split('\n')[0])" <key>.pub
   ```

   The private key and its password stay outside every repository and are never printed.
3. **Pull request.** Put the bump on a branch `release/<version>`, pass the gate
   (`sh scripts/quality-gate.sh pr`) and build from that commit. Merge the pull request before
   the release is published: the tag is created on `main` when the draft is published.
4. **Release notes** (`notes.md`, Polish). The first paragraph is shown in the app's update
   banner, so make it one short sentence in plain words. Then bullets for the changes, then the
   remark about unsigned installers and the SHA-256 sums.

## 2. Build

Both builds need `TAURI_SIGNING_PRIVATE_KEY` (the key file's path) and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Each writes `target/release-out/<version>/` with the
installers, and `platforms-<os>.json` with their updater signatures.

**Windows** (do not set `CARGO_TARGET_DIR`: the script reads `target/release/bundle`):

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = '<path of the key>'
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content '<path of the password file>' -Raw).Trim()
node scripts/release.mjs build
```

**Linux** (the NUC has no key and no clone of the release; use a clean clone):

```sh
git clone --branch release/<version> https://github.com/roxlukas/lokalomat.git ~/release/lokalomat
cd ~/release/lokalomat && npm ci
# Copy the key and password to the tmpfs, owner-only, and remove them afterwards:
#   scp <key> <password file> nuc:/dev/shm/relkeys/   (chmod 600)
export TAURI_SIGNING_PRIVATE_KEY=/dev/shm/relkeys/free.key
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat /dev/shm/relkeys/free.pass)"
node scripts/release.mjs build
rm -rf /dev/shm/relkeys
```

Run the build detached (`setsid nohup … &`) with a log, since it takes about ten minutes, and
copy `target/release-out/<version>/` back to the Windows PC with `scp -r`.

**Check the signatures** before publishing. The key id of each signature must be the one in
`tauri.conf.json`:

```sh
node -e "const p=require(process.argv[1]);for(const [k,v] of Object.entries(p)){const l=Buffer.from(v.signature,'base64').toString().split('\n');console.log(k, Buffer.from(l[1],'base64').subarray(2,10).reverse().toString('hex').toUpperCase())}" target/release-out/<version>/platforms-win32.json
```

## 3. Publish

```sh
node scripts/release.mjs publish notes.md <windows folder> <linux folder>
```

This merges the folders, writes `latest.json` and `SHA256SUMS.txt` and creates a **draft**
release `v<version>` in `roxlukas/lokalomat`. Nothing reaches users until the owner publishes
the draft on GitHub: the updater reads `releases/latest`, which skips drafts.

## 4. After publishing

- Install the previous version, open Settings → About, press "Check now" and update.
- Keep the build folders until the update path is confirmed.

## Pitfalls seen

- The updater key in `main` once differed from the key that signed the installers (compare the
  key ids as in step 1.2).
- `npm ci` still running when the first commit starts makes the gate fail on a missing
  `svelte-check`.
- The Windows build writes into `target/release`; never share that `target` folder with another
  edition (the script takes the installer by version, so two editions would be mixed).

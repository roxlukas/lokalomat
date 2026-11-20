#!/usr/bin/env node
// Release builds with signed updates (M6, ADR 0029).
//
//   node scripts/release.mjs build
//       On each platform (Windows on NIAGARA, Linux on the NUC): builds the
//       installers with updater signatures and collects them in
//       target/release-out/<version>/ under names without spaces.
//       Needs TAURI_SIGNING_PRIVATE_KEY (the key file's path or its content)
//       and TAURI_SIGNING_PRIVATE_KEY_PASSWORD. The key never enters the repo.
//
//   node scripts/release.mjs publish <notes.md> [dir ...]
//       Merges the collected folders (default: this machine's), writes
//       latest.json and SHA256SUMS.txt and creates a DRAFT GitHub release
//       v<version>. Updates reach users only when the owner publishes it:
//       the updater reads releases/latest, which skips drafts.
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const CONF = JSON.parse(readFileSync(join(ROOT, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'));
const VERSION = CONF.version;
const REPO = 'roxlukas/lokalomat';
const NAME = 'Lokalomat-AI-Studio';
const OUT = join(ROOT, 'target', 'release-out', VERSION);
const BUNDLE = join(ROOT, 'target', 'release', 'bundle');

function die(msg) {
  console.error(`release: ${msg}`);
  process.exit(1);
}

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

/** Installer files per platform: [bundle folder, file filter, updater platform key or null]. */
function targets() {
  if (process.platform === 'win32') {
    return [
      ['nsis', (f) => f.endsWith('-setup.exe'), 'windows-x86_64'],
      ['msi', (f) => f.endsWith('.msi'), null],
    ];
  }
  if (process.platform === 'linux') {
    return [
      ['appimage', (f) => f.endsWith('.AppImage'), 'linux-x86_64'],
      ['deb', (f) => f.endsWith('.deb'), null],
    ];
  }
  die('only Windows and Linux are released (ADR 0029)');
}

/** The app shows the Cargo version and the updater compares the Tauri one: they must match. */
function checkVersions() {
  const cargo = readFileSync(join(ROOT, 'Cargo.toml'), 'utf8').match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const npm = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8')).version;
  if (cargo !== VERSION || npm !== VERSION) {
    die(`versions differ: tauri.conf.json ${VERSION}, Cargo.toml ${cargo}, package.json ${npm}`);
  }
}

function build() {
  checkVersions();
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY) die('set TAURI_SIGNING_PRIVATE_KEY (path or content of the updater key)');
  if (CONF.plugins?.updater?.pubkey?.includes('PENDING')) die('the updater public key is not in tauri.conf.json yet');
  const tauriConfig = JSON.stringify({ bundle: { createUpdaterArtifacts: true } });
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  execFileSync(npm, ['run', 'tauri', 'build', '--', '--config', tauriConfig], {
    cwd: ROOT,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });
  mkdirSync(OUT, { recursive: true });
  const manifest = {};
  for (const [folder, match, platform] of targets()) {
    const dir = join(BUNDLE, folder);
    const file = readdirSync(dir).find((f) => match(f) && f.includes(VERSION));
    if (!file) die(`no ${folder} installer for ${VERSION} in ${dir}`);
    // GitHub turns spaces in asset names into dots; use names without spaces.
    const name = `${NAME}_${file.slice(file.indexOf(VERSION))}`;
    copyFileSync(join(dir, file), join(OUT, name));
    if (platform) {
      const sig = join(dir, `${file}.sig`);
      if (!existsSync(sig)) die(`missing updater signature ${sig}`);
      manifest[platform] = { file: name, signature: readFileSync(sig, 'utf8').trim() };
    }
    console.log(`  ${name}  ${sha256(join(OUT, name))}`);
  }
  writeFileSync(join(OUT, `platforms-${process.platform}.json`), JSON.stringify(manifest, null, 2));
  console.log(`release: collected in ${OUT}`);
}

function publish(notesPath, dirs) {
  if (!notesPath || !existsSync(notesPath)) die('usage: publish <notes.md> [dir ...]');
  const notes = readFileSync(notesPath, 'utf8');
  const folders = dirs.length ? dirs.map((d) => resolve(d)) : [OUT];
  const staging = join(ROOT, 'target', 'release-out', `${VERSION}-publish`);
  mkdirSync(staging, { recursive: true });
  const platforms = {};
  const assets = [];
  for (const dir of folders) {
    for (const f of readdirSync(dir)) {
      if (f.startsWith('platforms-')) {
        for (const [key, { file, signature }] of Object.entries(JSON.parse(readFileSync(join(dir, f), 'utf8')))) {
          platforms[key] = {
            signature,
            url: `https://github.com/${REPO}/releases/download/v${VERSION}/${file}`,
          };
        }
      } else if (f.startsWith(NAME)) {
        copyFileSync(join(dir, f), join(staging, f));
        assets.push(join(staging, f));
      }
    }
  }
  if (!Object.keys(platforms).length) die('no updater platforms collected');
  // The first paragraph of the notes is shown in the app's update banner.
  const summary = notes.split(/\n\s*\n/).find((p) => p.trim() && !p.startsWith('#'))?.trim() ?? '';
  const latest = { version: VERSION, notes: summary, pub_date: new Date().toISOString(), platforms };
  writeFileSync(join(staging, 'latest.json'), JSON.stringify(latest, null, 2) + '\n');
  const sums = assets.map((a) => `${sha256(a)}  ${basename(a)}`).join('\n') + '\n';
  writeFileSync(join(staging, 'SHA256SUMS.txt'), sums);
  console.log(sums);
  execFileSync(
    'gh',
    [
      'release', 'create', `v${VERSION}`, '--repo', REPO, '--draft',
      '--title', `Lokalomat AI Studio ${VERSION}`, '--notes-file', notesPath,
      ...assets, join(staging, 'latest.json'), join(staging, 'SHA256SUMS.txt'),
    ],
    { stdio: 'inherit' },
  );
  console.log(`release: draft v${VERSION} created; publish it on GitHub to start updates.`);
}

const [cmd, ...rest] = process.argv.slice(2);
if (cmd === 'build') build();
else if (cmd === 'publish') publish(rest[0], rest.slice(1));
else die('usage: node scripts/release.mjs build | publish <notes.md> [dir ...]');

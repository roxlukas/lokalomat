# ADR 0017: Missing system libraries on Linux (libgomp1)

- Status: accepted
- Date: 2026-11-12

## Context

On a minimal Ubuntu 26.04 install the upstream engine builds we download
(llama.cpp b12110 `ubuntu-x64` / `ubuntu-vulkan-x64`, whisper.cpp b5454
`whisper-bin-ubuntu-x64`) stop before `main` with
`error while loading shared libraries: libgomp.so.1` – package `libgomp1`
(GCC's OpenMP runtime) is not installed. The app only said "Could not start
the engine", which breaks plug and play (PRD §15).

Options:

1. **Bundle `libgomp.so.1`** next to the engine. Licence: GPLv3 with the GCC
   Runtime Library Exception. The exception lets the engines link to it without
   becoming GPL, but redistributing the library itself means shipping it under
   GPLv3 (licence text, source offer). There is also no upstream release asset
   with a stable URL and checksum: we would have to pin a `.deb` from the Ubuntu
   archive per release and keep it compatible with every target glibc.
2. **Declare the dependency and explain the fix.** The `.deb` package (M6)
   lists `libgomp1` in `Depends`, so apt installs it with the app. For other
   installs (AppImage, dev builds, servers) the app recognises the loader error
   and tells the user the one command that fixes it.

## Decision

Option 2.

- `crates/engines/src/syslibs.rs` recognises the glibc loader line, takes the
  soname and maps it to the Ubuntu package (known table, else Debian's rule
  `libfoo.so.3` → `libfoo3`).
- llama.cpp (`StartError::MissingLibrary`) and whisper/sd jobs
  (`CliError::MissingLibrary`) report it as error code `engine_missing_library`
  with the package as detail; the UI shows "A system component is missing. Run
  in a terminal: sudo apt install {package}" (`errors.*`, `chat.errors.*`,
  `jobs.errors.*`). The engine status keeps the technical text for the Engines
  view.
- After every engine install on Linux the app runs `<binary> --version` once
  and logs a warning plus a status message when a library is missing, so the
  problem shows up before the first chat or job.
- `tauri.conf.json` → `bundle.linux.deb.depends = ["libgomp1"]`.
- The app never runs `apt` itself (needs root; PRD §10 – no silent system
  changes).

## Consequences

- Standard Ubuntu desktops are unaffected; minimal installs get a clear fix.
- Other missing libraries (e.g. `libvulkan.so.1` for the Vulkan build) get the
  same message for free.
- Windows (missing DLL → exit `0xC0000135`, no output) and macOS (`dyld:
  Library not loaded`) are not covered yet.
- If we later ship an AppImage for headless servers, revisit bundling.

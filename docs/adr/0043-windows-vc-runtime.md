# ADR 0043: Visual C++ runtime for engines on Windows

- Status: accepted
- Date: 2026-11-27

## Context

On a Windows without the "Microsoft Visual C++ Redistributable" the upstream
engine builds (llama.cpp b12110, whisper.cpp, sd.cpp) do not start: Windows
shows a modal "VCRUNTIME140_1.dll was not found" dialog for `llama-server.exe`
and the chat waits "in queue" until the user clicks OK. Our own app does not
need the redistributable (Tauri links the C runtime statically), so nothing in
the installer brings it. The engine binaries import `vcruntime140.dll`,
`vcruntime140_1.dll`, `msvcp140*.dll` and `vcomp140.dll` (OpenMP). This
breaks plug and play (PRD §15). ADR 0017 left Windows open.

Options:

1. **Run `vc_redist.x64.exe` from the installer.** ~25 MB more, needs admin
   rights (our NSIS installer is per user), changes the system.
2. **App-local deployment.** Microsoft allows shipping the redistributable
   DLLs next to an application. Ship them with the app and make engines find
   them.
3. **Only explain.** Recognise exit code `0xC0000135` and tell the user to
   install the redistributable. Not plug and play.

## Decision

Option 2.

- `apps/desktop/src-tauri/build.rs` copies the eight DLLs from the build
  machine's MSVC redist folder (`LOKALSI_VCRT_DIR`, `VCToolsRedistDir`, or the
  newest Visual Studio found by `vswhere`; folders `Microsoft.VC14x.CRT` and
  `Microsoft.VC14x.OpenMP`) into the binary with `include_bytes!` (~1.5 MB).
  A Windows release build fails when any is missing; a debug build warns.
- At start (`vcrt::prepare`, first thing in `run_with`) the app writes them to
  `<app home>/runtime/vcrt` (only when missing or of another size) and puts
  that folder at the front of its own `PATH`. Every engine process inherits
  it. A runtime installed in System32 still wins, since Windows searches
  System32 before `PATH`.
- The app also calls `SetErrorMode(SEM_FAILCRITICALERRORS |
  SEM_NOOPENFILEERRORBOX)`, which children inherit: a missing DLL ends the
  engine with an exit code instead of a blocking dialog.
- Being in the shared library, this works for the free and the Pro edition
  without changes to their Tauri configurations.

## Consequences

- Engines start on a fresh Windows; already installed engines profit too.
- The DLL versions follow the build machine's MSVC toolset; update Visual
  Studio Build Tools before releases.
- Other processes the app starts (MCP servers, Python) also see the folder on
  `PATH`; it only adds the standard runtime.
- macOS (`dyld: Library not loaded`) is still not covered.

# ADR 0011: Linux (Ubuntu 24.04 and 26.04) as the third desktop target

- Status: accepted
- Date: 2026-11-11

## Context

PRD §2 named Windows and macOS (plus Android later). The product owner added Linux
as a third desktop target, aiming at Ubuntu 24.04 LTS and 26.04 LTS.

## Decision

- CI builds and tests on `ubuntu-24.04` and `ubuntu-26.04` next to Windows and macOS.
  System packages: WebKitGTK 4.1 (Tauri 2), OpenSSL (native-tls), D-Bus (Secret
  Service credential store via `keyring`).
- Platform key `linux-x64`. Every catalog variant that runs on Windows also runs on
  Linux (same ggml engines), so `linux-x64` was added to those variants.
- llama.cpp: the Ubuntu assets of the same release (b12110) – CUDA 13.4 (driver ≥ 580),
  CUDA 12.8, Vulkan (AMD/Intel/NVIDIA) and CPU; checksums from the GitHub digests.
- Credentials: Secret Service (GNOME Keyring / KWallet). A desktop session without one
  makes the HF token / MCP secrets fail with a clear message instead of storing them
  in plain text.
- Linux packaging (`.deb`, AppImage) arrives with the other installers in M6.

## Consequences

Linux ARM64 is not a target yet. The Vulkan build needs the system Vulkan loader
(`libvulkan1`, installed by default on Ubuntu desktops).

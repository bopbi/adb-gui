# AGENTS.md

Guidance for AI coding agents working in this repository.

## Project

Tools that complement `adb` (Android Debug Bridge) — a GUI for everyday Android
device/emulator operations, inspired by
[oh-my-android](https://github.com/ateymoori/oh-my-android).
The goal is one-click (or one-command) versions of the chores developers
otherwise type as `adb shell ...` incantations: device state, appearance
(dark mode, font scale, locale/RTL), network throttling, screenshots, app
management, and similar.

The repository is at its earliest stage: no source tree or build system exists
yet. As the stack is chosen, record the real build/test/lint commands below and
keep this file up to date.

## Environment

- Target platform: macOS (like the inspiration project).
- Requires the Android SDK `platform-tools` (`adb`). Resolve it through
  `ANDROID_HOME` / `ANDROID_SDK_ROOT` or the PATH — never hardcode a local
  SDK path.
- Talk to devices only through `adb`; do not bundle, vendor, or modify the SDK.

## Conventions

To be filled in as the project takes shape. Update this file once the manifest,
source layout, and tooling land.

## Working agreements

- Match the conventions of the code you find; the project has no established
  style of its own yet.
- Do not add a dependency without confirming it fits the chosen stack and
  version.
- Keep features small and composable — one capability per unit, mirroring
  oh-my-android's feature catalog.
- Device-facing code must fail gracefully when no device or emulator is
  connected.

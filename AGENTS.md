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

The chosen stack is **Rust** with the
[`gpui-kit`](https://github.com/longbridge/gpui-kit) crate (GPUI + component
library) for the GUI, driving the system `adb` binary.

The workspace has landed: `crates/adb-core` (headless adb client library —
SDK discovery, `AdbClient`, `track-devices` watcher, device parsing) and
`crates/adb-gui` (the gpui-kit application shell — device store bridged into
GPUI, `Feature` trait + registry, feature grid; real feature catalog lands in
later parts). See the Conventions section for build/test/lint commands and
`plan/` for the part-by-part roadmap.

## Skills

Before writing or reviewing any GPUI code, read
`.agents/skills/gpui-kit/SKILL.md` and its `references/coding-guides.md` —
they are the normative source for architecture, state ownership, public API,
naming, and testing decisions (including `#[gpui_kit::test]` UI integration
testing). Use `.agents/skills/gpui-kit-design-guides/` for any visible-surface
decision (layout, spacing, color, component choice, copy). The skills are
vendored from upstream and pinned in `.agents/skills/PINNED.md`; do not edit
them locally.

For project-specific work — adding a device feature to the catalog, extending
the app shell, writing UI tests, or any gpui-kit 0.7.1 API question — read
`.agents/skills/adb-gui/SKILL.md` first; it names the canonical pattern files
and records execution-verified API facts. It complements, never overrides,
the vendored skills.

## Environment

- Target platform: macOS (like the inspiration project).
- Requires the Android SDK `platform-tools` (`adb`). Resolve it through
  `ANDROID_HOME` / `ANDROID_SDK_ROOT` or the PATH — never hardcode a local
  SDK path.
- Talk to devices only through `adb`; do not bundle, vendor, or modify the SDK.

## Conventions

- Cargo workspace, Rust edition 2024. Two crates:
  - `crates/adb-core` — headless library; the **only** crate that spawns
    processes. All device I/O goes through `AdbClient` / `find_adb` /
    `spawn_device_watcher` here.
  - `crates/adb-gui` — the GPUI application (presentation only; no process
    spawning, no adb logic).
- Dependencies are deliberate and minimal (`adb-core` currently: `thiserror`
  only). Do not add `tokio`, `regex`, `serde`, or an async runtime without a
  plan that justifies it — sync `std` + threads suffices.
- Commands:
  - `cargo build` — build the workspace
  - `cargo test -p adb-core` — unit tests (fast, no device needed)
  - `cargo clippy --workspace -- -D warnings` — lint gate
  - `cargo fmt --check` — format gate
  - Device integration tests (opt-in, need a real device/emulator):
    `ADB_GUI_TEST_DEVICE=1 cargo test -p adb-core --features device-it -- --ignored`
- Tests use fixture strings and fake adb shell scripts; unit tests never touch
  a real SDK or device.

## Working agreements

- Match the conventions of the code you find; the project has no established
  style of its own yet.
- Do not add a dependency without confirming it fits the chosen stack and
  version.
- Keep features small and composable — one capability per unit, mirroring
  oh-my-android's feature catalog.
- Device-facing code must fail gracefully when no device or emulator is
  connected.

# adb-gui

A cross-platform desktop control panel for Android devices and emulators —
one click for the everyday chores you'd otherwise type as `adb shell ...`
incantations: dark mode, font scale, locale/RTL, network throttling,
screenshots, app management, GPS/battery/call simulation, and more.

Inspired by [oh-my-android](https://github.com/ateymoori/oh-my-android)
(the excellent macOS-only SwiftUI original by ateymoori). This project
reimplements the same idea in **Rust + [GPUI](https://gpui.rs)** (via the
[`gpui-kit`](https://github.com/longbridge/gpui-kit) crate) so it runs on
macOS, Linux, and Windows.

> Status: early development. The roadmap is planned in `plan/` — see
> `plan/2026-10-10-rust-gpui-adb-gui.md`.

## Features (planned, mirroring oh-my-android)

- **Appearance** — dark mode, font scale, display size, language/locale
  incl. RTL and pseudo-locales
- **Accessibility** — TalkBack toggle, animations off for UI tests
- **Navigation & input** — Back / Home / Recents, type text, hide keyboard
- **Capture** — screenshot to disk + clipboard, screen recording
- **Your app** — restart, clear data, force stop, uninstall, reset
  permissions, app info, open deep link, install APK (picker or drag-drop)
- **Network** — speed/latency profiles (LTE…GSM), airplane mode, Wi-Fi,
  mobile data
- **Simulate (emulator)** — GPS presets, battery level/charging, incoming
  call/SMS, AVD snapshots

Not in scope yet: Layout Inspector, accessibility audit, Data Inspector,
MCP server (follow-up work once the core panel lands).

## How it works

The app talks to devices only through the `adb` binary from your own
Android SDK — nothing is bundled or reimplemented. It finds the SDK via
`ANDROID_HOME` / `ANDROID_SDK_ROOT`, the PATH, or platform defaults.

Architecture (modeled on oh-my-android's one-way layering):

```
crates/
  adb-core/   # headless: SDK discovery, device tracking, shell/exec-out/emu
  adb-gui/    # gpui-kit app: feature registry, one small Feature per capability
```

UI → Features → adb-core → `adb`. Every icon in the grid is a small struct
implementing one `Feature` trait, registered in a single catalog.

## Requirements

- Rust toolchain (edition 2024)
- Android SDK platform-tools (`adb`) — via Android Studio or
  `brew install --cask android-commandlinetools`
- A running emulator or a device with USB debugging enabled

## Development

```sh
cargo build                       # build the workspace
cargo test -p adb-core            # headless adb layer tests
cargo test -p adb-gui             # UI integration tests (#[gpui_kit::test])
cargo clippy --workspace -- -D warnings
```

Device-dependent integration tests are opt-in:
`ADB_GUI_TEST_DEVICE=1 cargo test -p adb-core -- --ignored`

## Agent skills

This repo vendors GPUI skills for AI coding agents under `.agents/skills/`
(from [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit/tree/main/skills),
pinned in `.agents/skills/PINNED.md`):

- **gpui-kit** — normative coding guides, GPUI mechanics references,
  component catalog, UI integration testing. Read before writing or
  reviewing any GPUI code.
- **gpui-kit-design-guides** — component choice, layout, spacing, color,
  and interaction-state guidance for anything with a visible surface.

Agent-facing project guidance lives in [`AGENTS.md`](AGENTS.md)
(`CLAUDE.md`, `GEMINI.md`, `COPILOT.md`, and
`.github/copilot-instructions.md` are symlinks to it).

## License

[MIT](LICENSE) © Bobby Prabowo.

## Acknowledgements

- [oh-my-android](https://github.com/ateymoori/oh-my-android) — the
  reference project and feature-set inspiration (MIT, © Royan AB).
- [GPUI](https://gpui.rs) by Zed Industries, and
  [gpui-kit](https://github.com/longbridge/gpui-kit) by Longbridge.

Android is a trademark of Google LLC. This project is not affiliated with
or endorsed by Google.

---
name: adb-gui
description: 'Project-specific guide for the adb-gui workspace (crates/adb-core + crates/adb-gui). Use when adding or changing a device feature in the catalog, writing or extending UI integration tests, or working with the vendored gpui-kit 0.7.1 APIs as this project uses them. Read first — it names the canonical pattern files to copy and records gpui-kit 0.7.1 facts verified by execution that the upstream skill does not state. The vendored gpui-kit skill stays normative for everything else.'
---

# adb-gui project guide

Efficiency map for this repo. Read the named source files *first* — they are
the canonical patterns. Load vendored references only for the topic at hand.

## Read first, in this order

1. `crates/adb-gui/src/feature.rs` — the `Feature` trait (`feature.rs:46`),
   `FeatureRegistry`, `all_features()` (`feature.rs:81`), and
   `SampleDarkModeToggle` (`feature.rs:88`) as the worked example.
2. `crates/adb-gui/src/executor.rs` — the `FeatureExecutor` seam
   (`executor.rs:10`) and `AdbExecutor` (`executor.rs:30`).
3. `crates/adb-gui/src/device_store.rs` — store snapshot/selection rules;
   `with_devices` (`device_store.rs:54`) is the test injection point.
4. `crates/adb-gui/src/app.rs` — composition root; `with_parts`
   (`app.rs:144`) assembles store + registry + executor + subscriptions.
5. `crates/adb-gui/tests/ui_shell.rs` — the UI test recipe
   (`RecordingExecutor` stub at `ui_shell.rs:33`, `open_app` helper at
   `ui_shell.rs:107`).

## Adding a feature (the parts 04–06 recipe)

1. Write a small struct implementing `Feature` in `feature.rs` — copy
   `SampleDarkModeToggle`. Declare `kind()`, and implement `read_state` /
   `perform` as adb command sequences through the passed `&AdbClient`.
2. Register one `Arc::new(...)` in `all_features()`. Nothing else changes:
   the grid, picker gating, executor dispatch, and error notifications are
   already generic over `FeatureKind` (`Toggle`, `Stepper`, `Choice`,
   `Action`, `TextAction`). New `FeatureKind` variants render as disabled
   placeholders until `AppView::render_feature_cell` handles them.
3. `Availability::EmulatorOnly` features: guard `read_state`/`perform`
   against physical devices (check `serial` prefix or surface a clear error);
   the shell does not gate availability yet.

Rules that are easy to break:

- Features never spawn processes and never run on the render thread — adb
  calls happen only inside `FeatureExecutor` methods, which the app calls via
  `background_spawn`.
- `read_state` must tolerate missing/unknown output (OEM variance): degrade
  to `FeatureState::None`/off, never panic.
- Only `crates/adb-core` may spawn processes. The GUI crate depends on
  `adb-core` and on `gpui-kit = "=0.7.1"` only.

## gpui-kit 0.7.1 facts verified by execution

Upstream docs may predate these; trust this list over memory:

- Size helpers are Tailwind-style: `w_9` = 36px, `w_16` = 64px. There is no
  `w_36`; for a 144px cell use `.w(rems(9.))` (`app.rs:470`).
- `disabled()` on an `ElementSnapshot` is unreliable — it returns `None` even
  for disabled kit controls. Prove disabled/enabled state by *interaction*
  (click and assert nothing changed / executor not called), as
  `ui_shell.rs` does; never assert on `disabled()` alone.
- Controls self-register for test observation via their constructor id
  (`Switch::new("...")` is findable as-is); application divs need
  `.id(...).test_support()`.
- UI test flow: `#[gpui_kit::test]` → `cx.update(gpui_kit::init)` →
  `gpui_kit::open_window(...)` → `cx.update_window(handle, |_, window, cx| {
  window.render_frame(cx); ... })`. After any async entity work,
  `cx.run_until_parked()` and re-render before querying snapshots.
- Cells are unclickable while their feature is `busy` (initial state read in
  flight). Tests must drain the initial read before clicking a control.

## Verification

`cargo build` · `cargo test -p adb-core` (seconds) · `cargo test -p adb-gui` ·
`cargo clippy --workspace --all-targets -- -D warnings` · `cargo fmt --check`.
Device-dependent behavior is covered through the `FeatureExecutor` stub; real
device checks are opt-in (`ADB_GUI_TEST_DEVICE=1`, `--features device-it`,
`-- --ignored`).

## Boundaries

- The vendored `.agents/skills/gpui-kit/` skill remains normative for GPUI
  architecture, component APIs, and design decisions; read its
  `references/coding-guides.md` when changing shell structure. Do not edit
  vendored skills (see `.agents/skills/PINNED.md`).
- Keep this file short: add only execution-verified facts and pointers, not
  re-explanations of GPUI.

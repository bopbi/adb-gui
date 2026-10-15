//! UI integration tests for the app shell: empty state, device selection,
//! batched state population, and the sample toggle dispatching through a
//! stub executor.
//!
//! Runs headless via `#[gpui_kit::test]`; no adb or device required —
//! the store is pre-populated and the executor is a recording stub.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use adb_core::{AdbClient, Device, DeviceState};
use adb_gui::{AppView, DeviceStore, Feature, FeatureExecutor, FeatureInput, FeatureState};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size,
};

/// What the stub executor was asked to do, in order.
#[derive(Clone, Debug, PartialEq)]
enum RecordedCall {
    /// One batched state read (`read_all`) for the serial.
    ReadAll { serial: String },
    /// A feature command (`perform`) with the input it carried.
    Perform {
        feature_id: &'static str,
        serial: String,
        input: InputSummary,
    },
}

/// The user intent a perform carried, for assertions.
#[derive(Clone, Debug, PartialEq)]
enum InputSummary {
    None,
    Bool(bool),
    Index(usize),
    Text(String),
    Reset,
}

/// The seam UI tests stub out: records calls, reflects Bool performs back in
/// its batched read, never touches a device. `delegate_perform` runs the real
/// feature's perform (against a fake adb) so cross-layer behavior like the
/// screenshot clipboard is exercised; `fail_read_all` injects a re-read
/// failure (K3: post-perform offers must not depend on the re-read).
struct RecordingExecutor {
    calls: Mutex<Vec<RecordedCall>>,
    dark_mode_on: Mutex<bool>,
    delegate_perform: Mutex<bool>,
    fail_read_all: Mutex<bool>,
}

impl RecordingExecutor {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            dark_mode_on: Mutex::new(false),
            delegate_perform: Mutex::new(false),
            fail_read_all: Mutex::new(false),
        }
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    fn set_delegate_perform(&self, on: bool) {
        *self.delegate_perform.lock().unwrap() = on;
    }

    fn set_fail_read_all(&self, on: bool) {
        *self.fail_read_all.lock().unwrap() = on;
    }
}

impl FeatureExecutor for RecordingExecutor {
    fn perform(
        &self,
        _client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
        input: FeatureInput,
    ) -> Result<(), adb_core::AdbError> {
        let summary = match &input {
            FeatureInput::None => InputSummary::None,
            FeatureInput::Bool(on) => {
                *self.dark_mode_on.lock().unwrap() = *on;
                InputSummary::Bool(*on)
            }
            FeatureInput::Index(ix) => InputSummary::Index(*ix),
            FeatureInput::Text(text) => InputSummary::Text(text.clone()),
            FeatureInput::Reset => InputSummary::Reset,
        };
        self.calls.lock().unwrap().push(RecordedCall::Perform {
            feature_id: feature.id(),
            serial: serial.clone(),
            input: summary,
        });
        if *self.delegate_perform.lock().unwrap() {
            return feature.perform(&_client, &serial, input);
        }
        Ok(())
    }

    fn read_all(
        &self,
        _client: AdbClient,
        features: Vec<Arc<dyn Feature>>,
        serial: String,
    ) -> Result<Vec<FeatureState>, adb_core::AdbError> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::ReadAll { serial });
        if *self.fail_read_all.lock().unwrap() {
            return Err(adb_core::AdbError::CommandFailed {
                command: "batched read".to_string(),
                stderr: "injected re-read failure".to_string(),
            });
        }
        let dark_mode_on = *self.dark_mode_on.lock().unwrap();
        Ok(features
            .iter()
            .map(|feature| match feature.id() {
                "appearance.dark-mode" => FeatureState::Toggle(dark_mode_on),
                "appearance.font-scale" => FeatureState::Value("1.3".to_string()),
                "appearance.locale" => FeatureState::Value("en-US".to_string()),
                _ => FeatureState::None,
            })
            .collect())
    }
}

fn usable_device(serial: &str, model: &str) -> Device {
    Device {
        serial: serial.to_string(),
        state: DeviceState::Device,
        model: Some(model.to_string()),
    }
}

fn offline_device(serial: &str) -> Device {
    Device {
        serial: serial.to_string(),
        state: DeviceState::Offline,
        model: None,
    }
}

/// Open a headless window with an AppView over the given devices and executor.
fn open_app(
    cx: &mut TestAppContext,
    devices: Vec<Device>,
    executor: Arc<RecordingExecutor>,
) -> AnyWindowHandle {
    open_app_with_client(cx, devices, executor, AdbClient::new(PathBuf::from("adb")))
}

/// Like [`open_app`] with an explicit adb client — tests that delegate
/// performs to real features against a fake adb use this.
fn open_app_with_client(
    cx: &mut TestAppContext,
    devices: Vec<Device>,
    executor: Arc<RecordingExecutor>,
    client: AdbClient,
) -> AnyWindowHandle {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    // Tall enough for every catalog cell to be visible without
                    // scrolling — UI tests click cells directly.
                    size(px(360.), px(2200.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                let store = cx.new(|cx| DeviceStore::with_devices(client.clone(), devices, cx));
                cx.new(|cx| {
                    AppView::with_parts(
                        client,
                        store,
                        adb_gui::all_features(),
                        executor,
                        None,
                        window,
                        cx,
                    )
                })
            },
        )
        .expect("open test window")
        .0
    })
}

/// Drain the initial batched read so cells are not busy, then re-render.
fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn empty_state_renders_when_no_devices(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new());
    let handle = open_app(cx, Vec::new(), executor.clone());

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.find("no-devices").visible(),
            "empty state must render with no devices"
        );
        assert!(
            window.try_find("feature-grid").is_none(),
            "feature grid must not render with no devices"
        );
        // `disabled()` is not reported reliably; prove the picker rejects
        // interaction instead — clicking it must not open the popup.
        window.click("device-picker", cx);
        window.render_frame(cx);
        assert!(
            window.find("device-picker").expanded() != Some(true),
            "picker must not open with no devices"
        );
    })
    .unwrap();
    assert!(
        executor.calls().is_empty(),
        "no executor call may happen with no device"
    );
}

#[gpui_kit::test]
fn usable_device_enables_grid_and_offline_shows_warning(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new());
    let handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
    );

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("no-devices").is_none(),
            "grid state must replace the empty state"
        );
        assert!(
            window.find("feature-grid").visible(),
            "feature grid must render with a usable device"
        );
    })
    .unwrap();

    // Prove the selection is live: the batched read reached the stub
    // executor for the auto-selected serial.
    cx.run_until_parked();
    assert_eq!(
        executor.calls(),
        vec![RecordedCall::ReadAll {
            serial: "emulator-5554".to_string(),
        }],
        "auto-select must trigger the batched state read for the device"
    );

    // Negative case: only an offline device — present but unusable.
    let offline_executor = Arc::new(RecordingExecutor::new());
    let offline = open_app(
        cx,
        vec![offline_device("deadbeef")],
        offline_executor.clone(),
    );
    cx.update_window(offline, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.find("no-usable-device").visible(),
            "offline-only list must show the no-usable-device warning"
        );
        // Prove the toggle rejects interaction: click changes nothing.
        window.click("feature-toggle:appearance.dark-mode", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(offline, |_, window, cx| {
        window.render_frame(cx);
        assert_ne!(
            window.find("feature-toggle:appearance.dark-mode").checked(),
            Some(true),
            "toggle must stay off when no device is usable"
        );
    })
    .unwrap();
    assert!(
        offline_executor.calls().is_empty(),
        "no executor call may happen when no device is usable"
    );
}

#[gpui_kit::test]
fn batched_state_populates_cells_for_stubbed_device(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new());
    let handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
    );
    settle(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        // The stub's batched read returns Toggle(false) for dark mode.
        assert_eq!(
            window.find("feature-toggle:appearance.dark-mode").checked(),
            Some(false),
            "batched state must populate the toggle cell"
        );
        // The stepper cell is live: clicking + dispatches a perform for it.
        window.click("feature-stepper-plus:appearance.font-scale", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        executor.calls().iter().any(|call| matches!(
            call,
            RecordedCall::Perform {
                feature_id: "appearance.font-scale",
                ..
            }
        )),
        "stepper click must dispatch a perform for the font-scale feature"
    );
}

#[gpui_kit::test]
fn clicking_toggle_dispatches_through_stub_and_flips(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new());
    let handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
    );
    // Let the initial batched read land before interacting: the switch is
    // disabled while its cell is busy.
    settle(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        assert_eq!(
            window.find("feature-toggle:appearance.dark-mode").checked(),
            Some(false),
            "toggle starts off"
        );
        window.click("feature-toggle:appearance.dark-mode", cx);
    })
    .unwrap();

    // The dispatch runs on a background thread; let it land, then re-render.
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("feature-toggle:appearance.dark-mode").checked(),
            Some(true),
            "batched re-read after perform must flip the switch"
        );
    })
    .unwrap();

    assert_eq!(
        executor.calls(),
        vec![
            // Initial batched read for the auto-selected device.
            RecordedCall::ReadAll {
                serial: "emulator-5554".to_string(),
            },
            RecordedCall::Perform {
                feature_id: "appearance.dark-mode",
                serial: "emulator-5554".to_string(),
                input: InputSummary::Bool(true),
            },
            // Batched re-read after the successful perform.
            RecordedCall::ReadAll {
                serial: "emulator-5554".to_string(),
            },
        ],
        "click must perform Bool(true) then batched re-read for the selected serial"
    );
}

#[gpui_kit::test]
fn stepper_click_dispatches_preset_index(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new());
    let handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
    );
    settle(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        window.click("feature-stepper-plus:appearance.font-scale", cx);
    })
    .unwrap();
    cx.run_until_parked();

    // The stub's batched read reports font_scale "1.3" (preset index 3), so
    // the + button must request the next preset (index 4).
    assert!(
        executor.calls().iter().any(|call| matches!(
            call,
            RecordedCall::Perform {
                feature_id: "appearance.font-scale",
                input: InputSummary::Index(4),
                ..
            }
        )),
        "plus must dispatch Index(4), the preset after the stub's 1.3; calls: {:?}",
        executor.calls()
    );
}

#[gpui_kit::test]
fn choice_dropdown_dispatches_selected_locale(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new());
    let handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
    );
    settle(cx, handle);

    // The stub reports locale "en-US"; the dropdown must mirror it.
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(
            window
                .find("feature-choice:appearance.locale")
                .value()
                .map(str::to_string)
                .as_deref(),
            Some("English (US)"),
            "batched state must select en-US in the dropdown"
        );
        // Opening the dropdown must not apply anything.
        window
            .within("feature-choice:appearance.locale")
            .click("input", cx);
        assert_eq!(
            window.find("feature-choice:appearance.locale").expanded(),
            Some(true),
            "clicking the select must open the dropdown"
        );
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        executor
            .calls()
            .iter()
            .all(|call| !matches!(call, RecordedCall::Perform { .. })),
        "opening the dropdown must not apply anything; calls: {:?}",
        executor.calls()
    );

    // Move from en-US (index 1) to the next option and confirm it.
    cx.update_window(handle, |_, window, cx| {
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let calls = executor.calls();
    let performs: Vec<_> = calls
        .iter()
        .filter(|call| matches!(call, RecordedCall::Perform { .. }))
        .collect();
    assert_eq!(
        performs.len(),
        1,
        "exactly one apply — no intermediate locales; calls: {:?}",
        executor.calls()
    );
    assert!(
        matches!(
            performs[0],
            RecordedCall::Perform {
                feature_id: "appearance.locale",
                input: InputSummary::Index(2),
                ..
            }
        ),
        "confirming the next option must dispatch Index(2); calls: {:?}",
        executor.calls()
    );
}

#[gpui_kit::test]
fn stepper_reset_dispatches_reset_input(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new());
    let handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
    );
    settle(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        window.click("feature-stepper-reset:appearance.display-size", cx);
    })
    .unwrap();
    cx.run_until_parked();

    assert!(
        executor.calls().iter().any(|call| matches!(
            call,
            RecordedCall::Perform {
                feature_id: "appearance.display-size",
                input: InputSummary::Reset,
                ..
            }
        )),
        "the reset button must dispatch FeatureInput::Reset; calls: {:?}",
        executor.calls()
    );
}

#[gpui_kit::test]
fn text_action_types_and_dispatches_on_enter(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new());
    let handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
    );
    settle(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        window.click("feature-input:input.type-text", cx);
        window.input("hello device", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    assert!(
        executor.calls().iter().any(|call| matches!(
            call,
            RecordedCall::Perform {
                feature_id: "input.type-text",
                input: InputSummary::Text(text),
                ..
            } if text == "hello device"
        )),
        "Enter must dispatch the typed text to the type-text feature; calls: {:?}",
        executor.calls()
    );
}

#[gpui_kit::test]
fn destructive_action_waits_for_confirmation(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new());
    let handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
    );
    settle(cx, handle);

    // Clicking a destructive cell opens the confirmation dialog and must NOT
    // dispatch.
    cx.update_window(handle, |_, window, cx| {
        window.click("feature-action:app.clear-data", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("dialog").is_some(),
            "destructive click must open the confirmation dialog"
        );
    })
    .unwrap();
    assert!(
        executor
            .calls()
            .iter()
            .all(|call| !matches!(call, RecordedCall::Perform { .. })),
        "no perform may be dispatched before confirmation; calls: {:?}",
        executor.calls()
    );

    // Confirming dispatches exactly once. (The dialog confirms on Enter; the
    // component's internal OK button is not click-reachable through the test
    // harness's hit testing.)
    cx.update_window(handle, |_, window, cx| {
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let calls = executor.calls();
    let performs: Vec<_> = calls
        .iter()
        .filter(|call| matches!(call, RecordedCall::Perform { .. }))
        .collect();
    assert_eq!(
        performs.len(),
        1,
        "exactly one perform after confirmation; calls: {calls:?}"
    );
    assert!(
        matches!(
            performs[0],
            RecordedCall::Perform {
                feature_id: "app.clear-data",
                ..
            }
        ),
        "the confirmed action must be clear-data; calls: {calls:?}"
    );
}

/// K3: the screenshot must reach the system clipboard even when the
/// post-perform batched re-read fails — the clipboard offer is gated on the
/// perform, not the re-read.
#[gpui_kit::test]
fn screenshot_clipboard_survives_failed_reread(cx: &mut TestAppContext) {
    // Keep the saved PNG out of the developer's Pictures dir. Safety: only
    // this test in the ui_shell binary reads HOME.
    let scratch = std::env::temp_dir().join(format!("adb-gui-ui-home-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    unsafe { std::env::set_var("HOME", &scratch) };

    let executor = Arc::new(RecordingExecutor::new());
    executor.set_delegate_perform(true);
    let handle = open_app_with_client(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
        AdbClient::new(screenshot_fake_adb()),
    );
    settle(cx, handle);

    // The perform succeeds (fake adb answers exec-out) but the re-read fails.
    executor.set_fail_read_all(true);
    cx.update_window(handle, |_, window, cx| {
        window.click("feature-action:capture.screenshot", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let clipboard = cx
        .update(|cx| cx.read_from_clipboard())
        .expect("clipboard must be written even when the re-read fails");
    let has_image = clipboard.entries.iter().any(|entry| {
        matches!(
            entry,
            gpui_kit::ClipboardEntry::Image(image)
                if image.bytes.starts_with(b"\x89PNG\r\n\x1a\nFAKE-SCREENSHOT")
        )
    });
    assert!(
        has_image,
        "the screenshot PNG must be on the clipboard despite the failed re-read: {clipboard:?}"
    );

    // The re-read failure is still surfaced (display-only): initial read,
    // perform, then the failed re-read.
    let calls = executor.calls();
    assert_eq!(
        calls.len(),
        3,
        "expected initial read + perform + failed re-read; calls: {calls:?}"
    );
}

/// Fake adb that answers `exec-out screencap -p` with PNG marker bytes.
#[cfg(unix)]
fn screenshot_fake_adb() -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("adb-gui-ui-shot-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fake-adb-ui-shot");
    std::fs::write(
        &path,
        "#!/bin/sh\n\
         if [ \"$3\" = \"exec-out\" ]; then printf '\\x89PNG\\r\\n\\x1a\\nFAKE-SCREENSHOT'; fi\n\
         exit 0\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

/// Step 1: emulator-only cells render disabled on a physical device and
/// never dispatch; the same cells work when an emulator is selected.
#[gpui_kit::test]
fn emulator_only_cells_are_disabled_on_physical_device(cx: &mut TestAppContext) {
    let physical = Arc::new(RecordingExecutor::new());
    let physical_handle = open_app(
        cx,
        vec![usable_device("pixel7-abc123", "Pixel 7")],
        physical.clone(),
    );
    settle(cx, physical_handle);

    cx.update_window(physical_handle, |_, window, cx| {
        // The guard is honored for every control kind: a Toggle (battery
        // charging), a Stepper (battery level), a Choice select (network
        // speed), and a TextAction (incoming call input).
        window.click("feature-toggle:simulate.battery-charging", cx);
        window.click("feature-stepper-plus:simulate.battery-level", cx);
        window
            .within("feature-choice:network.speed")
            .click("input", cx);
        window.click("feature-input:simulate.incoming-call", cx);
        window.input("5551234", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        physical
            .calls()
            .iter()
            .all(|call| !matches!(call, RecordedCall::Perform { .. })),
        "emulator-only cells must never dispatch on a physical device; calls: {:?}",
        physical.calls()
    );

    // On an emulator the same controls dispatch.
    let emulator = Arc::new(RecordingExecutor::new());
    let emulator_handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        emulator.clone(),
    );
    settle(cx, emulator_handle);
    cx.update_window(emulator_handle, |_, window, cx| {
        window.click("feature-toggle:simulate.battery-charging", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        emulator.calls().iter().any(|call| matches!(
            call,
            RecordedCall::Perform {
                feature_id: "simulate.battery-charging",
                ..
            }
        )),
        "emulator-only cells must dispatch on an emulator; calls: {:?}",
        emulator.calls()
    );
}

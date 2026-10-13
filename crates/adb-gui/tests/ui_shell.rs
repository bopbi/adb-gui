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
/// its batched read, never touches a device.
struct RecordingExecutor {
    calls: Mutex<Vec<RecordedCall>>,
    dark_mode_on: Mutex<bool>,
}

impl RecordingExecutor {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            dark_mode_on: Mutex::new(false),
        }
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
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
            serial,
            input: summary,
        });
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
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    // Tall enough for every catalog cell to be visible without
                    // scrolling — UI tests click cells directly.
                    size(px(360.), px(1400.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                let client = AdbClient::new(PathBuf::from("adb"));
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

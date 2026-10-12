//! UI integration tests for the app shell: empty state, device selection,
//! and the sample toggle dispatching through a stub executor.
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
    Read {
        feature_id: &'static str,
        serial: String,
    },
    PerformAndRead {
        feature_id: &'static str,
        serial: String,
        turned_on: Option<bool>,
    },
}

/// The seam UI tests stub out: records calls, returns a canned state,
/// never touches a device.
struct RecordingExecutor {
    calls: Mutex<Vec<RecordedCall>>,
    read_result: FeatureState,
}

impl RecordingExecutor {
    fn new(read_result: FeatureState) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            read_result,
        }
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl FeatureExecutor for RecordingExecutor {
    fn perform_and_read(
        &self,
        _client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
        input: FeatureInput,
    ) -> Result<FeatureState, adb_core::AdbError> {
        let turned_on = match &input {
            FeatureInput::Bool(on) => Some(*on),
            _ => None,
        };
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::PerformAndRead {
                feature_id: feature.id(),
                serial,
                turned_on,
            });
        // A real device reflects the input back: perform returns the state
        // the toggle was switched to.
        Ok(FeatureState::Toggle(turned_on.unwrap_or(false)))
    }

    fn read(
        &self,
        _client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
    ) -> Result<FeatureState, adb_core::AdbError> {
        self.calls.lock().unwrap().push(RecordedCall::Read {
            feature_id: feature.id(),
            serial,
        });
        Ok(self.read_result.clone())
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
                    size(px(360.), px(640.)),
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

#[gpui_kit::test]
fn empty_state_renders_when_no_devices(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new(FeatureState::None));
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
    let executor = Arc::new(RecordingExecutor::new(FeatureState::Toggle(false)));
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
        // The store auto-selects the first usable device; the grid renders.
        assert!(
            window.find("feature-grid").visible(),
            "feature grid must render with a usable device"
        );
    })
    .unwrap();

    // Prove the selection is live: the initial state read reached the stub
    // executor for the auto-selected serial.
    cx.run_until_parked();
    assert_eq!(
        executor.calls(),
        vec![RecordedCall::Read {
            feature_id: "appearance.dark-mode",
            serial: "emulator-5554".to_string(),
        }],
        "auto-select must trigger the initial state read for the device"
    );

    // Negative case: only an offline device — present but unusable.
    let offline_executor = Arc::new(RecordingExecutor::new(FeatureState::Toggle(false)));
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
fn clicking_toggle_dispatches_through_stub_and_flips(cx: &mut TestAppContext) {
    let executor = Arc::new(RecordingExecutor::new(FeatureState::Toggle(false)));
    let handle = open_app(
        cx,
        vec![usable_device("emulator-5554", "sdk_gphone64")],
        executor.clone(),
    );

    // Let the initial state read land before interacting: the switch is
    // disabled while its cell is busy.
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
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
            "stub executor returned Toggle(true); the switch must flip"
        );
    })
    .unwrap();

    assert_eq!(
        executor.calls(),
        vec![
            // Initial state read for the auto-selected device.
            RecordedCall::Read {
                feature_id: "appearance.dark-mode",
                serial: "emulator-5554".to_string(),
            },
            RecordedCall::PerformAndRead {
                feature_id: "appearance.dark-mode",
                serial: "emulator-5554".to_string(),
                turned_on: Some(true),
            },
        ],
        "click must dispatch Bool(true) through the executor for the selected serial"
    );
}

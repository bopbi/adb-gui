use std::sync::Arc;

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureState};
use crate::features::{apply_state, battery, state_reader};

/// Runs feature commands against a device. The GUI dispatches these on GPUI's
/// background executor (see `AppView::dispatch`), so implementations may block
/// on adb; the render thread never does. The single seam UI tests stub out.
pub trait FeatureExecutor: Send + Sync {
    /// Apply `input` to the feature.
    fn perform(
        &self,
        client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
        input: FeatureInput,
    ) -> Result<(), AdbError>;

    /// Read the current state of every feature in ONE batched device round
    /// trip (initial load, and re-read after a successful `perform`).
    fn read_all(
        &self,
        client: AdbClient,
        features: Vec<Arc<dyn Feature>>,
        serial: String,
    ) -> Result<Vec<FeatureState>, AdbError>;
}

/// The production executor: feature-declared adb commands through `AdbClient`.
pub struct AdbExecutor;

impl FeatureExecutor for AdbExecutor {
    fn perform(
        &self,
        client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        feature.perform(&client, &serial, input)
    }

    fn read_all(
        &self,
        client: AdbClient,
        features: Vec<Arc<dyn Feature>>,
        serial: String,
    ) -> Result<Vec<FeatureState>, AdbError> {
        let state = state_reader::read(&client, &serial)?;
        let mut results: Vec<FeatureState> = features
            .iter()
            .map(|feature| apply_state(feature.as_ref(), &state))
            .collect();
        // The batched read is one device-shell round trip; emulator-console
        // commands cannot join it. For an emulator, fill the charging toggle
        // from a separate `power display` read. A failed console read leaves
        // the cell at FeatureState::None (refresh_all swallows read errors).
        // Serials of emulators start with `emulator-` (Device::is_emulator).
        if serial.starts_with("emulator-")
            && features
                .iter()
                .any(|feature| feature.id() == battery::BATTERY_CHARGING_ID)
            && let Ok(output) = client.emu(&serial, &["power", "display"])
        {
            let charging = battery::BatteryCharging::apply_power_display(&output);
            for (feature, result) in features.iter().zip(results.iter_mut()) {
                if feature.id() == battery::BATTERY_CHARGING_ID {
                    *result = charging.clone();
                }
            }
        }
        Ok(results)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    /// The charging toggle cannot join the device-shell batch; for an
    /// emulator serial the executor must issue its own `emu power display`
    /// and map the AC line onto the cell.
    #[test]
    fn read_all_fills_charging_toggle_from_emu_console() {
        let log = test_util::log_path("executor-battery-charging");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-executor-charging", &log));
        let features: Vec<Arc<dyn Feature>> = vec![
            Arc::new(battery::BatteryLevel),
            Arc::new(battery::BatteryCharging),
        ];

        let states = AdbExecutor
            .read_all(client, features, "emulator-5554".to_string())
            .expect("read_all");

        // fake_adb_emu answers `emu power display` with `AC: connected`.
        assert!(
            matches!(states[1], FeatureState::Toggle(true)),
            "charging cell must reflect the console's AC line"
        );
        assert!(
            test_util::logged(&log)
                .iter()
                .any(|line| line == "-s emulator-5554 emu power display"),
            "emulator serial must get its own power display read"
        );
        test_util::clean_up(&log);
    }
}

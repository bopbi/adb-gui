//! Airplane mode: the setting alone does not take effect until the
//! AIRPLANE_MODE broadcast is sent — this feature does both. Works on
//! physical devices too (availability Always); where `svc`/broadcast is
//! restricted the stderr surfaces and the toggle reverts on next read.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState};
use crate::features::state_reader::{self, AIRPLANE_MODE, BatchedState};

pub const AIRPLANE_MODE_ID: &str = "network.airplane-mode";

pub struct AirplaneMode;

impl AirplaneMode {
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        FeatureState::Toggle(state.get(AIRPLANE_MODE).trim() == "1")
    }
}

impl Feature for AirplaneMode {
    fn id(&self) -> &'static str {
        AIRPLANE_MODE_ID
    }

    fn title(&self) -> &'static str {
        "Airplane mode"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Toggle
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Bool(on) = input else {
            return Ok(());
        };
        let value: &str = if on { "1" } else { "0" };
        let state: &str = if on { "true" } else { "false" };
        state_reader::put_setting(client, serial, "global", "airplane_mode_on", value)?;
        client.shell(
            serial,
            &[
                "am",
                "broadcast",
                "-a",
                "android.intent.action.AIRPLANE_MODE",
                "--ez",
                "state",
                state,
            ],
        )?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn airplane_mode_sets_the_setting_then_broadcasts() {
        let log = test_util::log_path("airplane");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-airplane", &log));

        AirplaneMode
            .perform(&client, "s", FeatureInput::Bool(true))
            .unwrap();
        AirplaneMode
            .perform(&client, "s", FeatureInput::Bool(false))
            .unwrap();

        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell settings put global airplane_mode_on 1".to_string(),
                "-s s shell am broadcast -a android.intent.action.AIRPLANE_MODE --ez state true"
                    .to_string(),
                "-s s shell settings put global airplane_mode_on 0".to_string(),
                "-s s shell am broadcast -a android.intent.action.AIRPLANE_MODE --ez state false"
                    .to_string(),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn apply_maps_setting_to_toggle() {
        let feature = AirplaneMode;
        // AIRPLANE_MODE is the 10th query in the table.
        fn with_airplane(value: &str) -> BatchedState {
            state_reader::parse(&format!("1\n1\n1\n1\n1\n1\n1\n1\n1\n{value}\n"))
        }
        assert!(matches!(
            feature.apply(&with_airplane("1")),
            FeatureState::Toggle(true)
        ));
        // Off, 0, null, empty, unknown: off, no panic.
        for raw in ["0", "null", ""] {
            assert!(
                matches!(
                    feature.apply(&with_airplane(raw)),
                    FeatureState::Toggle(false)
                ),
                "value {raw:?} must map to off"
            );
        }
    }
}

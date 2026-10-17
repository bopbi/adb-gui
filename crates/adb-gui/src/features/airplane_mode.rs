//! Airplane mode via `cmd connectivity airplane-mode enable|disable` (API
//! 30+): it writes the global setting AND transitions the radios, so the
//! `airplane_mode_on` read-back stays correct. The old setting-plus-broadcast
//! path is gone — `AIRPLANE_MODE` is a protected broadcast the shell uid may
//! not send, which left the setting written with the radios unchanged. On
//! older devices the command fails with a visible error and nothing is
//! written. Works on physical devices too (availability Always).

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState};
use crate::features::state_reader::{AIRPLANE_MODE, BatchedState};

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
        let command: &str = if on { "enable" } else { "disable" };
        client.shell(serial, &["cmd", "connectivity", "airplane-mode", command])?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::state_reader;
    use crate::features::test_util;

    #[test]
    fn airplane_mode_uses_cmd_connectivity() {
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
                "-s s shell cmd connectivity airplane-mode enable".to_string(),
                "-s s shell cmd connectivity airplane-mode disable".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn airplane_mode_failure_propagates_and_writes_no_setting() {
        let log = test_util::log_path("airplane-fail");
        let client = AdbClient::new(test_util::fake_adb_failing("fake-adb-airplane-fail", &log));

        let err = AirplaneMode
            .perform(&client, "s", FeatureInput::Bool(true))
            .unwrap_err();
        assert!(err.to_string().contains("airplane-mode"), "{err}");
        assert!(
            test_util::logged(&log)
                .iter()
                .all(|line| !line.contains("settings put")),
            "a failed command must not fall back to writing the setting"
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

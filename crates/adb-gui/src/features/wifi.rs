//! Wi-Fi toggle via `svc`. Read-back is the `wifi_on` global setting; OEM
//! builds that report differently map unknown values to off, never panic.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState};
use crate::features::state_reader::{BatchedState, WIFI_ON};

pub const WIFI_ID: &str = "network.wifi";

pub struct Wifi;

impl Wifi {
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        FeatureState::Toggle(state.get(WIFI_ON).trim() == "1")
    }
}

impl Feature for Wifi {
    fn id(&self) -> &'static str {
        WIFI_ID
    }

    fn title(&self) -> &'static str {
        "Wi-Fi"
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
        client.shell(serial, &["svc", "wifi", command])?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::state_reader;
    use crate::features::test_util;

    #[test]
    fn wifi_toggles_via_svc() {
        let log = test_util::log_path("wifi");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-wifi", &log));
        Wifi.perform(&client, "s", FeatureInput::Bool(true))
            .unwrap();
        Wifi.perform(&client, "s", FeatureInput::Bool(false))
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell svc wifi enable".to_string(),
                "-s s shell svc wifi disable".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn apply_reads_wifi_on() {
        let feature = Wifi;
        // WIFI_ON is the 11th query in the table.
        fn with_wifi(value: &str) -> BatchedState {
            state_reader::parse(&format!("1\n1\n1\n1\n1\n1\n1\n1\n1\n1\n{value}\n"))
        }
        assert!(matches!(
            feature.apply(&with_wifi("1")),
            FeatureState::Toggle(true)
        ));
        for raw in ["0", "2", "null", ""] {
            assert!(
                matches!(feature.apply(&with_wifi(raw)), FeatureState::Toggle(false)),
                "value {raw:?} must map to off"
            );
        }
    }
}

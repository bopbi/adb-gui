//! Mobile data toggle via `svc data`. Read-back is the `mobile_data` global
//! setting; unknown values map to off.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState};
use crate::features::state_reader::{BatchedState, MOBILE_DATA};

pub const MOBILE_DATA_ID: &str = "network.mobile-data";

pub struct MobileData;

impl MobileData {
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        FeatureState::Toggle(state.get(MOBILE_DATA).trim() == "1")
    }
}

impl Feature for MobileData {
    fn id(&self) -> &'static str {
        MOBILE_DATA_ID
    }

    fn title(&self) -> &'static str {
        "Mobile data"
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
        client.shell(serial, &["svc", "data", command])?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::state_reader;
    use crate::features::test_util;

    #[test]
    fn mobile_data_toggles_via_svc() {
        let log = test_util::log_path("mobile-data");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-mobile-data", &log));
        MobileData
            .perform(&client, "s", FeatureInput::Bool(true))
            .unwrap();
        MobileData
            .perform(&client, "s", FeatureInput::Bool(false))
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell svc data enable".to_string(),
                "-s s shell svc data disable".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn apply_reads_mobile_data_setting() {
        let feature = MobileData;
        // MOBILE_DATA is the 12th query in the table.
        fn with_data(value: &str) -> BatchedState {
            state_reader::parse(&format!("1\n1\n1\n1\n1\n1\n1\n1\n1\n1\n1\n{value}\n"))
        }
        assert!(matches!(
            feature.apply(&with_data("1")),
            FeatureState::Toggle(true)
        ));
        for raw in ["0", "null", ""] {
            assert!(
                matches!(feature.apply(&with_data(raw)), FeatureState::Toggle(false)),
                "value {raw:?} must map to off"
            );
        }
    }
}

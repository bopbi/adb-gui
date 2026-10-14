//! Clear the foreground app's data (`pm clear`). Destructive: the shell's
//! confirmation dialog gates it.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};
use crate::features::foreground_app::perform_action;

pub const CLEAR_DATA_ID: &str = "app.clear-data";

pub struct ClearData;

impl Feature for ClearData {
    fn id(&self) -> &'static str {
        CLEAR_DATA_ID
    }

    fn title(&self) -> &'static str {
        "Clear app data"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Action
    }

    fn is_destructive(&self) -> bool {
        true
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        perform_action(client, serial, input, |pkg| {
            vec!["pm".into(), "clear".into(), pkg.into()]
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util::{self, FOREGROUND_PACKAGE};

    #[test]
    fn clear_data_is_destructive_and_clears_the_foreground_package() {
        assert!(ClearData.is_destructive());
        let log = test_util::log_path("clear-data");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-clear-data", &log));
        ClearData.perform(&client, "s", FeatureInput::None).unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell dumpsys activity activities".to_string(),
                format!("-s s shell pm clear {FOREGROUND_PACKAGE}"),
            ],
        );
        test_util::clean_up(&log);
    }
}

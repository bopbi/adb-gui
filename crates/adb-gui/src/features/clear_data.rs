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

    fn targets_foreground_app(&self) -> bool {
        true
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        perform_action(client, serial, input, |app| {
            vec![
                "pm".into(),
                "clear".into(),
                "--user".into(),
                app.user.to_string(),
                app.package.clone(),
            ]
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
                format!("-s s shell pm clear --user 0 {FOREGROUND_PACKAGE}"),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn clear_data_with_pinned_target_skips_live_resolution() {
        assert!(ClearData.targets_foreground_app());
        let log = test_util::log_path("clear-data-pinned");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-clear-data-pinned", &log));
        // The app layer resolved the target before its confirmation dialog;
        // the perform must use it as-is — no `dumpsys` round trip.
        ClearData
            .perform(
                &client,
                "s",
                FeatureInput::PinnedTarget {
                    package: "com.pinned.app".to_string(),
                    user: 10,
                },
            )
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            ["-s s shell pm clear --user 10 com.pinned.app".to_string()],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn clear_data_targets_the_foreground_apps_user() {
        let log = test_util::log_path("clear-data-user");
        let client = AdbClient::new(test_util::fake_adb_for_user(
            "fake-adb-clear-data-user",
            &log,
            10,
        ));
        ClearData.perform(&client, "s", FeatureInput::None).unwrap();
        assert_eq!(
            test_util::logged(&log).last().map(String::as_str),
            Some(format!("-s s shell pm clear --user 10 {FOREGROUND_PACKAGE}").as_str()),
        );
        test_util::clean_up(&log);
    }
}

//! Uninstall the foreground app (`pm uninstall`). Destructive: the shell's
//! confirmation dialog gates it.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};
use crate::features::foreground_app::perform_action;

pub const UNINSTALL_ID: &str = "app.uninstall";

pub struct Uninstall;

impl Feature for Uninstall {
    fn id(&self) -> &'static str {
        UNINSTALL_ID
    }

    fn title(&self) -> &'static str {
        "Uninstall"
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
                "uninstall".into(),
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
    fn uninstall_is_destructive_and_removes_the_foreground_package() {
        assert!(Uninstall.is_destructive());
        let log = test_util::log_path("uninstall");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-uninstall", &log));
        Uninstall.perform(&client, "s", FeatureInput::None).unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell dumpsys activity activities".to_string(),
                format!("-s s shell pm uninstall --user 0 {FOREGROUND_PACKAGE}"),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn uninstall_with_pinned_target_skips_live_resolution() {
        assert!(Uninstall.targets_foreground_app());
        let log = test_util::log_path("uninstall-pinned");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-uninstall-pinned", &log));
        // The app layer resolved the target before its confirmation dialog;
        // the perform must use it as-is — no `dumpsys` round trip.
        Uninstall
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
            ["-s s shell pm uninstall --user 10 com.pinned.app".to_string()],
        );
        test_util::clean_up(&log);
    }
}

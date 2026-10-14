//! Reset all runtime permissions — DEVICE-WIDE, matching oh-my-android; it
//! is not scoped to the foreground app (the title says so). Destructive: the
//! shell's confirmation dialog gates it.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};
use crate::features::foreground_app::perform_action;

pub const RESET_PERMISSIONS_ID: &str = "app.reset-permissions";

pub struct ResetPermissions;

impl Feature for ResetPermissions {
    fn id(&self) -> &'static str {
        RESET_PERMISSIONS_ID
    }

    fn title(&self) -> &'static str {
        "Reset permissions (device-wide)"
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
        // `pm reset-permissions` takes no package argument; the foreground
        // package is still resolved first so the action fails clearly when
        // nothing is foregrounded.
        perform_action(client, serial, input, |_| {
            vec!["pm".into(), "reset-permissions".into()]
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn reset_permissions_is_destructive_device_wide_and_probes_foreground() {
        assert!(ResetPermissions.is_destructive());
        let log = test_util::log_path("reset-permissions");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-reset-perms", &log));
        ResetPermissions
            .perform(&client, "s", FeatureInput::None)
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell dumpsys activity activities".to_string(),
                // Device-wide: no package argument.
                "-s s shell pm reset-permissions".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }
}

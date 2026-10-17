//! Restart the foreground app: force-stop it, then relaunch the launcher
//! activity. `monkey`'s odd stdout on some builds is ignored — the exit code
//! decides, which `AdbClient` already maps.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};
use crate::features::foreground_app;

pub const RESTART_ID: &str = "app.restart";

pub struct RestartApp;

impl Feature for RestartApp {
    fn id(&self) -> &'static str {
        RESTART_ID
    }

    fn title(&self) -> &'static str {
        "Restart app"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Action
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        if input != FeatureInput::None {
            return Ok(());
        }
        let app = foreground_app::current_app(client, serial)?;
        let user = app.user.to_string();
        client.shell(serial, &["am", "force-stop", "--user", &user, &app.package])?;
        // `monkey` has no `--user` flag, so the relaunch always targets the
        // current user even when the stopped app lived in another profile.
        client.shell(
            serial,
            &[
                "monkey",
                "-p",
                &app.package,
                "-c",
                "android.intent.category.LAUNCHER",
                "1",
            ],
        )?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util::{self, FOREGROUND_PACKAGE};

    #[test]
    fn restart_force_stops_then_monkey_relaunches() {
        let log = test_util::log_path("restart");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-restart", &log));
        RestartApp
            .perform(&client, "s", FeatureInput::None)
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell dumpsys activity activities".to_string(),
                format!("-s s shell am force-stop --user 0 {FOREGROUND_PACKAGE}"),
                format!(
                    "-s s shell monkey -p {FOREGROUND_PACKAGE} -c android.intent.category.LAUNCHER 1"
                ),
            ],
        );
        test_util::clean_up(&log);
    }
}

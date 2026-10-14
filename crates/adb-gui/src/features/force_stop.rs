//! Force-stop the foreground app (`am force-stop`).

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};
use crate::features::foreground_app::perform_action;

pub const FORCE_STOP_ID: &str = "app.force-stop";

pub struct ForceStop;

impl Feature for ForceStop {
    fn id(&self) -> &'static str {
        FORCE_STOP_ID
    }

    fn title(&self) -> &'static str {
        "Force stop"
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
        perform_action(client, serial, input, |pkg| {
            vec!["am".into(), "force-stop".into(), pkg.into()]
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util::{self, FOREGROUND_PACKAGE};

    #[test]
    fn force_stop_stops_the_foreground_package() {
        let log = test_util::log_path("force-stop");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-force-stop", &log));
        ForceStop.perform(&client, "s", FeatureInput::None).unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell dumpsys activity activities".to_string(),
                format!("-s s shell am force-stop {FOREGROUND_PACKAGE}"),
            ],
        );
        test_util::clean_up(&log);
    }
}

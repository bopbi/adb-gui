//! Open the system settings screen for the foreground app.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};
use crate::features::foreground_app::perform_action;

pub const APP_INFO_ID: &str = "app.info";

pub struct AppInfo;

impl Feature for AppInfo {
    fn id(&self) -> &'static str {
        APP_INFO_ID
    }

    fn title(&self) -> &'static str {
        "App info"
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
            vec![
                "am".into(),
                "start".into(),
                "-a".into(),
                "android.settings.APPLICATION_DETAILS_SETTINGS".into(),
                "-d".into(),
                format!("package:{pkg}"),
            ]
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util::{self, FOREGROUND_PACKAGE};

    #[test]
    fn app_info_opens_the_details_settings_for_the_foreground_package() {
        let log = test_util::log_path("app-info");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-app-info", &log));
        AppInfo.perform(&client, "s", FeatureInput::None).unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell dumpsys activity activities".to_string(),
                format!(
                    "-s s shell am start -a android.settings.APPLICATION_DETAILS_SETTINGS -d package:{FOREGROUND_PACKAGE}"
                ),
            ],
        );
        test_util::clean_up(&log);
    }
}

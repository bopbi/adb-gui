//! Navigation actions: Back / Home / Recents via `input keyevent`.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};

pub const BACK_ID: &str = "navigation.back";
pub const HOME_ID: &str = "navigation.home";
pub const RECENTS_ID: &str = "navigation.recents";

macro_rules! navigation_feature {
    ($name:ident, $id:expr, $title:literal, $keycode:literal) => {
        pub struct $name;

        impl Feature for $name {
            fn id(&self) -> &'static str {
                $id
            }

            fn title(&self) -> &'static str {
                $title
            }

            fn kind(&self) -> FeatureKind {
                FeatureKind::Action
            }

            fn perform(
                &self,
                client: &AdbClient,
                serial: &str,
                _input: FeatureInput,
            ) -> Result<(), AdbError> {
                client.shell(serial, &["input", "keyevent", $keycode])?;
                Ok(())
            }
        }
    };
}

navigation_feature!(Back, BACK_ID, "Back", "4");
navigation_feature!(Home, HOME_ID, "Home", "3");
navigation_feature!(Recents, RECENTS_ID, "Recents", "187");

#[cfg(test)]
mod tests {
    use super::*;

    // The argv tests drive a fake adb shell script (Unix-only, matching
    // adb-core's client tests).
    #[cfg(unix)]
    mod perform {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        #[test]
        fn each_action_emits_its_keyevent() {
            let dir = std::env::temp_dir()
                .join(format!("adb-gui-navigation-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let log = dir.join("argv.log");
            let adb = dir.join("fake-adb-navigation");
            fs::write(
                &adb,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\n",
                    log.display()
                ),
            )
            .unwrap();
            let mut perms = fs::metadata(&adb).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&adb, perms).unwrap();

            let client = AdbClient::new(adb);
            Back.perform(&client, "s", FeatureInput::None).unwrap();
            Home.perform(&client, "s", FeatureInput::None).unwrap();
            Recents.perform(&client, "s", FeatureInput::None).unwrap();

            let lines: Vec<String> = fs::read_to_string(&log)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            assert_eq!(
                lines,
                [
                    "-s s shell input keyevent 4",
                    "-s s shell input keyevent 3",
                    "-s s shell input keyevent 187",
                ],
                "Back/Home/Recents must map to keyevents 4/3/187"
            );
        }
    }
}

//! Hide keyboard action: `input keyevent 111` (KEYCODE_ESCAPE) dismisses the
//! soft keyboard.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};

pub const HIDE_KEYBOARD_ID: &str = "input.hide-keyboard";

pub struct HideKeyboard;

impl Feature for HideKeyboard {
    fn id(&self) -> &'static str {
        HIDE_KEYBOARD_ID
    }

    fn title(&self) -> &'static str {
        "Hide keyboard"
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
        client.shell(serial, &["input", "keyevent", "111"])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn perform_emits_escape_keyevent() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("adb-gui-hide-kb-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("argv.log");
        let adb = dir.join("fake-adb-hide-kb");
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
        HideKeyboard
            .perform(&client, "s", FeatureInput::None)
            .unwrap();

        let logged = fs::read_to_string(&log).unwrap();
        assert_eq!(logged.trim_end(), "-s s shell input keyevent 111");
    }
}

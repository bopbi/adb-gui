//! Install an APK: `adb install -r <path>`, where the path comes from the
//! cell's text input (paste a path, Enter) or from dropping an `.apk` file
//! onto the window (handled by the app layer, which dispatches the dropped
//! path as `FeatureInput::Text`). gpui 0.3.8 has no platform file-dialog
//! API, so path entry + drag-drop are the install affordances.
//!
//! The path is passed as a single argv element — never shell-joined — so
//! paths with spaces are safe (adb-core takes `&[&str]` for exactly this).

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};

pub const INSTALL_APK_ID: &str = "app.install-apk";

pub struct InstallApk;

impl Feature for InstallApk {
    fn id(&self) -> &'static str {
        INSTALL_APK_ID
    }

    fn title(&self) -> &'static str {
        "Install APK"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::TextAction
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Text(path) = input else {
            return Ok(());
        };
        let path = path.trim();
        if path.is_empty() {
            return Ok(());
        }
        let looks_like_apk = path
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".apk"));
        if !looks_like_apk {
            return Err(AdbError::CommandFailed {
                command: "install".to_string(),
                stderr: format!("not an .apk path: {path}"),
            });
        }
        // Single argv element: spaces in the path are safe.
        client.install(serial, path)?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn install_passes_the_path_as_one_argv_element() {
        let log = test_util::log_path("install");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-install", &log));
        InstallApk
            .perform(
                &client,
                "serial123",
                FeatureInput::Text("/tmp/my builds/app-debug.apk".to_string()),
            )
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            ["-s serial123 install -r /tmp/my builds/app-debug.apk".to_string(),],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn install_rejects_non_apk_paths_and_empty_input() {
        let log = test_util::log_path("install-reject");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-install-reject", &log));
        let err = InstallApk
            .perform(
                &client,
                "s",
                FeatureInput::Text("/tmp/notes.txt".to_string()),
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("not an .apk path"),
            "unexpected error: {err}"
        );
        // Empty and non-text input no-op without touching adb.
        InstallApk
            .perform(&client, "s", FeatureInput::Text(String::new()))
            .unwrap();
        InstallApk
            .perform(&client, "s", FeatureInput::Bool(true))
            .unwrap();
        assert!(
            !log.exists(),
            "no argv may be logged for rejected/empty input"
        );
        test_util::clean_up(&log);
    }
}

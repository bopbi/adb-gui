//! Dark mode toggle: `cmd uimode night` with a `ui_night_mode` fallback.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState};
use crate::features::state_reader::{self, BatchedState, UI_NIGHT_MODE};

pub const DARK_MODE_ID: &str = "appearance.dark-mode";

pub struct DarkMode;

impl DarkMode {
    /// `settings get secure ui_night_mode`: `2` = yes, `1` = no; 0/empty/null
    /// (auto or unknown) map to off rather than failing.
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        FeatureState::Toggle(state.get(UI_NIGHT_MODE).trim() == "2")
    }
}

impl Feature for DarkMode {
    fn id(&self) -> &'static str {
        DARK_MODE_ID
    }

    fn title(&self) -> &'static str {
        "Dark mode"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Toggle
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Bool(on) = input else {
            return Ok(());
        };
        let mode = if on { "yes" } else { "no" };
        // Primary path: the uimode command applies the change everywhere.
        // Older builds without `cmd uimode` fail there; fall back to the raw
        // setting, which newer builds also honor.
        if client
            .shell(serial, &["cmd", "uimode", "night", mode])
            .is_err()
        {
            let value = if on { "2" } else { "1" };
            state_reader::put_setting(client, serial, "secure", "ui_night_mode", value)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_maps_values_to_toggle() {
        let feature = DarkMode;
        assert!(matches!(
            feature.apply(&state_reader::parse("2\n")),
            FeatureState::Toggle(true)
        ));
        assert!(matches!(
            feature.apply(&state_reader::parse("1\n")),
            FeatureState::Toggle(false)
        ));
        // Auto (0), unknown, empty: show off, never panic.
        for raw in ["0", "null", ""] {
            assert!(
                matches!(
                    feature.apply(&state_reader::parse(&format!("{raw}\n"))),
                    FeatureState::Toggle(false)
                ),
                "value {raw:?} must map to off"
            );
        }
    }

    // Command-string tests: fake adb logs every argv it receives.
    #[cfg(all(test, unix))]
    mod argv {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        fn script(name: &str, body: &str) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("adb-gui-dark-mode-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
            path
        }

        /// Fake adb that logs argv; `fail_cmd` makes `cmd uimode` exit 1 to
        /// exercise the settings fallback.
        fn fake_adb(name: &str, log: &Path, fail_cmd: bool) -> PathBuf {
            let fail = if fail_cmd {
                r#"if [ "$4" = "cmd" ]; then exit 1; fi"#
            } else {
                ""
            };
            script(
                name,
                &format!(
                    r#"printf '%s\n' "$*" >> "{}"
{fail}
exit 0"#,
                    log.display()
                ),
            )
        }

        fn logged(log: &Path) -> Vec<String> {
            fs::read_to_string(log)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect()
        }

        #[test]
        fn perform_uses_cmd_uimode_yes_and_no() {
            let log = std::env::temp_dir().join(format!("adb-gui-dm-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-dm-ok", &log, false));
            let feature = DarkMode;

            feature
                .perform(&client, "s", FeatureInput::Bool(true))
                .unwrap();
            feature
                .perform(&client, "s", FeatureInput::Bool(false))
                .unwrap();
            // Non-bool input is a no-op.
            feature.perform(&client, "s", FeatureInput::None).unwrap();

            assert_eq!(
                logged(&log),
                [
                    "-s s shell cmd uimode night yes",
                    "-s s shell cmd uimode night no",
                ],
            );
        }

        #[test]
        fn perform_falls_back_to_ui_night_mode_when_cmd_missing() {
            let log = std::env::temp_dir()
                .join(format!("adb-gui-dm-fallback-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-dm-fallback", &log, true));
            let feature = DarkMode;

            feature
                .perform(&client, "s", FeatureInput::Bool(true))
                .unwrap();
            feature
                .perform(&client, "s", FeatureInput::Bool(false))
                .unwrap();

            // cmd fails; the setting write is attempted instead.
            assert_eq!(
                logged(&log),
                [
                    "-s s shell cmd uimode night yes",
                    "-s s shell settings put secure ui_night_mode 2",
                    "-s s shell cmd uimode night no",
                    "-s s shell settings put secure ui_night_mode 1",
                ],
            );
        }
    }
}

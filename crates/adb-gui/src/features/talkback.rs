//! TalkBack toggle: enables/disables the accessibility service by writing
//! `enabled_accessibility_services` + `accessibility_enabled`.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState};
use crate::features::state_reader::{self, ACCESSIBILITY_SERVICES, BatchedState};

pub const TALKBACK_ID: &str = "accessibility.talkback";

/// The TalkBack service component on Google emulators/devices with Play
/// services. OEM devices without it hit the enable error path.
pub const TALKBACK_COMPONENT: &str =
    "com.google.android.marvin.talkback/com.google.android.marvin.talkback.TalkBackService";
pub const TALKBACK_PACKAGE: &str = "com.google.android.marvin.talkback";

pub struct TalkBack;

impl TalkBack {
    /// On when the TalkBack component is in the enabled services list. The
    /// list is colon-separated; a substring check is safe because the
    /// component name is fully qualified.
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        FeatureState::Toggle(
            state
                .get(ACCESSIBILITY_SERVICES)
                .contains(TALKBACK_COMPONENT),
        )
    }
}

impl Feature for TalkBack {
    fn id(&self) -> &'static str {
        TALKBACK_ID
    }

    fn title(&self) -> &'static str {
        "TalkBack"
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
        if on {
            // Refuse to "enable" a component that is not installed — a
            // missing service would silently no-op on the device.
            let installed = client.shell(serial, &["pm", "path", TALKBACK_PACKAGE])?;
            if installed.trim().is_empty() {
                return Err(AdbError::CommandFailed {
                    command: format!("pm path {TALKBACK_PACKAGE}"),
                    stderr: "TalkBack is not installed on this device".to_string(),
                });
            }
            state_reader::put_setting(
                client,
                serial,
                "secure",
                "enabled_accessibility_services",
                TALKBACK_COMPONENT,
            )?;
            state_reader::put_setting(client, serial, "secure", "accessibility_enabled", "1")
        } else {
            state_reader::put_setting(
                client,
                serial,
                "secure",
                "enabled_accessibility_services",
                "",
            )?;
            state_reader::put_setting(client, serial, "secure", "accessibility_enabled", "0")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a BatchedState whose services answer (4th in the query table)
    /// is `value`.
    fn with_services(value: &str) -> BatchedState {
        state_reader::parse(&format!("1\n1\n1\n{value}\n"))
    }

    #[test]
    fn apply_parses_service_list() {
        let feature = TalkBack;
        let enabled = format!("com.svc/other:{TALKBACK_COMPONENT}");
        assert!(matches!(
            feature.apply(&with_services(&enabled)),
            FeatureState::Toggle(true)
        ));
        assert!(matches!(
            feature.apply(&with_services("com.svc/other:com.svc.AnotherService")),
            FeatureState::Toggle(false)
        ));
        // Empty / null / unknown: off, no panic.
        for raw in ["", "null"] {
            assert!(matches!(
                feature.apply(&with_services(raw)),
                FeatureState::Toggle(false)
            ));
        }
    }

    // Command-string tests: fake adb logs every argv it receives and answers
    // `pm path` (installed vs not installed).
    #[cfg(all(test, unix))]
    mod argv {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        fn fake_adb(name: &str, log: &Path, installed: bool) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("adb-gui-talkback-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            let answer = if installed {
                format!("package:{TALKBACK_PACKAGE}\n")
            } else {
                String::new()
            };
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\n\
                     if [ \"$4\" = \"pm\" ]; then printf '{answer}'; fi\n",
                    log.display()
                ),
            )
            .unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
            path
        }

        fn logged(log: &Path) -> Vec<String> {
            fs::read_to_string(log)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect()
        }

        #[test]
        fn enable_probes_pm_path_then_sets_both_settings() {
            let log =
                std::env::temp_dir().join(format!("adb-gui-tb-on-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-tb-on", &log, true));
            TalkBack
                .perform(&client, "s", FeatureInput::Bool(true))
                .unwrap();

            let component_setting = format!(
                "-s s shell settings put secure enabled_accessibility_services {TALKBACK_COMPONENT}"
            );
            assert_eq!(
                logged(&log),
                [
                    format!("-s s shell pm path {TALKBACK_PACKAGE}"),
                    component_setting,
                    "-s s shell settings put secure accessibility_enabled 1".to_string(),
                ],
            );
            let _ = fs::remove_file(&log);
        }

        #[test]
        fn enable_fails_clearly_when_not_installed() {
            let log =
                std::env::temp_dir().join(format!("adb-gui-tb-missing-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-tb-missing", &log, false));
            let err = TalkBack
                .perform(&client, "s", FeatureInput::Bool(true))
                .unwrap_err();
            assert!(
                err.to_string().contains("TalkBack is not installed"),
                "unexpected error: {err}"
            );
            // Only the pm probe ran — no settings writes.
            assert_eq!(
                logged(&log),
                [format!("-s s shell pm path {TALKBACK_PACKAGE}")]
            );
            let _ = fs::remove_file(&log);
        }

        #[test]
        fn disable_clears_both_settings() {
            let log =
                std::env::temp_dir().join(format!("adb-gui-tb-off-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-tb-off", &log, false));
            TalkBack
                .perform(&client, "s", FeatureInput::Bool(false))
                .unwrap();

            // The cleared service list is an empty argv element; `"$*"` logs
            // it as a trailing space, not as visible quotes.
            assert_eq!(
                logged(&log),
                [
                    "-s s shell settings put secure enabled_accessibility_services ",
                    "-s s shell settings put secure accessibility_enabled 0",
                ],
            );
            let _ = fs::remove_file(&log);
        }
    }
}

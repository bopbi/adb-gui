//! Layout bounds toggle: `setprop debug.layout` plus the system refresh
//! call. The refresh transaction code is API-level specific and may fail;
//! the prop still applies, the failure is tolerated per plan note.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState};
use crate::features::state_reader::{BatchedState, DEBUG_LAYOUT};

pub const LAYOUT_BOUNDS_ID: &str = "appearance.layout-bounds";

/// `service call activity 1599295570` — the hardcoded IBinder transaction
/// that re-reads system properties for the layout debugger.
const REFRESH_TRANSACTION: &str = "1599295570";

pub struct LayoutBounds;

impl LayoutBounds {
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        let on = state
            .value(DEBUG_LAYOUT)
            .is_some_and(|value| value.trim() == "true");
        FeatureState::Toggle(on)
    }
}

impl Feature for LayoutBounds {
    fn id(&self) -> &'static str {
        LAYOUT_BOUNDS_ID
    }

    fn title(&self) -> &'static str {
        "Layout bounds"
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
        let value = if on { "true" } else { "false" };
        client.shell(serial, &["setprop", "debug.layout", value])?;
        // Best-effort refresh: some API levels reject this transaction. The
        // prop is already set; a reboot or app restart shows the outlines.
        let _ = client.shell(
            serial,
            &["service", "call", "activity", REFRESH_TRANSACTION],
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::state_reader;

    /// Build a BatchedState whose debug.layout answer (9th in the table) is
    /// `value`.
    fn with_prop(value: &str) -> BatchedState {
        state_reader::parse(&format!("1\n1\n1\n1\n1\n1\n1\nen-US\n{value}\n"))
    }

    #[test]
    fn apply_maps_prop_to_toggle() {
        let feature = LayoutBounds;
        assert!(matches!(
            feature.apply(&with_prop("true")),
            FeatureState::Toggle(true)
        ));
        assert!(matches!(
            feature.apply(&with_prop("false")),
            FeatureState::Toggle(false)
        ));
        // Empty / unset: off, no panic.
        for raw in ["", "null"] {
            assert!(matches!(
                feature.apply(&with_prop(raw)),
                FeatureState::Toggle(false)
            ));
        }
    }

    // Command-string tests: fake adb logs every argv it receives. The refresh
    // `service call` is best-effort: a device that rejects it must not fail
    // the perform.
    #[cfg(all(test, unix))]
    mod argv {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        fn fake_adb(name: &str, log: &Path, fail_refresh: bool) -> PathBuf {
            let dir = std::env::temp_dir()
                .join(format!("adb-gui-layout-bounds-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            let fail = if fail_refresh {
                r#"if [ "$4" = "service" ]; then exit 1; fi"#
            } else {
                ""
            };
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\n{fail}\nexit 0\n",
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
        fn perform_sets_prop_then_refreshes() {
            let log = std::env::temp_dir().join(format!("adb-gui-lb-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-lb-ok", &log, false));
            let feature = LayoutBounds;

            feature
                .perform(&client, "s", FeatureInput::Bool(true))
                .unwrap();
            feature
                .perform(&client, "s", FeatureInput::Bool(false))
                .unwrap();

            assert_eq!(
                logged(&log),
                [
                    "-s s shell setprop debug.layout true",
                    "-s s shell service call activity 1599295570",
                    "-s s shell setprop debug.layout false",
                    "-s s shell service call activity 1599295570",
                ],
            );
            let _ = fs::remove_file(&log);
        }

        #[test]
        fn perform_tolerates_refresh_failure() {
            let log = std::env::temp_dir().join(format!(
                "adb-gui-lb-refresh-fail-log-{}",
                std::process::id()
            ));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-lb-refresh-fail", &log, true));
            let feature = LayoutBounds;

            // The prop write still succeeds; the failing refresh is swallowed.
            feature
                .perform(&client, "s", FeatureInput::Bool(true))
                .unwrap();
            assert_eq!(
                logged(&log),
                [
                    "-s s shell setprop debug.layout true",
                    "-s s shell service call activity 1599295570",
                ],
            );
            let _ = fs::remove_file(&log);
        }
    }
}

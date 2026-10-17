//! AVD snapshots: save and load by name on the emulator console. Both are
//! free-text actions — the plan's Choice-with-listing falls back to
//! free-text names because a static `Choice` cannot hold a dynamically
//! listed snapshot set (see the part's execution log). Loading can take
//! seconds; the cell's busy state (part 03) prevents concurrent snapshot
//! actions.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Availability, Feature, FeatureInput, FeatureKind};

pub const SNAPSHOT_SAVE_ID: &str = "simulate.snapshot-save";
pub const SNAPSHOT_LOAD_ID: &str = "simulate.snapshot-load";

fn validate_name(name: &str) -> Result<&str, AdbError> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return Err(AdbError::CommandFailed {
            command: "avd snapshot".to_string(),
            stderr:
                "snapshot name must be non-empty [A-Za-z0-9_-] (the console splits on whitespace)"
                    .to_string(),
        });
    }
    Ok(name)
}

pub struct SnapshotSave;

impl Feature for SnapshotSave {
    fn id(&self) -> &'static str {
        SNAPSHOT_SAVE_ID
    }

    fn title(&self) -> &'static str {
        "Save snapshot"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::TextAction
    }

    fn availability(&self) -> Availability {
        Availability::EmulatorOnly
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Text(name) = input else {
            return Ok(());
        };
        let name = validate_name(&name)?;
        client.emu(serial, &["avd", "snapshot", "save", name])?;
        Ok(())
    }
}

pub struct SnapshotLoad;

impl Feature for SnapshotLoad {
    fn id(&self) -> &'static str {
        SNAPSHOT_LOAD_ID
    }

    fn title(&self) -> &'static str {
        "Load snapshot"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::TextAction
    }

    fn availability(&self) -> Availability {
        Availability::EmulatorOnly
    }

    /// Loading discards all state created since the snapshot was saved.
    fn is_destructive(&self) -> bool {
        true
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Text(name) = input else {
            return Ok(());
        };
        let name = validate_name(&name)?;
        client.emu(serial, &["avd", "snapshot", "load", name])?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn save_and_load_send_the_console_command_with_the_name() {
        let log = test_util::log_path("snapshot");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-snapshot", &log));

        SnapshotSave
            .perform(
                &client,
                "emulator-5554",
                FeatureInput::Text("clean-state".to_string()),
            )
            .unwrap();
        SnapshotLoad
            .perform(
                &client,
                "emulator-5554",
                FeatureInput::Text("clean-state".to_string()),
            )
            .unwrap();

        assert_eq!(
            test_util::logged(&log),
            [
                "-s emulator-5554 emu avd snapshot save clean-state".to_string(),
                "-s emulator-5554 emu avd snapshot load clean-state".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn names_reject_whitespace_and_metacharacters() {
        let log = test_util::log_path("snapshot-bad");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-snapshot-bad", &log));
        for bad in ["", "   ", "clean state", "a;b", "../x"] {
            let err = SnapshotSave
                .perform(
                    &client,
                    "emulator-5554",
                    FeatureInput::Text(bad.to_string()),
                )
                .unwrap_err();
            assert!(
                err.to_string().contains("snapshot name"),
                "bad name {bad:?} must fail clearly: {err}"
            );
        }
        assert!(!log.exists(), "rejected names must not touch the device");
        test_util::clean_up(&log);
    }
}

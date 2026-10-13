//! Font scale stepper: `settings get/put system font_scale` over fixed
//! presets.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState, StepperSpec};
use crate::features::state_reader::{self, BatchedState, FONT_SCALE};

pub const FONT_SCALE_ID: &str = "appearance.font-scale";

/// The selectable font scale presets, smallest to largest.
pub const PRESETS: &[&str] = &["0.85", "1.0", "1.15", "1.3", "1.5", "2.0"];

pub struct FontScale;

impl FontScale {
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        match state.value(FONT_SCALE) {
            Some(value) => FeatureState::Value(value.to_string()),
            None => FeatureState::None,
        }
    }
}

impl Feature for FontScale {
    fn id(&self) -> &'static str {
        FONT_SCALE_ID
    }

    fn title(&self) -> &'static str {
        "Font scale"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Stepper(StepperSpec::Presets(PRESETS))
    }

    /// `FeatureInput::Index(i)` selects `PRESETS[i]` (clamped).
    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Index(ix) = input else {
            return Ok(());
        };
        let Some(value) = PRESETS.get(ix).copied() else {
            return Ok(());
        };
        state_reader::put_setting(client, serial, "system", "font_scale", value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_passes_value_through_and_tolerates_unknown() {
        let feature = FontScale;
        assert!(matches!(
            feature.apply(&state_reader::parse("1\n1.3\n")),
            FeatureState::Value(v) if v == "1.3"
        ));
        for raw in ["1\nnull\n", "1\n\n"] {
            assert!(
                matches!(feature.apply(&state_reader::parse(raw)), FeatureState::None),
                "value {raw:?} must map to none"
            );
        }
    }

    #[test]
    fn preset_index_matches_exact_and_nearest() {
        assert_eq!(crate::feature::nearest_index(PRESETS, "1.3"), Some(3));
        assert_eq!(
            crate::feature::nearest_index(PRESETS, "1.05"),
            Some(1),
            "nearest preset"
        );
        assert_eq!(crate::feature::nearest_index(PRESETS, "garbage"), None);
    }

    // Command-string tests: fake adb logs every argv it receives.
    #[cfg(all(test, unix))]
    mod argv {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        fn fake_adb(log: &Path) -> PathBuf {
            let dir = std::env::temp_dir()
                .join(format!("adb-gui-font-scale-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("fake-adb-font-scale");
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\n",
                    log.display()
                ),
            )
            .unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
            path
        }

        #[test]
        fn perform_puts_the_preset_at_the_given_index() {
            let log = std::env::temp_dir().join(format!("adb-gui-fs-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb(&log));
            let feature = FontScale;

            // Smallest, largest, and a middle preset.
            for (ix, expected) in [(0usize, "0.85"), (3, "1.3"), (5, "2.0")] {
                feature
                    .perform(&client, "s", FeatureInput::Index(ix))
                    .unwrap();
                let want = format!("-s s shell settings put system font_scale {expected}");
                let logged = fs::read_to_string(&log).unwrap();
                assert!(
                    logged.lines().any(|line| line == want),
                    "Index({ix}) must write font_scale {expected}; log: {logged}"
                );
            }

            // Out-of-range index and non-index input write nothing.
            let before = fs::read_to_string(&log).unwrap().lines().count();
            feature
                .perform(&client, "s", FeatureInput::Index(99))
                .unwrap();
            feature
                .perform(&client, "s", FeatureInput::Bool(true))
                .unwrap();
            let after = fs::read_to_string(&log).unwrap().lines().count();
            assert_eq!(before, after, "invalid input must not emit commands");
            let _ = fs::remove_file(&log);
        }
    }
}

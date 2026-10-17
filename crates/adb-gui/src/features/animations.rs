//! Animations-off toggle for UI tests: flips the three animation scales in
//! ONE shell call.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState};
use crate::features::state_reader::{
    self, ANIMATOR_DURATION_SCALE, BatchedState, TRANSITION_ANIMATION_SCALE, WINDOW_ANIMATION_SCALE,
};

pub const ANIMATIONS_ID: &str = "accessibility.animations-off";

/// `window_animation_scale`, `transition_animation_scale`, `animator_duration_scale`.
pub const SCALES: &[(&str, &str)] = &[
    ("global", "window_animation_scale"),
    ("global", "transition_animation_scale"),
    ("global", "animator_duration_scale"),
];

pub struct AnimationsOff;

impl AnimationsOff {
    /// Checked means "all three scales are 0" (animations off).
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        let all_off = [
            WINDOW_ANIMATION_SCALE,
            TRANSITION_ANIMATION_SCALE,
            ANIMATOR_DURATION_SCALE,
        ]
        .iter()
        .all(|key| state_reader::parse_scale(state.get(key)).is_some_and(|v| v == 0.0));
        FeatureState::Toggle(all_off)
    }
}

impl Feature for AnimationsOff {
    fn id(&self) -> &'static str {
        ANIMATIONS_ID
    }

    fn title(&self) -> &'static str {
        "Animations off"
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
        let value = if on { "0" } else { "1" };
        let joined = SCALES
            .iter()
            .map(|(table, key)| format!("settings put {table} {key} {value}"))
            .collect::<Vec<_>>()
            .join("; ");
        client.shell(serial, &[&joined])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a BatchedState whose three scale answers (5th–7th queries) are
    /// `window,transition,animator`.
    fn with_scales(window: &str, transition: &str, animator: &str) -> BatchedState {
        state_reader::parse(&format!(
            "1\n1\n1\nsvc\n{window}\n{transition}\n{animator}\n"
        ))
    }

    #[test]
    fn apply_checks_all_three_scales() {
        let feature = AnimationsOff;
        assert!(matches!(
            feature.apply(&with_scales("0.0", "0.0", "0.0")),
            FeatureState::Toggle(true)
        ));
        // Any non-zero scale means animations are on.
        assert!(matches!(
            feature.apply(&with_scales("0.0", "0.5", "0.0")),
            FeatureState::Toggle(false)
        ));
        // Unknown/empty values degrade to off rather than panic.
        assert!(matches!(
            feature.apply(&with_scales("null", "1.0", "")),
            FeatureState::Toggle(false)
        ));
    }

    // The perform test drives a fake adb shell script (Unix-only, matching
    // adb-core's client tests). The pure apply test above stays cross-platform.
    #[cfg(unix)]
    mod perform {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        fn fake_adb(log: &Path) -> PathBuf {
            let dir = std::env::temp_dir()
                .join(format!("adb-gui-animations-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("fake-adb-animations");
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
        fn perform_sets_all_three_scales_in_one_call() {
            let log = std::env::temp_dir().join(format!("adb-gui-anim-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb(&log));
            let feature = AnimationsOff;

            feature
                .perform(&client, "serial123", FeatureInput::Bool(true))
                .unwrap();
            feature
                .perform(&client, "serial123", FeatureInput::Bool(false))
                .unwrap();

            let lines: Vec<String> = fs::read_to_string(&log)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            assert_eq!(
                lines,
                [
                    "-s serial123 shell settings put global window_animation_scale 0; \
                     settings put global transition_animation_scale 0; \
                     settings put global animator_duration_scale 0",
                    "-s serial123 shell settings put global window_animation_scale 1; \
                     settings put global transition_animation_scale 1; \
                     settings put global animator_duration_scale 1",
                ],
                "all three scales must be written by one shell call"
            );
            let _ = fs::remove_file(&log);
        }
    }
}

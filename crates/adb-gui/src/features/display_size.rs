//! Display size (screen density) stepper: `wm density` read, `wm density
//! <dpi>` / `wm density reset` to change. Steps are relative to the current
//! density because the right default dpi is device-specific.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind, FeatureState, StepperSpec};
use crate::features::state_reader::{self, BatchedState, WM_DENSITY};

pub const DISPLAY_SIZE_ID: &str = "appearance.display-size";

/// How much one stepper click changes the density, in dpi.
const DENSITY_STEP: u32 = 40;

/// Never drive the density below this; below ~80 the UI becomes unusable.
const MIN_DENSITY: u32 = 120;

pub struct DisplaySize;

impl DisplaySize {
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        state
            .value(WM_DENSITY)
            .and_then(state_reader::parse_density)
            .map(|dpi| FeatureState::Value(dpi.to_string()))
            .unwrap_or(FeatureState::None)
    }
}

impl Feature for DisplaySize {
    fn id(&self) -> &'static str {
        DISPLAY_SIZE_ID
    }

    fn title(&self) -> &'static str {
        "Display size"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Stepper(StepperSpec::Relative {
            supports_reset: true,
        })
    }

    /// Index semantics: 0 = smaller (−40 dpi), 1 = larger (+40 dpi).
    /// `FeatureInput::Reset` restores the device's physical default.
    /// Other inputs no-op.
    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        if input == FeatureInput::Reset {
            client.shell(serial, &["wm", "density", "reset"])?;
            return Ok(());
        }
        let FeatureInput::Index(ix) = input else {
            return Ok(());
        };
        // parse_density prefers the Override line when one is set, so
        // stepping continues from the effective density (H1).
        let current = state_reader::parse_density(&client.shell(serial, &["wm", "density"])?)
            .ok_or_else(|| AdbError::CommandFailed {
                command: "wm density".to_string(),
                stderr: "could not parse current density".to_string(),
            })?;
        let next = match ix {
            0 => current.saturating_sub(DENSITY_STEP).max(MIN_DENSITY),
            1 => current + DENSITY_STEP,
            _ => return Ok(()),
        };
        client.shell(serial, &["wm", "density", &next.to_string()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_maps_density_output_to_value() {
        let feature = DisplaySize;
        assert!(matches!(
            feature.apply(&state_reader::parse(
                "1\n1.0\nPhysical density: 440\n"
            )),
            FeatureState::Value(v) if v == "440"
        ));
        // Unknown/garbage density → none, no panic.
        assert!(matches!(
            feature.apply(&state_reader::parse("1\n1.0\nweird\n")),
            FeatureState::None
        ));
    }

    // The perform tests drive a fake adb shell script (Unix-only, matching
    // adb-core's client tests). The pure apply test above stays cross-platform.
    #[cfg(unix)]
    mod perform {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        fn script(name: &str, body: &str) -> PathBuf {
            let dir = std::env::temp_dir()
                .join(format!("adb-gui-display-size-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
            path
        }

        /// Fake adb: answers `wm density` with a fixed density, logs every
        /// argv it receives. `name` must be unique per test — tests run in
        /// parallel within one process and share the fixture dir.
        fn fake_adb(name: &str, log: &Path, density: &str) -> PathBuf {
            let body = format!(
                r#"printf '%s\n' "$*" >> "{}"
if [ "$4" = "wm" ] && [ "$5" = "density" ] && [ $# -eq 5 ]; then
  printf 'Physical density: {density}\n'
fi"#,
                log.display()
            );
            script(name, &body)
        }

        #[test]
        fn stepper_emits_wm_density_commands() {
            let log = std::env::temp_dir().join(format!("adb-gui-ds-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-display-size-440", &log, "440"));
            let feature = DisplaySize;

            // Decrease: reads density, then sets 440 - 40 = 400.
            feature
                .perform(&client, "serial123", FeatureInput::Index(0))
                .unwrap();
            // Increase: 440 + 40 = 480.
            feature
                .perform(&client, "serial123", FeatureInput::Index(1))
                .unwrap();
            // Reset.
            feature
                .perform(&client, "serial123", FeatureInput::Reset)
                .unwrap();

            let lines: Vec<String> = fs::read_to_string(&log)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            assert_eq!(
                lines,
                [
                    "-s serial123 shell wm density",
                    "-s serial123 shell wm density 400",
                    "-s serial123 shell wm density",
                    "-s serial123 shell wm density 480",
                    "-s serial123 shell wm density reset",
                ],
                "stepper must read the density, then put the stepped value"
            );
            let _ = fs::remove_file(&log);
        }

        /// H1: with an override set, `wm density` prints Physical first and
        /// Override second; the step must be computed from the effective
        /// (override) density, not the physical one.
        #[test]
        fn stepper_steps_from_override_density_when_set() {
            let log = std::env::temp_dir()
                .join(format!("adb-gui-ds-override-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let dir = std::env::temp_dir()
                .join(format!("adb-gui-display-size-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let adb = dir.join("fake-adb-display-size-override");
            fs::write(
                &adb,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\n\
                     if [ \"$4\" = \"wm\" ] && [ \"$5\" = \"density\" ] && [ $# -eq 5 ]; then\n\
                     \x20 printf 'Physical density: 440\\nOverride density: 400\\n'\nfi\nexit 0\n",
                    log.display()
                ),
            )
            .unwrap();
            let mut perms = fs::metadata(&adb).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&adb, perms).unwrap();

            let client = AdbClient::new(adb);
            let feature = DisplaySize;
            // Decrease from the override 400, not the physical 440.
            feature
                .perform(&client, "serial123", FeatureInput::Index(0))
                .unwrap();

            let logged = fs::read_to_string(&log).unwrap();
            assert!(
                logged.contains("shell wm density 360"),
                "step must be computed from the override 400, got: {logged}"
            );
            let _ = fs::remove_file(&log);
        }

        #[test]
        fn stepper_never_drops_below_minimum() {
            let log = std::env::temp_dir().join(format!("adb-gui-ds-log2-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-display-size-130", &log, "130"));
            let feature = DisplaySize;
            feature
                .perform(&client, "serial123", FeatureInput::Index(0))
                .unwrap();
            let logged = fs::read_to_string(&log).unwrap();
            assert!(
                logged.contains("shell wm density 120"),
                "density must clamp at {MIN_DENSITY}, got: {logged}"
            );
            let _ = fs::remove_file(&log);
        }
    }
}

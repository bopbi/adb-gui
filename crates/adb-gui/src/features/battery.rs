//! Battery simulation on the emulator console: capacity stepper (±10,
//! clamped 0–100) and a separate charging toggle.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Availability, Feature, FeatureInput, FeatureKind, FeatureState, StepperSpec};

pub const BATTERY_LEVEL_ID: &str = "simulate.battery-level";
pub const BATTERY_CHARGING_ID: &str = "simulate.battery-charging";

const BATTERY_STEP: i32 = 10;

/// Relative stepper: the −/+ buttons send `Index(0)`/`Index(1)`; the current
/// capacity is read from the console, stepped by 10, and clamped to 0–100.
/// Console reads cannot join the batched device-shell read, so the executor
/// issues them separately for emulators (see `AdbExecutor::read_all`).
pub struct BatteryLevel;

impl Feature for BatteryLevel {
    fn id(&self) -> &'static str {
        BATTERY_LEVEL_ID
    }

    fn title(&self) -> &'static str {
        "Battery level"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Stepper(StepperSpec::Relative {
            supports_reset: false,
        })
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
        let FeatureInput::Index(ix) = input else {
            return Ok(());
        };
        // Bare `power capacity` is set-only on the console (answers KO); the
        // read is `power display`, a multi-line status block.
        let current =
            parse_capacity(&client.emu(serial, &["power", "display"])?).ok_or_else(|| {
                AdbError::CommandFailed {
                    command: "emu power display".to_string(),
                    stderr: "could not parse current battery level".to_string(),
                }
            })?;
        let next = match ix {
            0 => (current - BATTERY_STEP).max(0),
            1 => (current + BATTERY_STEP).min(100),
            _ => return Ok(()),
        };
        client.emu(serial, &["power", "capacity", &next.to_string()])?;
        Ok(())
    }
}

/// Parse the capacity out of `power display` output — tolerant of the
/// console's exact formatting (OK prefix, indentation, surrounding lines):
/// find the `capacity:` line and take its number.
fn parse_capacity(display: &str) -> Option<i32> {
    display.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("capacity") {
            return None;
        }
        lower
            .split(':')
            .nth(1)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

/// Charging toggle: `emu power ac on|off`.
pub struct BatteryCharging;

impl BatteryCharging {
    /// Map a `power display` status block to the toggle's state: the `AC:`
    /// line is `connected` (charging) or `disconnected` (not charging).
    /// Tolerates the console's formatting and unknown output →
    /// [`FeatureState::None`], matching `parse_capacity`'s line-scan style.
    pub(crate) fn apply_power_display(display: &str) -> FeatureState {
        for line in display.lines() {
            let lower = line.to_ascii_lowercase();
            let mut parts = lower.split(':');
            if parts.next().map(str::trim) != Some("ac") {
                continue;
            }
            return match parts.next().map(str::trim) {
                Some("connected") => FeatureState::Toggle(true),
                Some("disconnected") => FeatureState::Toggle(false),
                _ => FeatureState::None,
            };
        }
        FeatureState::None
    }
}

impl Feature for BatteryCharging {
    fn id(&self) -> &'static str {
        BATTERY_CHARGING_ID
    }

    fn title(&self) -> &'static str {
        "Battery charging"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Toggle
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
        let FeatureInput::Bool(on) = input else {
            return Ok(());
        };
        let state: &str = if on { "on" } else { "off" };
        client.emu(serial, &["power", "ac", state])?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn level_steps_from_display_capacity_and_clamps() {
        assert!(matches!(
            BatteryLevel.availability(),
            Availability::EmulatorOnly
        ));
        let log = test_util::log_path("battery-level");
        // fake_adb_emu answers `emu power display` with capacity 50 and the
        // set-only bare `power capacity` with KO (the L1 console behavior).
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-battery-level", &log));

        BatteryLevel
            .perform(&client, "emulator-5554", FeatureInput::Index(1))
            .unwrap();
        BatteryLevel
            .perform(&client, "emulator-5554", FeatureInput::Index(0))
            .unwrap();

        assert_eq!(
            test_util::logged(&log),
            [
                "-s emulator-5554 emu power display".to_string(),
                "-s emulator-5554 emu power capacity 60".to_string(),
                "-s emulator-5554 emu power display".to_string(),
                "-s emulator-5554 emu power capacity 40".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn parse_capacity_tolerates_console_formatting() {
        let display = "power display\n  AC: connected\n  status: Charging\n  capacity: 50\n";
        assert_eq!(parse_capacity(display), Some(50));
        assert_eq!(parse_capacity("capacity:\t100"), Some(100));
        assert_eq!(parse_capacity("KO: unknown command"), None);
        assert_eq!(parse_capacity("capacity: garbage"), None);
        assert_eq!(parse_capacity(""), None);
    }

    #[test]
    fn apply_power_display_maps_the_ac_line() {
        let display = "power display\n  AC: connected\n  status: Charging\n  capacity: 50\n";
        assert!(matches!(
            BatteryCharging::apply_power_display(display),
            FeatureState::Toggle(true)
        ));
        assert!(matches!(
            BatteryCharging::apply_power_display("AC: disconnected"),
            FeatureState::Toggle(false)
        ));
        // No AC line, or a malformed one: unknown state, never a wrong toggle.
        assert!(matches!(
            BatteryCharging::apply_power_display("capacity: 50\nstatus: Charging"),
            FeatureState::None
        ));
        assert!(matches!(
            BatteryCharging::apply_power_display("AC: whatever\n"),
            FeatureState::None
        ));
        assert!(matches!(
            BatteryCharging::apply_power_display("KO: unknown command"),
            FeatureState::None
        ));
        assert!(matches!(
            BatteryCharging::apply_power_display(""),
            FeatureState::None
        ));
    }

    #[test]
    fn charging_toggles_ac() {
        let log = test_util::log_path("battery-charging");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-battery-charging", &log));
        BatteryCharging
            .perform(&client, "emulator-5554", FeatureInput::Bool(true))
            .unwrap();
        BatteryCharging
            .perform(&client, "emulator-5554", FeatureInput::Bool(false))
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s emulator-5554 emu power ac on".to_string(),
                "-s emulator-5554 emu power ac off".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }
}

//! Network speed profile: one Choice applies BOTH the speed and the latency
//! profile (oh-my-android pairs them). Emulator console only.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Availability, ChoiceOption, Feature, FeatureInput, FeatureKind};

pub const NETWORK_SPEED_ID: &str = "network.speed";

/// Speed profiles accepted by `emu network speed|delay`. The trailing
/// "default" entry maps to `full` (no throttling), per oh-my-android's
/// "default" profile.
pub const PROFILES: &[ChoiceOption] = &[
    ChoiceOption {
        id: "gsm",
        label: "GSM (slowest)",
    },
    ChoiceOption {
        id: "hscsd",
        label: "HSCSD",
    },
    ChoiceOption {
        id: "gprs",
        label: "GPRS (2.5G)",
    },
    ChoiceOption {
        id: "edge",
        label: "EDGE (2.75G)",
    },
    ChoiceOption {
        id: "umts",
        label: "UMTS (3G)",
    },
    ChoiceOption {
        id: "hsdpa",
        label: "HSDPA (3.5G)",
    },
    ChoiceOption {
        id: "lte",
        label: "LTE (4G)",
    },
    ChoiceOption {
        id: "full",
        label: "Full speed (no throttle)",
    },
    ChoiceOption {
        id: "default",
        label: "Default (no throttle)",
    },
];

/// Latency paired with each speed profile, per oh-my-android's table. The
/// emulator console's `network delay` accepts `gprs|edge|umts|none|<min>[:<max>]`,
/// so profiles without a named entry map to a `min:max` range (hscsd has no
/// upstream entry; GPRS-class latency is the closest named profile). Both
/// `full` and `default` mean no throttling → `none`.
pub fn delay_for(profile_id: &str) -> &'static str {
    match profile_id {
        "gsm" => "600:1200",
        "hscsd" | "gprs" => "gprs",
        "edge" => "edge",
        "umts" => "umts",
        "hsdpa" => "80:150",
        "lte" => "30:60",
        _ => "none",
    }
}

pub struct NetworkSpeed;

impl Feature for NetworkSpeed {
    fn id(&self) -> &'static str {
        NETWORK_SPEED_ID
    }

    fn title(&self) -> &'static str {
        "Network speed"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Choice(PROFILES.to_vec())
    }

    fn availability(&self) -> Availability {
        Availability::EmulatorOnly
    }

    /// Speed AND delay together, per oh-my-android's pairing.
    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Index(ix) = input else {
            return Ok(());
        };
        let Some(profile) = PROFILES.get(ix) else {
            return Ok(());
        };
        // oh-my-android's "default" profile applies `full` (no throttling).
        let profile_id = if profile.id == "default" {
            "full"
        } else {
            profile.id
        };
        client.emu(serial, &["network", "speed", profile_id])?;
        client.emu(serial, &["network", "delay", delay_for(profile.id)])?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn speed_is_emulator_only_and_applies_speed_plus_delay() {
        assert!(matches!(
            NetworkSpeed.availability(),
            Availability::EmulatorOnly
        ));
        // Paired delay values from oh-my-android's table.
        for (profile_id, expected_delay) in [
            ("gsm", "600:1200"),
            ("hscsd", "gprs"),
            ("gprs", "gprs"),
            ("edge", "edge"),
            ("umts", "umts"),
            ("hsdpa", "80:150"),
            ("lte", "30:60"),
            ("full", "none"),
            ("default", "none"),
        ] {
            assert_eq!(delay_for(profile_id), expected_delay, "{profile_id}");
        }

        let log = test_util::log_path("network-speed");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-net-speed", &log));
        let edge_ix = PROFILES.iter().position(|p| p.id == "edge").unwrap();

        NetworkSpeed
            .perform(&client, "emulator-5554", FeatureInput::Index(edge_ix))
            .unwrap();

        assert_eq!(
            test_util::logged(&log),
            [
                "-s emulator-5554 emu network speed edge".to_string(),
                "-s emulator-5554 emu network delay edge".to_string(),
            ],
        );
        test_util::clean_up(&log);

        let lte_ix = PROFILES.iter().position(|p| p.id == "lte").unwrap();
        NetworkSpeed
            .perform(&client, "emulator-5554", FeatureInput::Index(lte_ix))
            .unwrap();

        assert_eq!(
            test_util::logged(&log),
            [
                "-s emulator-5554 emu network speed lte".to_string(),
                "-s emulator-5554 emu network delay 30:60".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn speed_ignores_out_of_range_and_non_index_input() {
        let log = test_util::log_path("network-speed-noop");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-net-speed-noop", &log));
        NetworkSpeed
            .perform(&client, "emulator-5554", FeatureInput::Index(99))
            .unwrap();
        NetworkSpeed
            .perform(&client, "emulator-5554", FeatureInput::Bool(true))
            .unwrap();
        assert!(!log.exists(), "invalid input must not touch the device");
        test_util::clean_up(&log);
    }

    #[test]
    fn default_profile_applies_full() {
        let log = test_util::log_path("network-speed-default");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-net-speed-default", &log));
        let default_ix = PROFILES.iter().position(|p| p.id == "default").unwrap();

        NetworkSpeed
            .perform(&client, "emulator-5554", FeatureInput::Index(default_ix))
            .unwrap();

        assert_eq!(
            test_util::logged(&log),
            [
                "-s emulator-5554 emu network speed full".to_string(),
                "-s emulator-5554 emu network delay none".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }
}

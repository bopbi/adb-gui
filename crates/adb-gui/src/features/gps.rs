//! GPS simulation: city presets (one static table) plus a custom-coordinates
//! input. The emulator console takes `geo fix <lon> <lat>` — longitude
//! FIRST — and this module owns that ordering.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Availability, ChoiceOption, Feature, FeatureInput, FeatureKind};

pub const GPS_PRESET_ID: &str = "simulate.gps-preset";
pub const GPS_CUSTOM_ID: &str = "simulate.gps-custom";

/// City presets. The option id is "<lon> <lat>" (the exact `geo fix`
/// argument pair); the label is the city.
pub const CITIES: &[ChoiceOption] = &[
    ChoiceOption {
        id: "18.0686 59.3293",
        label: "Stockholm",
    },
    ChoiceOption {
        id: "-122.4194 37.7749",
        label: "San Francisco",
    },
    ChoiceOption {
        id: "106.8456 -6.2088",
        label: "Jakarta",
    },
    ChoiceOption {
        id: "139.6917 35.6895",
        label: "Tokyo",
    },
    ChoiceOption {
        id: "-0.1278 51.5074",
        label: "London",
    },
    ChoiceOption {
        id: "151.2093 -33.8688",
        label: "Sydney",
    },
];

/// Choice of city preset → `emu geo fix <lon> <lat>`.
pub struct GpsPreset;

impl Feature for GpsPreset {
    fn id(&self) -> &'static str {
        GPS_PRESET_ID
    }

    fn title(&self) -> &'static str {
        "GPS city preset"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Choice(CITIES.to_vec())
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
        let Some(city) = CITIES.get(ix) else {
            return Ok(());
        };
        let mut args = vec!["geo", "fix"];
        args.extend(city.id.split_whitespace());
        client.emu(serial, &args)?;
        Ok(())
    }
}

/// Custom coordinates, typed as "<lat> <lon>" (latitude first, the human
/// order; the feature swaps for the console's lon-first convention).
pub struct GpsCustom;

impl Feature for GpsCustom {
    fn id(&self) -> &'static str {
        GPS_CUSTOM_ID
    }

    fn title(&self) -> &'static str {
        "GPS custom (lat lon)"
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
        let FeatureInput::Text(coords) = input else {
            return Ok(());
        };
        let mut parts = coords.split_whitespace();
        let (Some(lat), Some(lon), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(AdbError::CommandFailed {
                command: "geo fix".to_string(),
                stderr: "expected \"<lat> <lon>\", e.g. \"59.33 18.07\"".to_string(),
            });
        };
        if lat.parse::<f64>().is_err() || lon.parse::<f64>().is_err() {
            return Err(AdbError::CommandFailed {
                command: "geo fix".to_string(),
                stderr: format!("coordinates must be numbers, got \"{coords}\""),
            });
        }
        // Console order: longitude first.
        client.emu(serial, &["geo", "fix", lon, lat])?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn preset_fixes_city_coordinates_lon_first() {
        assert!(matches!(
            GpsPreset.availability(),
            Availability::EmulatorOnly
        ));
        let log = test_util::log_path("gps-preset");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-gps-preset", &log));
        let sf_ix = CITIES
            .iter()
            .position(|c| c.label == "San Francisco")
            .unwrap();

        GpsPreset
            .perform(&client, "emulator-5554", FeatureInput::Index(sf_ix))
            .unwrap();

        assert_eq!(
            test_util::logged(&log),
            ["-s emulator-5554 emu geo fix -122.4194 37.7749".to_string()],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn custom_swaps_human_lat_lon_to_console_lon_lat() {
        let log = test_util::log_path("gps-custom");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-gps-custom", &log));

        GpsCustom
            .perform(
                &client,
                "emulator-5554",
                FeatureInput::Text("59.3293 18.0686".to_string()),
            )
            .unwrap();

        assert_eq!(
            test_util::logged(&log),
            ["-s emulator-5554 emu geo fix 18.0686 59.3293".to_string()],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn custom_rejects_malformed_coordinates() {
        let log = test_util::log_path("gps-custom-bad");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-gps-custom-bad", &log));
        for bad in ["onlylat", "1 2 3", "north west"] {
            let err = GpsCustom
                .perform(
                    &client,
                    "emulator-5554",
                    FeatureInput::Text(bad.to_string()),
                )
                .unwrap_err();
            assert!(
                err.to_string().contains("geo fix"),
                "bad input {bad:?} must fail clearly: {err}"
            );
        }
        assert!(!log.exists(), "rejected input must not touch the device");
        test_util::clean_up(&log);
    }
}

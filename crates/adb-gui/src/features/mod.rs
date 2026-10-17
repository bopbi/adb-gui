//! The device feature catalog: one small module per capability, registered
//! in [`all_features`] grouped under section headers (mirroring
//! oh-my-android's sectioned catalog).

pub mod airplane_mode;
pub mod animations;
pub mod app_info;
pub mod battery;
pub mod clear_data;
pub mod dark_mode;
pub mod deep_link;
pub mod display_size;
pub mod font_scale;
pub mod force_stop;
pub mod foreground_app;
pub mod gps;
pub mod hide_keyboard;
pub mod incoming_call;
pub mod incoming_sms;
pub mod install_apk;
pub mod layout_bounds;
pub mod locale;
pub mod mobile_data;
pub mod navigation;
pub mod network_speed;
pub mod pictures;
pub mod reset_permissions;
pub mod restart_app;
pub mod screen_record;
pub mod screenshot;
pub mod snapshots;
pub mod state_reader;
pub mod talkback;
#[cfg(all(test, unix))]
pub(crate) mod test_util;
pub mod type_text;
pub mod uninstall;
pub mod wifi;

use std::sync::Arc;

use crate::feature::{Feature, FeatureRegistry, FeatureState};

/// The catalog: sections in display order, features within each.
pub fn all_features() -> FeatureRegistry {
    FeatureRegistry::with_sections(vec![
        (
            "Appearance",
            vec![
                Arc::new(dark_mode::DarkMode),
                Arc::new(font_scale::FontScale),
                Arc::new(display_size::DisplaySize),
                Arc::new(locale::Locale),
                Arc::new(layout_bounds::LayoutBounds),
            ],
        ),
        (
            "Accessibility",
            vec![
                Arc::new(talkback::TalkBack),
                Arc::new(animations::AnimationsOff),
            ],
        ),
        (
            "Navigation",
            vec![
                Arc::new(navigation::Back),
                Arc::new(navigation::Home),
                Arc::new(navigation::Recents),
                Arc::new(type_text::TypeText),
                Arc::new(hide_keyboard::HideKeyboard),
            ],
        ),
        (
            "Network",
            vec![
                Arc::new(network_speed::NetworkSpeed),
                Arc::new(airplane_mode::AirplaneMode),
                Arc::new(wifi::Wifi),
                Arc::new(mobile_data::MobileData),
            ],
        ),
        (
            "Simulate",
            vec![
                Arc::new(gps::GpsPreset),
                Arc::new(gps::GpsCustom),
                Arc::new(battery::BatteryLevel),
                Arc::new(battery::BatteryCharging),
                Arc::new(incoming_call::IncomingCall),
                Arc::new(incoming_sms::IncomingSms),
                Arc::new(snapshots::SnapshotSave),
                Arc::new(snapshots::SnapshotLoad),
            ],
        ),
        (
            "Capture",
            vec![
                Arc::new(screenshot::Screenshot::new()),
                Arc::new(screen_record::ScreenRecord::new()),
            ],
        ),
        (
            "App",
            vec![
                Arc::new(restart_app::RestartApp),
                Arc::new(clear_data::ClearData),
                Arc::new(force_stop::ForceStop),
                Arc::new(uninstall::Uninstall),
                Arc::new(reset_permissions::ResetPermissions),
                Arc::new(app_info::AppInfo),
                Arc::new(deep_link::DeepLink),
                Arc::new(install_apk::InstallApk),
            ],
        ),
    ])
}

/// Map one batched device read to a feature's display state. Features with
/// nothing readable (nav keys, capture, app actions) get [`FeatureState::None`].
/// Emulator-console-only reads (battery charging) cannot join the batch;
/// the executor fills those cells with a separate `emu` read afterwards.
pub(crate) fn apply_state(
    feature: &dyn Feature,
    state: &state_reader::BatchedState,
) -> FeatureState {
    match feature.id() {
        dark_mode::DARK_MODE_ID => dark_mode::DarkMode.apply(state),
        font_scale::FONT_SCALE_ID => font_scale::FontScale.apply(state),
        display_size::DISPLAY_SIZE_ID => display_size::DisplaySize.apply(state),
        locale::LOCALE_ID => locale::Locale.apply(state),
        layout_bounds::LAYOUT_BOUNDS_ID => layout_bounds::LayoutBounds.apply(state),
        talkback::TALKBACK_ID => talkback::TalkBack.apply(state),
        animations::ANIMATIONS_ID => animations::AnimationsOff.apply(state),
        airplane_mode::AIRPLANE_MODE_ID => airplane_mode::AirplaneMode.apply(state),
        wifi::WIFI_ID => wifi::Wifi.apply(state),
        mobile_data::MOBILE_DATA_ID => mobile_data::MobileData.apply(state),
        _ => FeatureState::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::Availability;

    /// Every emulator-only feature in the catalog reports the variant (L6:
    /// keep this list honest when adding simulate/network features).
    #[test]
    fn emulator_only_features_report_the_availability() {
        let expected: &[&str] = &[
            network_speed::NETWORK_SPEED_ID,
            gps::GPS_PRESET_ID,
            gps::GPS_CUSTOM_ID,
            battery::BATTERY_LEVEL_ID,
            battery::BATTERY_CHARGING_ID,
            incoming_call::INCOMING_CALL_ID,
            incoming_sms::INCOMING_SMS_ID,
            snapshots::SNAPSHOT_SAVE_ID,
            snapshots::SNAPSHOT_LOAD_ID,
        ];
        let registry = all_features();
        for id in expected {
            let feature = registry
                .features()
                .iter()
                .find(|f| f.id() == *id)
                .unwrap_or_else(|| panic!("{id} must be registered"));
            assert!(
                matches!(feature.availability(), Availability::EmulatorOnly),
                "{id} must be EmulatorOnly"
            );
        }
        // Everything else stays available on physical devices.
        for feature in registry.features() {
            if !expected.contains(&feature.id()) {
                assert!(
                    matches!(feature.availability(), Availability::Always),
                    "{} unexpectedly restricts availability",
                    feature.id()
                );
            }
        }
    }
}

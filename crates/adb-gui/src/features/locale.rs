//! Locale choice: `cmd locale set-device-locale <tag>` over preset tags,
//! including RTL (ar-EG) and pseudo-locales (en-XA, ar-XB). The command only
//! exists from Android 16 QPR2 (API 36.1); on older devices it fails and the
//! error says so.

use adb_core::{AdbClient, AdbError};

use crate::feature::{ChoiceOption, Feature, FeatureInput, FeatureKind, FeatureState};
use crate::features::state_reader::{BatchedState, SYSTEM_LOCALES};

pub const LOCALE_ID: &str = "appearance.locale";

pub struct Locale;

/// The selectable locale presets. `id` is the BCP-47 tag passed to
/// `cmd locale set-device-locale`; `"default"` is the no-override choice.
pub const OPTIONS: &[ChoiceOption] = &[
    ChoiceOption {
        id: "default",
        label: "System default",
    },
    ChoiceOption {
        id: "en-US",
        label: "English (US)",
    },
    ChoiceOption {
        id: "en-GB",
        label: "English (UK)",
    },
    ChoiceOption {
        id: "de-DE",
        label: "German",
    },
    ChoiceOption {
        id: "es-ES",
        label: "Spanish",
    },
    ChoiceOption {
        id: "fr-FR",
        label: "French",
    },
    ChoiceOption {
        id: "hi-IN",
        label: "Hindi",
    },
    ChoiceOption {
        id: "ja-JP",
        label: "Japanese",
    },
    ChoiceOption {
        id: "pt-BR",
        label: "Portuguese (BR)",
    },
    ChoiceOption {
        id: "ar-EG",
        label: "Arabic (RTL)",
    },
    ChoiceOption {
        id: "en-XA",
        label: "Pseudo · Accented (en-XA)",
    },
    ChoiceOption {
        id: "ar-XB",
        label: "Pseudo · RTL (ar-XB)",
    },
];

impl Locale {
    pub fn apply(&self, state: &BatchedState) -> FeatureState {
        current_tag(state)
            .map(|tag| FeatureState::Value(tag.to_string()))
            .unwrap_or(FeatureState::None)
    }
}

/// The device's current locale tag from `system_locales` ("[en-US,de-DE]"
/// on multi-locale builds) — first tag, brackets stripped; None when unset.
fn current_tag(state: &BatchedState) -> Option<&str> {
    let raw = state.value(SYSTEM_LOCALES)?.trim_start_matches('[');
    let tag = raw.split([',', ']']).next()?.trim();
    (!tag.is_empty()).then_some(tag)
}

impl Feature for Locale {
    fn id(&self) -> &'static str {
        LOCALE_ID
    }

    fn title(&self) -> &'static str {
        "Locale"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Choice(OPTIONS.to_vec())
    }

    /// `FeatureInput::Index(i)` applies `OPTIONS[i]`; the "System default"
    /// entry is a deliberate no-op — there is no portable adb command to
    /// clear a locale override, so the current locale stays.
    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Index(ix) = input else {
            return Ok(());
        };
        let Some(option) = OPTIONS.get(ix) else {
            return Ok(());
        };
        if option.id == "default" {
            return Ok(());
        }
        client
            .shell(serial, &["cmd", "locale", "set-device-locale", option.id])
            .map_err(|err| match err {
                AdbError::CommandFailed { command, stderr } => AdbError::CommandFailed {
                    command,
                    stderr: format!(
                        "{stderr} (setting the device-wide locale requires Android 16 QPR2 / API 36.1 or later)"
                    ),
                },
                other => other,
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::state_reader;

    /// Build a BatchedState whose system_locales answer (8th in the query
    /// table) is `value`.
    fn with_locales(value: &str) -> BatchedState {
        state_reader::parse(&format!("1\n1\n1\n1\n1\n1\n1\n{value}\n"))
    }

    #[test]
    fn apply_reads_current_tag() {
        let feature = Locale;
        // Newer builds report a bracketed list; older ones a bare tag.
        assert!(matches!(
            feature.apply(&with_locales("[en-US]")),
            FeatureState::Value(v) if v == "en-US"
        ));
        assert!(matches!(
            feature.apply(&with_locales("ar-EG")),
            FeatureState::Value(v) if v == "ar-EG"
        ));
        // Unset/unknown: no current selection.
        for raw in ["null", "", "[]"] {
            assert!(
                matches!(feature.apply(&with_locales(raw)), FeatureState::None),
                "value {raw:?} must map to none"
            );
        }
    }

    #[test]
    fn current_tag_parses_list_forms() {
        assert_eq!(current_tag(&with_locales("[en-US,de-DE]")), Some("en-US"));
        assert_eq!(current_tag(&with_locales("en-US")), Some("en-US"));
        assert_eq!(current_tag(&with_locales("null")), None);
    }

    // Command-string tests: fake adb logs every argv it receives.
    #[cfg(all(test, unix))]
    mod argv {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        fn fake_adb(name: &str, log: &Path) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("adb-gui-locale-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
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
        fn perform_sets_the_locale_at_the_given_index() {
            let log =
                std::env::temp_dir().join(format!("adb-gui-locale-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-locale-set", &log));
            let feature = Locale;

            // Arabic (RTL) and a pseudo-locale, by their option indices.
            let ar_ix = OPTIONS.iter().position(|o| o.id == "ar-EG").unwrap();
            let xa_ix = OPTIONS.iter().position(|o| o.id == "en-XA").unwrap();
            feature
                .perform(&client, "s", FeatureInput::Index(ar_ix))
                .unwrap();
            feature
                .perform(&client, "s", FeatureInput::Index(xa_ix))
                .unwrap();

            assert_eq!(
                fs::read_to_string(&log)
                    .unwrap()
                    .lines()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
                [
                    "-s s shell cmd locale set-device-locale ar-EG",
                    "-s s shell cmd locale set-device-locale en-XA",
                ],
            );
            let _ = fs::remove_file(&log);
        }

        #[test]
        fn perform_explains_the_api_requirement_when_the_command_fails() {
            let log = std::env::temp_dir()
                .join(format!("adb-gui-locale-fail-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(crate::features::test_util::fake_adb_failing(
                "fake-adb-locale-fail",
                &log,
            ));
            let ar_ix = OPTIONS.iter().position(|o| o.id == "ar-EG").unwrap();
            let err = Locale
                .perform(&client, "s", FeatureInput::Index(ar_ix))
                .unwrap_err();
            let message = err.to_string();
            assert!(
                message.contains("API 36.1"),
                "error must name the API requirement: {message}"
            );
            assert!(
                message.contains("Exception occurred"),
                "the device's own error must be kept: {message}"
            );
            let _ = fs::remove_file(&log);
        }

        #[test]
        fn perform_treats_default_and_out_of_range_as_noops() {
            let log = std::env::temp_dir()
                .join(format!("adb-gui-locale-noop-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb("fake-adb-locale-noop", &log));
            let feature = Locale;

            let default_ix = OPTIONS.iter().position(|o| o.id == "default").unwrap();
            feature
                .perform(&client, "s", FeatureInput::Index(default_ix))
                .unwrap();
            feature
                .perform(&client, "s", FeatureInput::Index(99))
                .unwrap();
            feature
                .perform(&client, "s", FeatureInput::Bool(true))
                .unwrap();

            assert!(
                !log.exists(),
                "default/out-of-range/non-index input must not touch the device"
            );
        }
    }
}

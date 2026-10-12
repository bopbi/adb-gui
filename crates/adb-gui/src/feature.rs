use std::sync::Arc;

use adb_core::{AdbClient, AdbError};

/// The control a feature renders as in the grid — the direct port of
/// oh-my-android's FeatureKind.
pub enum FeatureKind {
    Toggle,
    Stepper,
    Choice(Vec<ChoiceOption>),
    Action,
    TextAction,
}

/// One selectable option of a [`FeatureKind::Choice`] feature.
pub struct ChoiceOption {
    pub id: &'static str,
    pub label: &'static str,
}

/// Where a feature may run.
pub enum Availability {
    Always,
    EmulatorOnly,
}

/// The current on-device state of a feature, read back for display.
#[derive(Clone)]
pub enum FeatureState {
    Toggle(bool),
    Value(String),
    None,
}

/// The user intent a feature is asked to apply.
pub enum FeatureInput {
    None,
    Bool(bool),
    Index(usize),
    Text(String),
}

/// One device capability: declares what it is and the adb commands it needs.
/// The GUI renders from [`Feature::kind`] and runs [`Feature::perform`] /
/// [`Feature::read_state`] on a background thread through `AdbClient`.
pub trait Feature: Send + Sync {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn kind(&self) -> FeatureKind;
    fn availability(&self) -> Availability {
        Availability::Always
    }
    fn is_destructive(&self) -> bool {
        false
    }
    fn read_state(&self, client: &AdbClient, serial: &str) -> Result<FeatureState, AdbError>;
    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError>;
}

/// The registry of all features, mirroring oh-my-android's FeatureCatalog.
pub struct FeatureRegistry {
    features: Vec<Arc<dyn Feature>>,
}

impl FeatureRegistry {
    pub fn new(features: Vec<Arc<dyn Feature>>) -> Self {
        Self { features }
    }

    pub fn features(&self) -> &[Arc<dyn Feature>] {
        &self.features
    }
}

/// The catalog. Parts 04–06 register the real feature set here.
pub fn all_features() -> FeatureRegistry {
    FeatureRegistry::new(vec![Arc::new(SampleDarkModeToggle)])
}

/// Proving ground for the Feature pattern; hardened in part 04.
///
/// `settings get/put secure ui_night_mode`: `1` = no, `2` = yes.
pub struct SampleDarkModeToggle;

impl Feature for SampleDarkModeToggle {
    fn id(&self) -> &'static str {
        "appearance.dark-mode"
    }

    fn title(&self) -> &'static str {
        "Dark mode"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Toggle
    }

    fn read_state(&self, client: &AdbClient, serial: &str) -> Result<FeatureState, AdbError> {
        let output = client.shell(serial, &["settings", "get", "secure", "ui_night_mode"])?;
        // Values other than 1/2 (0, null, empty) mean auto/unknown: show off.
        Ok(FeatureState::Toggle(output.trim() == "2"))
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
        let value: &str = if on { "2" } else { "1" };
        client.shell(
            serial,
            &["settings", "put", "secure", "ui_night_mode", value],
        )?;
        Ok(())
    }
}

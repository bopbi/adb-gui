use std::sync::Arc;

use adb_core::{AdbClient, AdbError};

/// The control a feature renders as in the grid — the direct port of
/// oh-my-android's FeatureKind. The GUI renders only from this data; no
/// feature-specific knowledge may leak into the view.
pub enum FeatureKind {
    Toggle,
    Stepper(StepperSpec),
    Choice(Vec<ChoiceOption>),
    Action,
    TextAction,
}

/// How a stepper cell steps: fixed presets (font scale) or relative down/up
/// steps with an optional reset (display size, later battery).
pub enum StepperSpec {
    Presets(&'static [&'static str]),
    Relative { supports_reset: bool },
}

impl StepperSpec {
    /// The inputs the −/+ buttons dispatch for the current value. Preset
    /// steppers move to the adjacent preset; relative steppers use
    /// `Index(0)` = down, `Index(1)` = up.
    pub fn step_inputs(&self, current: Option<&str>) -> (FeatureInput, FeatureInput) {
        match self {
            StepperSpec::Presets(presets) => {
                let current_ix = current
                    .and_then(|value| nearest_index(presets, value))
                    .unwrap_or(0);
                (
                    FeatureInput::Index(current_ix.saturating_sub(1)),
                    FeatureInput::Index((current_ix + 1).min(presets.len() - 1)),
                )
            }
            StepperSpec::Relative { .. } => (FeatureInput::Index(0), FeatureInput::Index(1)),
        }
    }
}

/// The preset a device value belongs to: exact match, else the nearest by
/// numeric distance (devices report off-preset values like "1.05"). None
/// when the value is unparseable.
pub(crate) fn nearest_index(presets: &[&str], value: &str) -> Option<usize> {
    let parsed: f32 = value.parse().ok()?;
    if let Some(ix) = presets.iter().position(|preset| *preset == value) {
        return Some(ix);
    }
    presets
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da = (parsed - a.parse::<f32>().unwrap()).abs();
            let db = (parsed - b.parse::<f32>().unwrap()).abs();
            da.total_cmp(&db)
        })
        .map(|(ix, _)| ix)
}

/// One selectable option of a [`FeatureKind::Choice`] feature.
#[derive(Clone)]
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
#[derive(Clone, PartialEq)]
pub enum FeatureInput {
    None,
    Bool(bool),
    Index(usize),
    Text(String),
    /// Step back to the device's default (relative steppers with reset).
    Reset,
}

/// Bytes the app should place on the system clipboard after a successful
/// [`Feature::perform`] (e.g. a screenshot PNG). The clipboard lives in the
/// app layer, so features hand the payload over instead of touching it
/// themselves.
pub struct ClipboardPayload {
    /// Image format tag; `"png"` is the only supported value today.
    pub format: &'static str,
    pub bytes: Vec<u8>,
}

/// One device capability: declares what it is and the adb commands it needs.
/// The GUI renders from [`Feature::kind`] and runs [`Feature::perform`] on a
/// background thread through `AdbClient`; reads join the shared batch in
/// `features::state_reader` and map through `apply` in `features::mod`.
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
    /// Payload to offer the system clipboard after a successful `perform`
    /// (screenshots). Default: nothing.
    fn clipboard_payload(&self) -> Option<ClipboardPayload> {
        None
    }
    /// A user-facing note stashed by the last `perform` (e.g. where a capture
    /// landed when the Pictures dir was unavailable). The app shows it as a
    /// notification after a successful perform. Default: nothing.
    fn notice(&self) -> Option<String> {
        None
    }
    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError>;
}

/// One section of the catalog: a titled group of consecutive features in the
/// registry, mirroring oh-my-android's sectioned feature list.
pub struct Section {
    pub title: &'static str,
    pub start: usize,
    pub len: usize,
}

/// The registry of all features, mirroring oh-my-android's FeatureCatalog.
pub struct FeatureRegistry {
    features: Vec<Arc<dyn Feature>>,
    sections: Vec<Section>,
}

impl FeatureRegistry {
    pub fn new(features: Vec<Arc<dyn Feature>>) -> Self {
        let len = features.len();
        Self {
            features,
            sections: vec![Section {
                title: "",
                start: 0,
                len,
            }],
        }
    }

    /// Build a registry from titled groups; the flat feature order is the
    /// concatenation of the groups, and [`Self::sections`] records where each
    /// group lives so the grid can render headers.
    pub fn with_sections(groups: Vec<(&'static str, Vec<Arc<dyn Feature>>)>) -> Self {
        let mut features = Vec::new();
        let mut sections = Vec::new();
        for (title, group) in groups {
            sections.push(Section {
                title,
                start: features.len(),
                len: group.len(),
            });
            features.extend(group);
        }
        Self { features, sections }
    }

    pub fn features(&self) -> &[Arc<dyn Feature>] {
        &self.features
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }
}

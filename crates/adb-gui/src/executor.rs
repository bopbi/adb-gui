use std::sync::Arc;

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureState};
use crate::features::{apply_state, state_reader};

/// Runs feature commands against a device. The GUI dispatches these on GPUI's
/// background executor (see `AppView::dispatch`), so implementations may block
/// on adb; the render thread never does. The single seam UI tests stub out.
pub trait FeatureExecutor: Send + Sync {
    /// Apply `input` to the feature.
    fn perform(
        &self,
        client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
        input: FeatureInput,
    ) -> Result<(), AdbError>;

    /// Read the current state of every feature in ONE batched device round
    /// trip (initial load, and re-read after a successful `perform`).
    fn read_all(
        &self,
        client: AdbClient,
        features: Vec<Arc<dyn Feature>>,
        serial: String,
    ) -> Result<Vec<FeatureState>, AdbError>;
}

/// The production executor: feature-declared adb commands through `AdbClient`.
pub struct AdbExecutor;

impl FeatureExecutor for AdbExecutor {
    fn perform(
        &self,
        client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        feature.perform(&client, &serial, input)
    }

    fn read_all(
        &self,
        client: AdbClient,
        features: Vec<Arc<dyn Feature>>,
        serial: String,
    ) -> Result<Vec<FeatureState>, AdbError> {
        let state = state_reader::read(&client, &serial)?;
        Ok(features
            .iter()
            .map(|feature| apply_state(feature.as_ref(), &state))
            .collect())
    }
}

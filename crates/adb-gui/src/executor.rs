use std::sync::Arc;

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureState};

/// Runs feature commands against a device. The GUI dispatches these on GPUI's
/// background executor (see `AppView::dispatch`), so implementations may block
/// on adb; the render thread never does. The single seam UI tests stub out.
pub trait FeatureExecutor: Send + Sync {
    /// Apply `input`, then read the resulting state so the cell can refresh.
    fn perform_and_read(
        &self,
        client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
        input: FeatureInput,
    ) -> Result<FeatureState, AdbError>;

    /// Read the current state without performing anything (initial load).
    fn read(
        &self,
        client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
    ) -> Result<FeatureState, AdbError>;
}

/// The production executor: feature-declared adb commands through `AdbClient`.
pub struct AdbExecutor;

impl FeatureExecutor for AdbExecutor {
    fn perform_and_read(
        &self,
        client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
        input: FeatureInput,
    ) -> Result<FeatureState, AdbError> {
        feature.perform(&client, &serial, input)?;
        feature.read_state(&client, &serial)
    }

    fn read(
        &self,
        client: AdbClient,
        feature: Arc<dyn Feature>,
        serial: String,
    ) -> Result<FeatureState, AdbError> {
        feature.read_state(&client, &serial)
    }
}

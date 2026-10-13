//! GPUI desktop GUI for everyday adb operations.
//!
//! Presentation only: every device interaction is declared by a [`feature`]
//! and executed through `adb_core::AdbClient` on a background thread. This
//! crate never spawns processes.

mod app;
mod device_store;
mod executor;
mod feature;
mod features;

pub use app::AppView;
pub use device_store::{DeviceStore, DeviceStoreEvent};
pub use executor::{AdbExecutor, FeatureExecutor};
pub use feature::{
    Availability, ChoiceOption, Feature, FeatureInput, FeatureKind, FeatureRegistry, FeatureState,
    Section, StepperSpec,
};
pub use features::all_features;

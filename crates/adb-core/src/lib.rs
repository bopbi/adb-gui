//! Headless Android Debug Bridge client library.
//!
//! This crate is the **only** place in the workspace that spawns processes.
//! Everything device-facing goes through [`AdbClient`], which owns the adb
//! binary path and runs every command through one watchdog-guarded
//! spawn/capture path. The GUI crate (`adb-gui`) depends on this library and
//! must never invoke processes itself.
//!
//! Sync by design: blocking adb calls run on background threads, and callers
//! bridge into their UI runtime via channels.

mod client;
mod device;
mod error;
mod sdk;
pub mod shell_quote;
mod track;

pub use client::{AdbClient, ShellChild};
pub use device::{Device, DeviceState, parse_devices};
pub use error::{AdbError, Result};
pub use sdk::find_adb;
pub use track::spawn_device_watcher;

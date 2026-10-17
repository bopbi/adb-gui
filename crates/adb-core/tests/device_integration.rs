//! Opt-in integration tests against a real device or emulator.
//!
//! These never run in normal CI: the `device-it` cargo feature must be
//! enabled AND `ADB_GUI_TEST_DEVICE=1` must be set, and the tests are
//! `#[ignore]`d on top of that:
//!
//! ```sh
//! ADB_GUI_TEST_DEVICE=1 cargo test -p adb-core --features device-it -- --ignored
//! ```

#![cfg(feature = "device-it")]

use adb_core::{AdbClient, find_adb, spawn_device_watcher};

fn require_device_env() {
    if std::env::var_os("ADB_GUI_TEST_DEVICE").is_none() {
        panic!("set ADB_GUI_TEST_DEVICE=1 to run device integration tests");
    }
}

#[test]
#[ignore = "requires a connected device/emulator (ADB_GUI_TEST_DEVICE=1)"]
fn lists_devices_and_runs_shell_echo() {
    require_device_env();
    let client = AdbClient::new(find_adb().expect("adb not found"));
    let devices = client.devices().expect("adb devices failed");
    assert!(!devices.is_empty(), "no device/emulator connected");
    let serial = devices[0].serial.clone();
    let out = client
        .shell(&serial, &["echo", "ok"])
        .expect("shell failed");
    assert!(out.contains("ok"), "unexpected shell output: {out}");
}

#[test]
#[ignore = "requires a connected device/emulator (ADB_GUI_TEST_DEVICE=1)"]
fn device_watcher_streams_initial_snapshot() {
    require_device_env();
    let adb_path = find_adb().expect("adb not found");
    let (watcher, rx) = spawn_device_watcher(adb_path);
    let devices = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("no device snapshot within 10s");
    assert!(!devices.is_empty(), "watcher saw no devices");
    // stop() kills the adb child and joins the thread; the real adb
    // track-devices process stays alive on a stable list, so without the
    // guard this join would hang.
    watcher.stop();
}

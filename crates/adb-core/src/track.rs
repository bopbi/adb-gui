use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::device::{Device, DeviceState, parse_devices};

const RESPAWN_BACKOFF: Duration = Duration::from_secs(1);

/// Start a background thread running `adb track-devices` and stream device
/// list snapshots over the returned channel. The watcher kills the adb
/// child and exits when the receiver is dropped, and respawns adb if the
/// track-devices process dies.
pub fn spawn_device_watcher(adb_path: PathBuf) -> (JoinHandle<()>, Receiver<Vec<Device>>) {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || watcher_loop(adb_path, tx));
    (handle, rx)
}

fn watcher_loop(adb_path: PathBuf, tx: mpsc::Sender<Vec<Device>>) {
    loop {
        match run_watcher(&adb_path, &tx) {
            Ok(()) => {
                // adb process exited (crash, server kill): brief backoff,
                // then respawn. If the receiver is gone the next send
                // fails and we exit.
                std::thread::sleep(RESPAWN_BACKOFF);
            }
            Err(()) => return, // receiver disconnected
        }
    }
}

/// Returns Ok(()) when the adb child exits, Err(()) when the receiver is gone.
fn run_watcher(adb_path: &PathBuf, tx: &mpsc::Sender<Vec<Device>>) -> Result<(), ()> {
    let mut child: Child = std::process::Command::new(adb_path)
        .args(["track-devices"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;

    let stdout = child.stdout.take().expect("piped stdout");
    let mut devices: BTreeMap<String, DeviceState> = BTreeMap::new();

    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break }; // EOF or read error: respawn
        if !apply_line(&mut devices, &line) {
            continue; // blank/unparseable: no change
        }
        let snapshot: Vec<Device> = devices
            .iter()
            .map(|(serial, state)| Device {
                serial: serial.clone(),
                state: *state,
                model: None,
            })
            .collect();
        // Send failure means the receiver was dropped: kill adb and exit.
        if tx.send(snapshot).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(());
        }
    }

    // EOF: adb exited. Probe the channel once so a dropped receiver ends
    // the thread even when no further device changes arrive.
    if tx.send(snapshot_of(&devices)).is_err() {
        return Err(());
    }
    Ok(())
}

fn snapshot_of(devices: &BTreeMap<String, DeviceState>) -> Vec<Device> {
    devices
        .iter()
        .map(|(serial, state)| Device {
            serial: serial.clone(),
            state: *state,
            model: None,
        })
        .collect()
}

/// Fold one `track-devices` line (`serial\tstate`) into the accumulated
/// device map. Returns true when the map changed.
///
/// adb sends a full list on connect and one line per change afterwards, so
/// upserting converges without frame boundaries. A device that vanishes is
/// reported by adb with an `offline` line before removal from its own view,
/// so the snapshot keeps the last known state until the watcher respawns.
fn apply_line(devices: &mut BTreeMap<String, DeviceState>, line: &str) -> bool {
    let mut parsed = parse_devices(line);
    if parsed.len() != 1 {
        return false;
    }
    let device = parsed.remove(0);
    match devices.get(&device.serial) {
        Some(existing) if *existing == device.state => false,
        _ => {
            devices.insert(device.serial, device.state);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn script(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("adb-gui-track-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn apply_line_upserts_and_reports_change() {
        let mut map = BTreeMap::new();
        assert!(apply_line(&mut map, "emulator-5554\tdevice"));
        assert_eq!(map.get("emulator-5554"), Some(&DeviceState::Device));

        // Same state again: no change reported.
        assert!(!apply_line(&mut map, "emulator-5554\tdevice"));

        // State transition: change reported.
        assert!(apply_line(&mut map, "emulator-5554\toffline"));
        assert_eq!(map.get("emulator-5554"), Some(&DeviceState::Offline));

        // Second device added.
        assert!(apply_line(&mut map, "phone-1\tdevice"));
        assert_eq!(map.len(), 2);

        // Garbage lines ignored.
        assert!(!apply_line(&mut map, ""));
        assert!(!apply_line(&mut map, "nonsense words here"));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn watcher_streams_snapshots_and_exits_when_receiver_drops() {
        // The fake adb prints an initial list, then one state change, then
        // exits; the watcher must stream both snapshots and end the thread
        // when the receiver is dropped (detected at the post-EOF probe).
        let adb = script(
            "fake-track-devices",
            r#"printf 'emulator-5554\tdevice\nphone-1\tdevice\n'
sleep 0.2
printf 'phone-1\toffline\n'
exit 0"#,
        );

        let (handle, rx) = spawn_device_watcher(adb);

        // adb sends one line per change, so snapshots arrive incrementally;
        // wait until the offline transition lands.
        let mut saw_full_list = false;
        let mut saw_offline = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !saw_offline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let snapshot = rx.recv_timeout(remaining).expect("snapshot timed out");
            if snapshot.len() == 2 {
                saw_full_list = true;
            }
            if let Some(phone) = snapshot.iter().find(|d| d.serial == "phone-1") {
                if phone.state == DeviceState::Offline {
                    saw_offline = true;
                }
            }
        }
        assert!(saw_full_list, "never saw the two-device snapshot");
        assert!(saw_offline, "never saw the offline transition");

        drop(rx); // receiver gone: watcher must exit at the next send
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done_tx.send(handle.join());
        });
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("watcher thread did not exit after receiver drop")
            .expect("watcher thread panicked");
    }

    #[test]
    fn watcher_respawns_dead_adb() {
        let counter =
            std::env::temp_dir().join(format!("adb-gui-track-counter-{}", std::process::id()));
        let _ = fs::remove_file(&counter);
        let adb = script(
            "fake-track-respawn",
            r#"echo x >> "$ADB_GUI_TRACK_COUNTER"
printf 'emulator-5554\tdevice\n'
exit 0"#,
        );
        // Safety: read only by the fake adb spawned by this test.
        unsafe { std::env::set_var("ADB_GUI_TRACK_COUNTER", &counter) };

        let (handle, rx) = spawn_device_watcher(adb);
        rx.recv().expect("initial snapshot");

        // adb exits after each print; the watcher should respawn it.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut invocations = 1u32;
        while invocations < 2 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
            invocations = fs::read_to_string(&counter)
                .map(|s| s.lines().count() as u32)
                .unwrap_or(0);
        }
        assert!(
            invocations >= 2,
            "adb was respawned only {invocations} time(s)"
        );

        drop(rx);
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done_tx.send(handle.join());
        });
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("watcher did not exit")
            .expect("watcher panicked");
        unsafe { std::env::remove_var("ADB_GUI_TRACK_COUNTER") };
        let _ = fs::remove_file(&counter);
    }
}

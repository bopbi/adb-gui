use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::device::{Device, parse_devices};

const RESPAWN_BACKOFF: Duration = Duration::from_secs(1);

/// Guard for a running device watcher. `stop()` (or dropping the guard)
/// kills the current adb child, so the watcher's blocked read sees EOF and
/// the thread exits even when no device change ever arrives. `stop()` also
/// joins the thread; dropping without stopping shuts adb down but does not
/// wait for the thread.
pub struct DeviceWatcher {
    handle: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl DeviceWatcher {
    /// Shut the watcher down and wait for its thread to exit.
    pub fn stop(mut self) {
        self.initiate_shutdown();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    fn initiate_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let mut slot = self.child.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(child) = slot.as_mut() {
            let _ = child.kill();
        }
    }
}

impl Drop for DeviceWatcher {
    fn drop(&mut self) {
        self.initiate_shutdown();
    }
}

/// Start a background thread running `adb track-devices` and stream device
/// list snapshots over the returned channel. The returned guard stops the
/// watcher deterministically (`stop()` or Drop); a dropped receiver remains
/// a fallback exit path. The watcher respawns adb if the track-devices
/// process dies, unless shutdown was requested.
pub fn spawn_device_watcher(adb_path: PathBuf) -> (DeviceWatcher, Receiver<Vec<Device>>) {
    let (tx, rx) = mpsc::channel();
    let shutdown = Arc::new(AtomicBool::new(false));
    let child = Arc::new(Mutex::new(None));
    let handle = std::thread::spawn({
        let shutdown = shutdown.clone();
        let child = child.clone();
        move || watcher_loop(adb_path, tx, shutdown, child)
    });
    let watcher = DeviceWatcher {
        handle: Some(handle),
        shutdown,
        child,
    };
    (watcher, rx)
}

fn watcher_loop(
    adb_path: PathBuf,
    tx: mpsc::Sender<Vec<Device>>,
    shutdown: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        match run_watcher(&adb_path, &tx, &shutdown, &child) {
            Ok(()) => {
                // adb process exited (crash, server kill, or stop() kill):
                // brief backoff, then respawn — unless shutdown was requested.
                // If the receiver is gone the next send fails and we exit.
                std::thread::sleep(RESPAWN_BACKOFF);
            }
            Err(()) => return, // receiver disconnected
        }
    }
}

/// Returns Ok(()) when the adb child exits, Err(()) when the receiver is gone.
fn run_watcher(
    adb_path: &PathBuf,
    tx: &mpsc::Sender<Vec<Device>>,
    shutdown: &AtomicBool,
    shared_child: &Mutex<Option<Child>>,
) -> Result<(), ()> {
    let mut child: Child = std::process::Command::new(adb_path)
        .args(["track-devices"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;

    let stdout = child.stdout.take().expect("piped stdout");
    // Publish the child so stop() can kill it while the read below blocks;
    // if shutdown raced the spawn, kill immediately instead of blocking.
    {
        let mut slot = shared_child.lock().unwrap_or_else(|e| e.into_inner());
        if shutdown.load(Ordering::SeqCst) {
            let _ = child.kill();
        }
        *slot = Some(child);
    }
    let mut reader = BufReader::new(stdout);

    let result = loop {
        // Every frame is the COMPLETE current device list: the payload of a
        // frame replaces the snapshot wholesale, so a device absent from a
        // frame is gone (AOSP SERVICES.TXT: track-devices sends a full
        // device-list description, hex-length-framed, on every change).
        let snapshot = match read_frame(&mut reader) {
            Ok(Some(payload)) => parse_devices(&String::from_utf8_lossy(&payload)),
            // EOF: adb exited (or stop() killed it). Probe the channel once
            // so a dropped receiver ends the thread even when no further
            // device changes arrive.
            Ok(None) => break probe(tx),
            // Malformed frame or read error: respawn adb.
            Err(()) => break Ok(()),
        };
        // Send failure means the receiver was dropped: kill adb and exit.
        if tx.send(snapshot).is_err() {
            break Err(());
        }
    };

    // Reap the child whether it exited, was killed by stop(), or is still
    // running because the receiver went away.
    if let Some(mut child) = shared_child
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

/// Post-EOF probe: report one (possibly empty) snapshot so a receiver that
/// was dropped while adb idles still ends the watcher.
fn probe(tx: &mpsc::Sender<Vec<Device>>) -> Result<(), ()> {
    if tx.send(Vec::new()).is_err() {
        return Err(());
    }
    Ok(())
}

enum FrameError {
    /// Clean EOF before any frame byte.
    Eof,
    /// EOF mid-frame, a bad length prefix, or a read error.
    Malformed,
}

/// Read one hex-length-framed message: 4 ASCII hex chars giving the payload
/// byte count, then exactly that many payload bytes. Returns Ok(None) on a
/// clean EOF between frames.
fn read_frame(reader: &mut impl Read) -> Result<Option<Vec<u8>>, ()> {
    match read_frame_inner(reader) {
        Ok(payload) => Ok(Some(payload)),
        Err(FrameError::Eof) => Ok(None),
        Err(FrameError::Malformed) => Err(()),
    }
}

fn read_frame_inner(reader: &mut impl Read) -> Result<Vec<u8>, FrameError> {
    let mut header = [0u8; 4];
    read_up_to(reader, &mut header)?;
    let length = std::str::from_utf8(&header)
        .ok()
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .ok_or(FrameError::Malformed)? as usize;

    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|_| FrameError::Malformed)?;
    Ok(payload)
}

/// Read exactly `buf.len()` bytes; distinguish a clean EOF at the frame
/// boundary (no bytes read) from EOF mid-frame.
fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> Result<(), FrameError> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Err(FrameError::Eof),
            Ok(0) => return Err(FrameError::Malformed),
            Ok(n) => filled += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(FrameError::Malformed),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceState;

    #[test]
    fn read_frame_parses_hex_length_prefix() {
        let mut stream: &[u8] = b"001632081FDH2000CY\tdevice\n";
        let payload = read_frame(&mut stream).unwrap().expect("one frame");
        assert_eq!(payload, b"32081FDH2000CY\tdevice\n");
        // Stream fully consumed: next read is a clean EOF.
        assert!(read_frame(&mut stream).unwrap().is_none());
    }

    #[test]
    fn read_frame_handles_multi_device_and_empty_payloads() {
        let payload = "32081FDH2000CY\tdevice\nemulator-5554\tdevice\n";
        assert_eq!(payload.len(), 0x2B);
        let framed = format!("{:04x}{payload}", payload.len());
        let mut stream: &[u8] = framed.as_bytes();
        let frame = read_frame(&mut stream).unwrap().expect("frame");
        assert_eq!(frame, payload.as_bytes());

        // Only the FIRST line of a frame carries the prefix; the others must
        // stay glued to their own serials.
        let parsed = parse_devices(&String::from_utf8_lossy(&frame));
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].serial, "32081FDH2000CY");
        assert_eq!(parsed[1].serial, "emulator-5554");

        // A 0000 frame means "no devices" and must parse as an empty list.
        let mut stream: &[u8] = b"0000";
        let frame = read_frame(&mut stream).unwrap().expect("frame");
        assert!(parse_devices(&String::from_utf8_lossy(&frame)).is_empty());
    }

    #[test]
    fn read_frame_rejects_malformed_streams() {
        // Non-hex length prefix.
        let mut stream: &[u8] = b"zz16payload";
        assert!(read_frame(&mut stream).is_err());
        // Truncated payload.
        let mut stream: &[u8] = b"0016abc";
        assert!(read_frame(&mut stream).is_err());
        // Truncated header mid-frame is malformed, not clean EOF.
        let mut stream: &[u8] = b"00";
        assert!(read_frame(&mut stream).is_err());
    }

    // The watcher tests drive a fake adb shell script; they are Unix-only
    // (matching client.rs's `#[cfg(all(test, unix))]`). The pure frame tests
    // above stay cross-platform.
    #[cfg(unix)]
    mod watcher {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        fn script(name: &str, body: &str) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("adb-gui-track-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
            path
        }

        /// Wait for snapshots until `predicate` holds or the deadline passes.
        fn collect_until(
            rx: &Receiver<Vec<Device>>,
            deadline: std::time::Instant,
            mut predicate: impl FnMut(&[Device]) -> bool,
        ) -> Vec<Device> {
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                let snapshot = rx.recv_timeout(remaining).expect("snapshot timed out");
                if predicate(&snapshot) {
                    return snapshot;
                }
            }
        }

        fn exit_after_receiver_drop(watcher: DeviceWatcher, rx: Receiver<Vec<Device>>) {
            drop(rx); // receiver gone: watcher must exit at the next send
            stop_within(watcher, "watcher thread did not exit after receiver drop");
        }

        /// Stop the watcher on a helper thread and fail if it takes too long
        /// (a hung join would otherwise block the test forever).
        fn stop_within(watcher: DeviceWatcher, message: &str) {
            let (done_tx, done_rx) = mpsc::channel();
            std::thread::spawn(move || {
                watcher.stop();
                let _ = done_tx.send(());
            });
            done_rx
                .recv_timeout(Duration::from_secs(10))
                .expect(message);
        }

        #[test]
        fn watcher_streams_framed_snapshots_with_clean_serials() {
            // Real track-devices framing: 4 hex chars + payload, where the
            // payload is the complete device list. The first frame lists two
            // devices; the second frame lists them again with one offline.
            // Regression: parsed serials must never contain the hex prefix.
            let adb = script(
                "fake-track-framed",
                r#"printf '002B'; printf '32081FDH2000CY\tdevice\nemulator-5554\tdevice\n'
sleep 0.2
printf '002C'; printf '32081FDH2000CY\tdevice\nemulator-5554\toffline\n'
exit 0"#,
            );

            let (watcher, rx) = spawn_device_watcher(adb);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);

            let first = collect_until(&rx, deadline, |snapshot| snapshot.len() == 2);
            assert_eq!(first.len(), 2);
            assert!(
                first
                    .iter()
                    .any(|d| d.serial == "32081FDH2000CY" && d.state == DeviceState::Device),
                "physical device must appear with its clean serial, got {first:?}"
            );
            assert!(
                first
                    .iter()
                    .all(|d| !d.serial.starts_with("0016") && !d.serial.starts_with("002B")),
                "no serial may carry the frame's hex prefix"
            );

            let second = collect_until(&rx, deadline, |snapshot| {
                snapshot
                    .iter()
                    .any(|d| d.serial == "emulator-5554" && d.state == DeviceState::Offline)
            });
            assert_eq!(second.len(), 2, "a frame is the complete list");
            assert!(
                second.iter().all(|d| !d.serial.starts_with("002C")),
                "later frames must not leak their prefix either"
            );

            exit_after_receiver_drop(watcher, rx);
        }

        #[test]
        fn watcher_replaces_snapshot_across_frames() {
            // Frame 1 lists two devices; frame 2 drops phone-1 entirely.
            // Replacement semantics: the absent device must vanish from the
            // snapshot (no upsert of stale entries).
            let adb = script(
                "fake-track-removal",
                r#"printf '002B'; printf '32081FDH2000CY\tdevice\nemulator-5554\tdevice\n'
sleep 0.2
printf '0015'; printf 'emulator-5554\tdevice\n'
exit 0"#,
            );

            let (watcher, rx) = spawn_device_watcher(adb);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);

            let first = collect_until(&rx, deadline, |snapshot| snapshot.len() == 2);
            assert!(first.iter().any(|d| d.serial == "32081FDH2000CY"));

            let second = collect_until(&rx, deadline, |snapshot| {
                snapshot.len() == 1 && snapshot[0].serial == "emulator-5554"
            });
            assert_eq!(second.len(), 1);
            assert_eq!(second[0].state, DeviceState::Device);

            exit_after_receiver_drop(watcher, rx);
        }

        #[test]
        fn watcher_streams_empty_frame_when_all_devices_disconnect() {
            let adb = script(
                "fake-track-empty",
                r#"printf '0016'; printf '32081FDH2000CY\tdevice\n'
sleep 0.2
printf '0000'
exit 0"#,
            );

            let (watcher, rx) = spawn_device_watcher(adb);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);

            collect_until(&rx, deadline, |snapshot| snapshot.len() == 1);
            let empty = collect_until(&rx, deadline, |snapshot| snapshot.is_empty());
            assert!(empty.is_empty());

            exit_after_receiver_drop(watcher, rx);
        }

        #[test]
        fn watcher_respawns_dead_adb() {
            let counter =
                std::env::temp_dir().join(format!("adb-gui-track-counter-{}", std::process::id()));
            let _ = fs::remove_file(&counter);
            let adb = script(
                "fake-track-respawn",
                &format!(
                    "echo x >> \"{counter}\"\nprintf '0015'; printf 'emulator-5554\\tdevice\\n'\nexit 0",
                    counter = counter.display(),
                ),
            );

            let (watcher, rx) = spawn_device_watcher(adb);
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

            let _ = fs::remove_file(&counter);
            exit_after_receiver_drop(watcher, rx);
        }

        #[test]
        fn stop_ends_a_silent_watcher_promptly() {
            // The PM1 defect: with a stable device list no frame arrives, so
            // the old watcher stayed blocked in read_frame forever. The fake
            // here never writes and never exits; stop() must kill the child
            // so the blocked read sees EOF and the thread exits promptly.
            // `exec` makes sleep the child process itself, so killing the
            // child is not masked by a shell waiting on a grandchild.
            let adb = script("fake-track-silent", "exec sleep 60");

            let (watcher, rx) = spawn_device_watcher(adb);
            // Keep the receiver alive: the exit must come from the guard,
            // not the receiver-drop fallback.
            stop_within(watcher, "watcher did not exit after stop()");
            drop(rx);
        }
    }
}

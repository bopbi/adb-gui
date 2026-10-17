//! Screen recording: start/stop `screenrecord` on the device, then pull the
//! mp4 to the Pictures dir and delete the remote copy. The recording runs in
//! a long-lived child owned by this feature (kill-on-drop), NOT through the
//! 30 s watchdog path.

use std::sync::Mutex;

use adb_core::{AdbClient, AdbError, ShellChild};

use crate::feature::{Feature, FeatureInput, FeatureKind};
use crate::features::pictures;

pub const SCREEN_RECORD_ID: &str = "capture.record";
pub const REMOTE_PATH: &str = "/sdcard/adb-gui-rec.mp4";

pub struct ScreenRecord {
    /// Some while a recording is running; the guard kills the device-side
    /// process on stop or drop.
    child: Mutex<Option<ShellChild>>,
    /// User-facing note from the last perform (where the mp4 landed when
    /// the Pictures dir was unavailable), awaiting pickup by the app layer.
    last_notice: Mutex<Option<String>>,
}

impl ScreenRecord {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            last_notice: Mutex::new(None),
        }
    }
}

impl Feature for ScreenRecord {
    fn id(&self) -> &'static str {
        SCREEN_RECORD_ID
    }

    fn title(&self) -> &'static str {
        "Record screen"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Action
    }

    /// A user-facing note from the last stop (where the mp4 landed when the
    /// Pictures dir was unavailable).
    fn notice(&self) -> Option<String> {
        self.last_notice.lock().unwrap().take()
    }

    /// Click toggles: not recording → start; recording → stop, pull, cleanup.
    /// A recording that hit screenrecord's 3-minute cap still produces a
    /// complete file, so stopping afterwards is not an error.
    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        _input: FeatureInput,
    ) -> Result<(), AdbError> {
        let mut guard = self.child.lock().unwrap();
        if let Some(mut child) = guard.take() {
            child.stop()?;
            let filename = pictures::timestamped_filename("recording", "mp4");
            let (local, fell_back) = pictures::save_target(&pictures::pictures_dir(), &filename);
            client.pull(serial, REMOTE_PATH, &local.to_string_lossy())?;
            client.shell(serial, &["rm", REMOTE_PATH])?;
            *self.last_notice.lock().unwrap() = fell_back.then(|| {
                format!(
                    "Pictures dir unavailable; recording saved to {}",
                    local.display()
                )
            });
            Ok(())
        } else {
            let child = client.spawn_shell_child(serial, &["screenrecord", REMOTE_PATH])?;
            *guard = Some(child);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_idle() {
        let feature = ScreenRecord::new();
        assert!(feature.child.lock().unwrap().is_none());
    }

    // Lifecycle tests drive a fake adb (Unix-only).
    #[cfg(all(test, unix))]
    mod lifecycle {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;

        /// Fake adb: `screenrecord` sleeps (a stand-in for the device-side
        /// process); logs every argv; answers `pull` by writing a marker file
        /// so the local pull target exists.
        fn fake_adb(name: &str, log: &std::path::Path, pulled_marker: &std::path::Path) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("adb-gui-record-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{log}\"\n\
                     case \"$*\" in\n\
                     *screenrecord*) sleep 60 ;;\n\
                     *\" pull \"*) touch \"{marker}\" ;;\n\
                     esac\n\
                     exit 0\n",
                    log = log.display(),
                    marker = pulled_marker.display(),
                ),
            )
            .unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
            path
        }

        #[test]
        fn start_then_stop_pulls_and_cleans_up() {
            let _env = crate::features::test_util::CAPTURE_DIR_LOCK.lock().unwrap();
            let pid = std::process::id();
            let log = std::env::temp_dir().join(format!("adb-gui-rec-log-{pid}"));
            let marker = std::env::temp_dir().join(format!("adb-gui-rec-pulled-{pid}"));
            let _ = fs::remove_file(&log);
            let _ = fs::remove_file(&marker);
            // Keep pulled recordings out of the developer's Pictures dir.
            crate::features::test_util::use_scratch_capture_dir_locked("record");
            let client = AdbClient::new(fake_adb("fake-adb-record", &log, &marker));
            let feature = ScreenRecord::new();

            feature
                .perform(&client, "serial123", FeatureInput::None)
                .unwrap();
            assert!(
                feature.child.lock().unwrap().is_some(),
                "first click starts recording"
            );
            // spawn is async: wait for the child script to log its argv
            // before stopping, or the kill can land before its first write.
            wait_for_log(&log, "screenrecord");

            feature
                .perform(&client, "serial123", FeatureInput::None)
                .unwrap();
            assert!(
                feature.child.lock().unwrap().is_none(),
                "second click stops it"
            );

            let logged = fs::read_to_string(&log).unwrap();
            assert!(
                logged.contains("shell screenrecord /sdcard/adb-gui-rec.mp4"),
                "record must spawn a device-side screenrecord; log: {logged}"
            );
            assert!(
                logged.contains(&format!("pull {REMOTE_PATH}")),
                "stop must pull the recording; log: {logged}"
            );
            assert!(
                logged.contains(&format!("shell rm {REMOTE_PATH}")),
                "stop must delete the remote file; log: {logged}"
            );
            let _ = fs::remove_file(&log);
            let _ = fs::remove_file(&marker);
        }

        /// Poll the fake-adb log until `needle` appears (the child script is
        /// spawned concurrently and may write a beat after perform returns).
        fn wait_for_log(log: &std::path::Path, needle: &str) {
            for _ in 0..100 {
                if fs::read_to_string(log)
                    .map(|text| text.contains(needle))
                    .unwrap_or(false)
                {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            panic!(
                "log never contained {needle:?}: {:?}",
                fs::read_to_string(log)
            );
        }

        /// K2: when the Pictures dir is unavailable, stopping still pulls (to
        /// the temp dir) and stashes a notice naming the path.
        #[test]
        fn stop_falls_back_and_stashes_a_notice() {
            let _env = crate::features::test_util::CAPTURE_DIR_LOCK.lock().unwrap();
            let pid = std::process::id();
            let log = std::env::temp_dir().join(format!("adb-gui-rec-fallback-log-{pid}"));
            let marker = std::env::temp_dir().join(format!("adb-gui-rec-fallback-{pid}"));
            let _ = fs::remove_file(&log);
            let _ = fs::remove_file(&marker);
            let home =
                crate::features::test_util::use_scratch_capture_dir_locked("record-fallback");
            fs::write(home.join("Pictures"), b"not a dir").unwrap();
            let client = AdbClient::new(fake_adb("fake-adb-record-fallback", &log, &marker));
            let feature = ScreenRecord::new();

            feature
                .perform(&client, "serial123", FeatureInput::None)
                .unwrap();
            wait_for_log(&log, "screenrecord");
            feature
                .perform(&client, "serial123", FeatureInput::None)
                .unwrap();

            let notice = feature.notice().expect("fallback must surface a notice");
            assert!(
                notice.contains(std::env::temp_dir().to_string_lossy().as_ref()),
                "notice must name the temp-dir landing path: {notice}"
            );
            let _ = fs::remove_file(&log);
            let _ = fs::remove_file(&marker);
        }

        #[test]
        fn dropped_feature_kills_the_recording() {
            let pid = std::process::id();
            let log = std::env::temp_dir().join(format!("adb-gui-rec-drop-log-{pid}"));
            let marker = std::env::temp_dir().join(format!("adb-gui-rec-drop-{pid}"));
            let _ = fs::remove_file(&log);
            let _ = fs::remove_file(&marker);
            let client = AdbClient::new(fake_adb("fake-adb-record-drop", &log, &marker));

            let feature = ScreenRecord::new();
            feature
                .perform(&client, "serial123", FeatureInput::None)
                .unwrap();
            let child_pid = feature
                .child
                .lock()
                .unwrap()
                .as_ref()
                .and_then(ShellChild::pid)
                .expect("recording child has a pid");

            drop(feature);
            // The kill is async from the test's perspective; give it a moment.
            let mut alive = true;
            for _ in 0..50 {
                alive = std::process::Command::new("kill")
                    .args(["-0", &child_pid.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                if !alive {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            assert!(!alive, "dropping the feature must kill the recording");
            let _ = fs::remove_file(&log);
            let _ = fs::remove_file(&marker);
        }
    }
}

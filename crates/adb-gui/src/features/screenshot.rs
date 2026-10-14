//! Screenshot: `exec-out screencap -p` → PNG file in the Pictures dir, and
//! the image handed to the app layer for the system clipboard.

use std::sync::Mutex;

use adb_core::{AdbClient, AdbError};

use crate::feature::{ClipboardPayload, Feature, FeatureInput, FeatureKind};
use crate::features::pictures;

pub const SCREENSHOT_ID: &str = "capture.screenshot";

pub struct Screenshot {
    /// The PNG from the most recent perform, awaiting pickup by the app
    /// layer's clipboard writer.
    last_png: Mutex<Option<Vec<u8>>>,
    /// User-facing note from the last perform (where the file landed when
    /// the Pictures dir was unavailable), awaiting pickup by the app layer.
    last_notice: Mutex<Option<String>>,
}

impl Screenshot {
    pub fn new() -> Self {
        Self {
            last_png: Mutex::new(None),
            last_notice: Mutex::new(None),
        }
    }
}

impl Feature for Screenshot {
    fn id(&self) -> &'static str {
        SCREENSHOT_ID
    }

    fn title(&self) -> &'static str {
        "Screenshot"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Action
    }

    fn clipboard_payload(&self) -> Option<ClipboardPayload> {
        self.last_png
            .lock()
            .unwrap()
            .take()
            .map(|bytes| ClipboardPayload {
                format: "png",
                bytes,
            })
    }

    fn notice(&self) -> Option<String> {
        self.last_notice.lock().unwrap().take()
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        if input != FeatureInput::None {
            return Ok(());
        }
        let png = client.exec_out(serial, &["screencap", "-p"])?;
        if png.is_empty() {
            return Err(AdbError::CommandFailed {
                command: "screencap -p".to_string(),
                stderr: "device returned no image bytes".to_string(),
            });
        }
        let filename = pictures::timestamped_filename("screenshot", "png");
        let (path, fell_back) =
            pictures::save_with_fallback(&pictures::pictures_dir(), &filename, &png)?;
        *self.last_notice.lock().unwrap() = fell_back.then(|| {
            format!(
                "Pictures dir unavailable; screenshot saved to {}",
                path.display()
            )
        });
        *self.last_png.lock().unwrap() = Some(png);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_payload_yields_the_performed_png_once() {
        let feature = Screenshot::new();
        assert!(feature.clipboard_payload().is_none());
        *feature.last_png.lock().unwrap() = Some(b"png-bytes".to_vec());
        let payload = feature.clipboard_payload().expect("payload after perform");
        assert_eq!(payload.format, "png");
        assert_eq!(payload.bytes, b"png-bytes");
        // Taken: a second pickup does not re-copy the same shot.
        assert!(feature.clipboard_payload().is_none());
    }

    // argv tests drive a fake adb (Unix-only). The pure payload test above
    // stays cross-platform.
    #[cfg(all(test, unix))]
    mod argv {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;

        fn fake_adb(name: &str, log: &std::path::Path) -> PathBuf {
            let dir = std::env::temp_dir()
                .join(format!("adb-gui-screenshot-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\n\
                     if [ \"$3\" = \"exec-out\" ]; then printf '\\x89PNG\\r\\n\\x1a\\nfake'; fi\n\
                     exit 0\n",
                    log.display()
                ),
            )
            .unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
            path
        }

        #[test]
        fn perform_captures_via_exec_out_and_saves_png() {
            let _env = crate::features::test_util::ENV_LOCK.lock().unwrap();
            let log = std::env::temp_dir().join(format!("adb-gui-shot-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            // Keep the saved PNG out of the developer's Pictures dir.
            let home = crate::features::test_util::use_scratch_home_locked("screenshot");
            let client = AdbClient::new(fake_adb("fake-adb-shot", &log));
            let feature = Screenshot::new();

            feature
                .perform(&client, "serial123", FeatureInput::None)
                .unwrap();

            let logged = fs::read_to_string(&log).unwrap();
            assert!(
                logged.contains("-s serial123 exec-out screencap -p"),
                "screenshot must use binary-safe exec-out; log: {logged}"
            );
            let payload = feature.clipboard_payload().expect("png captured");
            assert!(payload.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
            // No fallback: no notice.
            assert!(feature.notice().is_none());
            // The file landed in the scratch Pictures dir.
            let saved = home.join("Pictures").join("adb-gui");
            assert!(
                saved
                    .read_dir()
                    .map(|mut entries| entries.next().is_some())
                    .unwrap_or(false),
                "a timestamped PNG must be saved under {saved:?}"
            );
            let _ = fs::remove_dir_all(&saved);
            let _ = fs::remove_file(&log);
        }

        /// K2: when the Pictures dir is unavailable, the screenshot still
        /// saves (to the temp dir) and stashes a notice naming the path.
        #[test]
        fn perform_falls_back_and_stashes_a_notice() {
            let _env = crate::features::test_util::ENV_LOCK.lock().unwrap();
            let log = std::env::temp_dir()
                .join(format!("adb-gui-shot-fallback-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let home = crate::features::test_util::use_scratch_home_locked("shot-fallback");
            // A file where the Pictures directory would be makes Pictures
            // unwritable.
            fs::write(home.join("Pictures"), b"not a dir").unwrap();
            let client = AdbClient::new(fake_adb("fake-adb-shot-fallback", &log));
            let feature = Screenshot::new();

            feature
                .perform(&client, "serial123", FeatureInput::None)
                .unwrap();

            let notice = feature.notice().expect("fallback must surface a notice");
            assert!(
                notice.contains(std::env::temp_dir().to_string_lossy().as_ref()),
                "notice must name the temp-dir landing path: {notice}"
            );
            // The clipboard payload is unaffected by the fallback.
            assert!(feature.clipboard_payload().is_some());
            let _ = fs::remove_file(&log);
        }
    }
}

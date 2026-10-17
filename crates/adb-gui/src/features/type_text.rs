//! Type text: sends the input through `input text` with POSIX single-quote
//! escaping so spaces/quotes/metachars land literally on the device.

use adb_core::{AdbClient, AdbError, shell_quote::quote_for_shell};

use crate::feature::{Feature, FeatureInput, FeatureKind};

pub const TYPE_TEXT_ID: &str = "input.type-text";

pub struct TypeText;

impl Feature for TypeText {
    fn id(&self) -> &'static str {
        TYPE_TEXT_ID
    }

    fn title(&self) -> &'static str {
        "Type text"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::TextAction
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Text(text) = input else {
            return Ok(());
        };
        if text.is_empty() {
            return Ok(());
        }
        // AOSP's `input text` turns the substring `%s` into a space, so no
        // single invocation may contain one: each `%` goes out on its own.
        for segment in segments_around_percent(&text) {
            let command = format!("input text {}", quote_for_shell(segment));
            client.shell(serial, &[&command])?;
        }
        Ok(())
    }
}

/// Split `text` so every `%` is its own segment and the text between
/// percents stays intact; empty segments are skipped.
fn segments_around_percent(text: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    for (ix, _) in text.match_indices('%') {
        if ix > start {
            segments.push(&text[start..ix]);
        }
        segments.push(&text[ix..=ix]);
        start = ix + 1;
    }
    if start < text.len() {
        segments.push(&text[start..]);
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    // The quoting tests drive a fake adb shell script (Unix-only, matching
    // adb-core's client tests).
    #[cfg(unix)]
    mod perform {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        /// Fake adb that logs every argv it receives. The script name derives
        /// from the log path: tests run in parallel within one process and
        /// share the fixture dir, so each test needs its own script file.
        fn fake_adb(log: &Path) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("adb-gui-type-text-test-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let name = format!("fake-adb-{}", log.file_stem().unwrap().to_string_lossy());
            let path = dir.join(name);
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\n",
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
        fn perform_quotes_text_for_the_device_shell() {
            let log = std::env::temp_dir().join(format!("adb-gui-tt-log-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb(&log));
            let feature = TypeText;

            feature
                .perform(
                    &client,
                    "serial123",
                    FeatureInput::Text("hello world".to_string()),
                )
                .unwrap();
            feature
                .perform(
                    &client,
                    "serial123",
                    FeatureInput::Text("it's $(rm -rf /) \"quoted\"".to_string()),
                )
                .unwrap();

            let lines: Vec<String> = fs::read_to_string(&log)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            assert_eq!(
                lines,
                [
                    "-s serial123 shell input text 'hello world'",
                    "-s serial123 shell input text 'it'\\''s $(rm -rf /) \"quoted\"'",
                ],
                "text must be single-quote quoted so the device shell cannot expand it"
            );
            let _ = fs::remove_file(&log);
        }

        #[test]
        fn perform_splits_percent_so_input_text_never_sees_percent_s() {
            let log = std::env::temp_dir().join(format!("adb-gui-tt-log3-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb(&log));
            let feature = TypeText;

            feature
                .perform(
                    &client,
                    "serial123",
                    FeatureInput::Text("value=%s".to_string()),
                )
                .unwrap();
            feature
                .perform(
                    &client,
                    "serial123",
                    FeatureInput::Text("100%%".to_string()),
                )
                .unwrap();

            let lines: Vec<String> = fs::read_to_string(&log)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            assert_eq!(
                lines,
                [
                    "-s serial123 shell input text 'value='",
                    "-s serial123 shell input text '%'",
                    "-s serial123 shell input text 's'",
                    "-s serial123 shell input text '100'",
                    "-s serial123 shell input text '%'",
                    "-s serial123 shell input text '%'",
                ],
            );
            let _ = fs::remove_file(&log);
        }

        #[test]
        fn perform_ignores_empty_and_non_text_input() {
            let log = std::env::temp_dir().join(format!("adb-gui-tt-log2-{}", std::process::id()));
            let _ = fs::remove_file(&log);
            let client = AdbClient::new(fake_adb(&log));
            let feature = TypeText;
            feature
                .perform(&client, "serial123", FeatureInput::Text(String::new()))
                .unwrap();
            feature
                .perform(&client, "serial123", FeatureInput::Bool(true))
                .unwrap();
            assert!(
                !log.exists(),
                "empty or non-text input must not touch the device"
            );
        }
    }
}

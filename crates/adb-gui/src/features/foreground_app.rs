//! Foreground-app detection: parse `dumpsys activity activities` for the
//! resumed activity. This is the most version-fragile parser in the app —
//! every app-action feature consumes this one function so OEM/API output
//! differences have a single point to fix.

use adb_core::{AdbClient, AdbError};

/// The foreground app's package, or a clear error when nothing is resumed.
pub fn current_package(client: &AdbClient, serial: &str) -> Result<String, AdbError> {
    let output = client.shell(serial, &["dumpsys", "activity", "activities"])?;
    parse_current_package(&output).ok_or_else(|| AdbError::CommandFailed {
        command: "dumpsys activity activities".to_string(),
        stderr: "no foreground app detected".to_string(),
    })
}

/// Shared perform body for foreground-app actions — the single consumption
/// point for [`current_package`]: resolve the foreground package, then run
/// the command `build` derives from it. No-ops on any input other than
/// `FeatureInput::None` (what action cells dispatch).
pub fn perform_action(
    client: &AdbClient,
    serial: &str,
    input: crate::feature::FeatureInput,
    build: fn(&str) -> Vec<String>,
) -> Result<(), AdbError> {
    if input != crate::feature::FeatureInput::None {
        return Ok(());
    }
    let package = current_package(client, serial)?;
    let owned: Vec<String> = build(&package);
    let argv: Vec<&str> = owned.iter().map(String::as_str).collect();
    client.shell(serial, &argv)?;
    Ok(())
}

/// Pure parser (unit-tested against API-level fixtures). Accepts both
/// `mResumedActivity` (older APIs) and `topResumedActivity` (API 29+), in
/// `package/.Activity` or `package/pkg.Activity` form. First match wins;
/// None when neither key is present.
pub fn parse_current_package(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        if !(line.contains("mResumedActivity") || line.contains("topResumedActivity")) {
            continue;
        }
        if let Some(package) = extract_package(line) {
            return Some(package);
        }
    }
    None
}

/// Find the `package/component` token on a resumed-activity line and return
/// the package part, rejecting tokens that cannot be package names.
fn extract_package(line: &str) -> Option<String> {
    for token in line.split_whitespace() {
        // The key and value may be glued (`topResumedActivity=ActivityRecord{…}`)
        // or split (`mResumedActivity:`); take what follows the last `:`/`=`.
        let token = token.rsplit([':', '=']).next().unwrap_or(token);
        if let Some((package, _component)) = token.split_once('/') {
            let plausible = !package.is_empty()
                && package
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
            if plausible {
                return Some(package.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mresumed_activity_form() {
        // API 26-28 style: `mResumedActivity: ActivityRecord{hash u0 pkg/.Act tN}`.
        let output = "Stack #0:\n\
                      \x20   mResumedActivity: ActivityRecord{3f5a2d1 u0 com.example.shop/.ui.CartActivity t123}\n\
                      \x20   mLastPausedActivity: ActivityRecord{...}\n";
        assert_eq!(
            parse_current_package(output).as_deref(),
            Some("com.example.shop")
        );
    }

    #[test]
    fn parses_topresumed_activity_form() {
        // API 29+ style: `topResumedActivity=ActivityRecord{hash u0 pkg/pkg.Act tN}`,
        // sometimes fully-qualified component.
        let output = "top display 0:\n\
                      \x20   topResumedActivity=ActivityRecord{a1b2c3 u0 com.oem.camera/com.oem.camera.MainActivity t7}\n";
        assert_eq!(
            parse_current_package(output).as_deref(),
            Some("com.oem.camera")
        );
    }

    #[test]
    fn missing_keys_yield_none() {
        let output = "Stack #0:\n  mLastPausedActivity: ActivityRecord{...}\n";
        assert_eq!(parse_current_package(output), None);
        assert_eq!(parse_current_package(""), None);
    }

    #[test]
    fn ignores_resumed_lines_without_package() {
        // A defunct record line that mentions the key but has no package.
        let output = "  mResumedActivity: null\n";
        assert_eq!(parse_current_package(output), None);
    }
}

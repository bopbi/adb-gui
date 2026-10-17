//! Foreground-app detection: parse `dumpsys activity activities` for the
//! resumed activity. This is the most version-fragile parser in the app —
//! every app-action feature consumes this one function so OEM/API output
//! differences have a single point to fix.

use adb_core::{AdbClient, AdbError};

/// The foreground app: its package and the Android user (`u<N>` in the
/// activity record) it runs as — work-profile apps run as a non-zero user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundApp {
    pub package: String,
    pub user: u32,
}

/// The foreground app, or a clear error when nothing is resumed.
pub fn current_app(client: &AdbClient, serial: &str) -> Result<ForegroundApp, AdbError> {
    let output = client.shell(serial, &["dumpsys", "activity", "activities"])?;
    parse_current_app(&output).ok_or_else(|| AdbError::CommandFailed {
        command: "dumpsys activity activities".to_string(),
        stderr: "no foreground app detected".to_string(),
    })
}

/// The foreground app's package, or a clear error when nothing is resumed.
pub fn current_package(client: &AdbClient, serial: &str) -> Result<String, AdbError> {
    current_app(client, serial).map(|app| app.package)
}

/// Shared perform body for foreground-app actions — the single consumption
/// point for [`current_app`]: resolve the foreground app, then run the
/// command `build` derives from it. No-ops on any input other than
/// `FeatureInput::None` (what action cells dispatch).
pub fn perform_action(
    client: &AdbClient,
    serial: &str,
    input: crate::feature::FeatureInput,
    build: fn(&ForegroundApp) -> Vec<String>,
) -> Result<(), AdbError> {
    if input != crate::feature::FeatureInput::None {
        return Ok(());
    }
    let app = current_app(client, serial)?;
    let owned: Vec<String> = build(&app);
    let argv: Vec<&str> = owned.iter().map(String::as_str).collect();
    client.shell(serial, &argv)?;
    Ok(())
}

/// Pure parser (unit-tested against API-level fixtures). Prefers the
/// focused `topResumedActivity` record (API 29+) anywhere in the dump — in
/// split-screen an unfocused stack's `mResumedActivity` can appear first —
/// and falls back to the first `mResumedActivity` (older APIs). Components
/// may be `package/.Activity` or `package/pkg.Activity`; the user is 0 when
/// the record has no `u<N>` token. None when neither key is present.
pub fn parse_current_app(output: &str) -> Option<ForegroundApp> {
    for key in ["topResumedActivity", "mResumedActivity"] {
        for line in output.lines() {
            let line = line.trim();
            if !line.contains(key) {
                continue;
            }
            if let Some(app) = extract_app(line) {
                return Some(app);
            }
        }
    }
    None
}

/// Find the `package/component` token on a resumed-activity line and return
/// the package (rejecting tokens that cannot be package names) together with
/// the record's user id.
fn extract_app(line: &str) -> Option<ForegroundApp> {
    let user = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix('u')?.parse::<u32>().ok())
        .unwrap_or(0);
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
                return Some(ForegroundApp {
                    package: package.to_string(),
                    user,
                });
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
            parse_current_app(output).map(|a| a.package).as_deref(),
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
            parse_current_app(output).map(|a| a.package).as_deref(),
            Some("com.oem.camera")
        );
    }

    #[test]
    fn missing_keys_yield_none() {
        let output = "Stack #0:\n  mLastPausedActivity: ActivityRecord{...}\n";
        assert_eq!(parse_current_app(output), None);
        assert_eq!(parse_current_app(""), None);
    }

    #[test]
    fn ignores_resumed_lines_without_package() {
        // A defunct record line that mentions the key but has no package.
        let output = "  mResumedActivity: null\n";
        assert_eq!(parse_current_app(output), None);
    }

    #[test]
    fn top_resumed_wins_over_an_earlier_unfocused_resumed_activity() {
        // Split-screen: the unfocused stack's mResumedActivity (work-profile
        // app) is listed before the focused topResumedActivity.
        let output = "Stack #2:\n\
                      \x20   mResumedActivity: ActivityRecord{1 u10 com.work.mail/.Main t9}\n\
                      top display 0:\n\
                      \x20   topResumedActivity=ActivityRecord{2 u0 com.example.focus/.Main t3}\n";
        assert_eq!(
            parse_current_app(output),
            Some(ForegroundApp {
                package: "com.example.focus".to_string(),
                user: 0
            })
        );
    }

    #[test]
    fn extracts_the_work_profile_user() {
        let output = "  topResumedActivity=ActivityRecord{2 u10 com.work.mail/.Main t3}\n";
        assert_eq!(
            parse_current_app(output),
            Some(ForegroundApp {
                package: "com.work.mail".to_string(),
                user: 10
            })
        );
    }

    #[test]
    fn user_defaults_to_zero_without_a_user_token() {
        let output = "  mResumedActivity: ActivityRecord{x com.example.shop/.Main t1}\n";
        assert_eq!(parse_current_app(output).map(|a| a.user), Some(0));
    }
}

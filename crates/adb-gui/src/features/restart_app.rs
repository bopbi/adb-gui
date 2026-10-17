//! Restart the foreground app: force-stop it, then relaunch its launcher
//! activity. The relaunch resolves the launcher component with
//! `cmd package resolve-activity` and starts it via `am start --user`, so an
//! app in a work profile restarts inside that profile (`monkey` has no
//! `--user` and would relaunch the personal copy).

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};
use crate::features::foreground_app;

pub const RESTART_ID: &str = "app.restart";

pub struct RestartApp;

impl Feature for RestartApp {
    fn id(&self) -> &'static str {
        RESTART_ID
    }

    fn title(&self) -> &'static str {
        "Restart app"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::Action
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
        let app = foreground_app::current_app(client, serial)?;
        let user = app.user.to_string();
        client.shell(serial, &["am", "force-stop", "--user", &user, &app.package])?;
        let resolved = client.shell(
            serial,
            &[
                "cmd",
                "package",
                "resolve-activity",
                "--user",
                &user,
                "--brief",
                "-a",
                "android.intent.action.MAIN",
                "-c",
                "android.intent.category.LAUNCHER",
                &app.package,
            ],
        )?;
        let component = resolve_component(&resolved).ok_or_else(|| AdbError::CommandFailed {
            command: "cmd package resolve-activity".to_string(),
            stderr: format!("no launcher activity found for {}", app.package),
        })?;
        client.shell(serial, &["am", "start", "--user", &user, "-n", &component])?;
        Ok(())
    }
}

/// The launcher component from `resolve-activity --brief` output: the last
/// non-empty line, in `package/.Activity` form. None when unparseable.
fn resolve_component(output: &str) -> Option<String> {
    let line = output
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())?;
    if line.contains('/') && !line.contains(char::is_whitespace) {
        Some(line.to_string())
    } else {
        None
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util::{self, FOREGROUND_PACKAGE};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// Fake adb whose `cmd package resolve-activity` answers unparseable
    /// garbage — `fake_adb_failing` cannot be used here because perform must
    /// get past `dumpsys` and reach the resolve before failing.
    fn fake_adb_no_launcher(name: &str, log: &Path) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("adb-gui-restart-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{log}\"\n\
                 if [ \"$4\" = \"dumpsys\" ]; then\n\
                 \x20 printf '  topResumedActivity=ActivityRecord{{a1b2c3 u0 {pkg}/.MainActivity t7}}\\n'\n\
                 fi\n\
                 if [ \"$4\" = \"cmd\" ]; then\n\
                 \x20 printf 'No activity found\\n'\n\
                 fi\n\
                 exit 0\n",
                log = log.display(),
                pkg = FOREGROUND_PACKAGE,
            ),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn restart_force_stops_then_relaunches_in_user_scope() {
        let log = test_util::log_path("restart");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-restart", &log));
        RestartApp
            .perform(&client, "s", FeatureInput::None)
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell dumpsys activity activities".to_string(),
                format!("-s s shell am force-stop --user 0 {FOREGROUND_PACKAGE}"),
                format!(
                    "-s s shell cmd package resolve-activity --user 0 --brief \
                     -a android.intent.action.MAIN -c android.intent.category.LAUNCHER {FOREGROUND_PACKAGE}"
                ),
                format!("-s s shell am start --user 0 -n {FOREGROUND_PACKAGE}/.MainActivity"),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn restart_relaunch_scopes_to_user_10() {
        let log = test_util::log_path("restart-user10");
        let client = AdbClient::new(test_util::fake_adb_for_user(
            "fake-adb-restart-user10",
            &log,
            10,
        ));
        RestartApp
            .perform(&client, "s", FeatureInput::None)
            .unwrap();
        let logged = test_util::logged(&log);
        assert!(logged.iter().any(
            |line| line == &format!("-s s shell am force-stop --user 10 {FOREGROUND_PACKAGE}")
        ));
        assert!(
            logged
                .iter()
                .any(|line| { line.contains("cmd package resolve-activity --user 10") })
        );
        assert!(logged.iter().any(|line| {
            line == &format!("-s s shell am start --user 10 -n {FOREGROUND_PACKAGE}/.MainActivity")
        }));
        test_util::clean_up(&log);
    }

    #[test]
    fn restart_resolve_failure_never_attempts_start() {
        let log = test_util::log_path("restart-fail");
        let client = AdbClient::new(fake_adb_no_launcher("fake-adb-restart-fail", &log));
        let result = RestartApp.perform(&client, "s", FeatureInput::None);
        assert!(result.is_err());
        let logged = test_util::logged(&log);
        assert!(logged.iter().any(|line| line.contains("resolve-activity")));
        assert!(!logged.iter().any(|line| line.contains("am start")));
        test_util::clean_up(&log);
    }

    #[test]
    fn resolve_component_takes_the_last_nonempty_line() {
        assert_eq!(
            resolve_component("com.example.app/.MainActivity\n"),
            Some("com.example.app/.MainActivity".to_string())
        );
        assert_eq!(
            resolve_component("noise\ncom.example.app/.MainActivity\n\n"),
            Some("com.example.app/.MainActivity".to_string())
        );
    }

    #[test]
    fn resolve_component_rejects_unparseable_output() {
        assert_eq!(resolve_component(""), None);
        assert_eq!(resolve_component("No activity found\n"), None);
        // A line with interior whitespace is not a bare component.
        assert_eq!(
            resolve_component("priority=0 com.example.app/.Main\n"),
            None
        );
    }
}

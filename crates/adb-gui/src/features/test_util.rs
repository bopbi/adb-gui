//! Test-only fixtures shared by feature argv tests: a fake adb that answers
//! `dumpsys activity activities` with a foreground-app fixture and logs
//! every argv it receives.

#![cfg(test)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The package the fixture reports as foreground.
pub const FOREGROUND_PACKAGE: &str = "com.example.app";

/// Fake adb: `dumpsys activity activities` answers with a topResumedActivity
/// record for [`FOREGROUND_PACKAGE`]; `cmd package resolve-activity` answers
/// with that package's launcher component; every argv is appended to `log`.
/// `name` must be unique per test (parallel tests share the fixture dir).
pub fn fake_adb(name: &str, log: &Path) -> PathBuf {
    fake_adb_for_user(name, log, 0)
}

/// Like [`fake_adb`], but the foreground record belongs to Android user
/// `user` (`u<user>`), as for a work-profile app.
pub fn fake_adb_for_user(name: &str, log: &Path, user: u32) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("adb-gui-app-actions-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{log}\"\n\
             if [ \"$4\" = \"dumpsys\" ]; then\n\
             \x20 printf '  topResumedActivity=ActivityRecord{{a1b2c3 u{user} {pkg}/.MainActivity t7}}\\n'\n\
             fi\n\
             if [ \"$4\" = \"cmd\" ]; then\n\
             \x20 printf '{pkg}/.MainActivity\\n'\n\
             fi\n\
             exit 0\n",
            log = log.display(),
            pkg = FOREGROUND_PACKAGE,
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

/// The argv lines the fake adb logged.
pub fn logged(log: &Path) -> Vec<String> {
    fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// A fresh log path unique to the calling test.
pub fn log_path(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("adb-gui-{tag}-log-{}", std::process::id()));
    let _ = fs::remove_file(&path);
    path
}

/// Remove the log file at the end of a test.
pub fn clean_up(log: &Path) {
    let _ = fs::remove_file(log);
}

/// Fake adb with an emulator console: logs every argv; answers
/// `emu power display` with a status block (capacity 50) and
/// `settings get global wifi_on` / `settings get global mobile_data` with 1.
/// `name` must be unique per test.
pub fn fake_adb_emu(name: &str, log: &Path) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("adb-gui-emu-actions-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{log}\"\n\
             case \"$*\" in\n\
             *'emu power display') printf 'OK\\nAC: online\\nstatus: Charging\\nhealth: Good\\npresent: true\\ncapacity: 50\\n' ;;
             *'emu power capacity') printf 'KO: missing argument\\n' ;;\n\
             *'settings get global wifi_on') printf '1\\n' ;;\n\
             *'settings get global mobile_data') printf '1\\n' ;;\n\
             esac\n\
             exit 0\n",
            log = log.display(),
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

/// Fake adb whose every command logs its argv, prints a stderr line, and
/// exits non-zero — for failure-path tests. `name` must be unique per test.
pub fn fake_adb_failing(name: &str, log: &Path) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("adb-gui-fail-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{log}\"\n\
             echo 'Exception occurred while executing' >&2\n\
             exit 1\n",
            log = log.display(),
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

/// Isolation lock for tests that set the capture-dir override: serializes
/// the override between tests (they run in parallel threads). Ordering only
/// — the override replaces the unsafe env mutation, so no soundness contract
/// depends on this lock.
pub static CAPTURE_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Point capture output at a scratch dir unique to `tag` (holding
/// [`CAPTURE_DIR_LOCK`] first) so capture features write under it instead of
/// the developer's real Pictures folder. Returns the scratch dir; captures
/// land in its `Pictures/adb-gui` subdir (created by the capture itself, so
/// a test can still plant a blocker there).
pub fn use_scratch_capture_dir_locked(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("adb-gui-fake-home-{tag}-{}", std::process::id()));
    fs::create_dir_all(&home).unwrap();
    crate::features::pictures::set_capture_dir_override(Some(
        home.join("Pictures").join("adb-gui"),
    ));
    home
}

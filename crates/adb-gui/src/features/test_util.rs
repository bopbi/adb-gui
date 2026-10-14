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
/// record for [`FOREGROUND_PACKAGE`]; every argv is appended to `log`.
/// `name` must be unique per test (parallel tests share the fixture dir).
pub fn fake_adb(name: &str, log: &Path) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("adb-gui-app-actions-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{log}\"\n\
             if [ \"$4\" = \"dumpsys\" ]; then\n\
             \x20 printf '  topResumedActivity=ActivityRecord{{a1b2c3 u0 {pkg}/.MainActivity t7}}\\n'\n\
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

/// Env-mutation lock: tests that redirect HOME take this guard so they
/// cannot interfere with each other (tests run in parallel threads).
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Point HOME at a scratch dir unique to `tag` (holding [`ENV_LOCK`] first)
/// so capture features write under it instead of the developer's real
/// Pictures folder. Returns the scratch dir.
pub fn use_scratch_home_locked(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("adb-gui-fake-home-{tag}-{}", std::process::id()));
    fs::create_dir_all(&home).unwrap();
    // Safety: the ENV_LOCK guard serializes HOME mutation between tests;
    // every test sets its own per-tag value before reading it.
    unsafe { std::env::set_var("HOME", &home) };
    home
}

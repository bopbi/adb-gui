use std::path::PathBuf;

use crate::error::{AdbError, Result};

const ADB_BINARY: &str = if cfg!(windows) { "adb.exe" } else { "adb" };

/// Locate the `adb` binary.
///
/// Discovery order: `ANDROID_HOME`, then `ANDROID_SDK_ROOT`, then
/// `platform-tools/adb` reachable from PATH, then platform defaults (macOS
/// `~/Library/Android/sdk`, Linux `~/Android/Sdk`, Windows
/// `%LOCALAPPDATA%\Android\Sdk`).
pub fn find_adb() -> Result<PathBuf> {
    resolve(&|key| std::env::var(key).ok()).ok_or(AdbError::SdkNotFound)
}

/// Discovery logic with an injectable environment lookup, so tests can
/// point it at fixture directories instead of the real SDK. Each source
/// yields a full adb path; the first one that exists on disk wins.
pub(crate) fn resolve(lookup: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let candidates = [
        lookup("ANDROID_HOME").map(|root| sdk_adb(&root)),
        lookup("ANDROID_SDK_ROOT").map(|root| sdk_adb(&root)),
        find_on_path(lookup("PATH").as_deref()),
        platform_default(&lookup).map(|root| sdk_adb(&root.to_string_lossy())),
    ];

    candidates.into_iter().flatten().find(|adb| adb.is_file())
}

fn sdk_adb(root: &str) -> PathBuf {
    PathBuf::from(root).join("platform-tools").join(ADB_BINARY)
}

/// Probe PATH for either a bare `adb` binary or a `platform-tools`
/// directory containing it; returns the full adb path.
fn find_on_path(path_var: Option<&str>) -> Option<PathBuf> {
    let path_var = path_var?;
    for dir in std::env::split_paths(std::ffi::OsStr::new(path_var)) {
        let bare = dir.join(ADB_BINARY);
        if bare.is_file() {
            return Some(bare);
        }
        let nested = dir.join("platform-tools").join(ADB_BINARY);
        if nested.is_file() {
            return Some(nested);
        }
    }
    None
}

fn platform_default(lookup: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if cfg!(windows) {
        let local_app_data = lookup("LOCALAPPDATA")?;
        Some(PathBuf::from(local_app_data).join("Android").join("Sdk"))
    } else if cfg!(target_os = "macos") {
        lookup("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Android")
                .join("sdk")
        })
    } else {
        lookup("HOME").map(|home| PathBuf::from(home).join("Android").join("Sdk"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    /// Unique fixture root per test; removed on drop.
    struct FixtureDir(PathBuf);

    impl FixtureDir {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "adb-gui-sdk-test-{}-{}",
                name,
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        /// Create `<root>/platform-tools/adb` and return the adb path.
        fn with_sdk_layout(&self) -> PathBuf {
            let adb = sdk_adb(&self.0.to_string_lossy());
            fs::create_dir_all(adb.parent().unwrap()).unwrap();
            fs::write(&adb, b"").unwrap();
            adb
        }
    }

    impl Drop for FixtureDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn lookup_from(map: HashMap<&str, String>) -> impl Fn(&str) -> Option<String> {
        move |key| map.get(key).cloned()
    }

    #[test]
    fn android_home_wins_over_sdk_root() {
        let home = FixtureDir::new("home-wins");
        let expected = home.with_sdk_layout();
        let decoy = FixtureDir::new("home-wins-decoy");
        decoy.with_sdk_layout();

        let map = HashMap::from([
            ("ANDROID_HOME", home.0.to_string_lossy().into_owned()),
            ("ANDROID_SDK_ROOT", decoy.0.to_string_lossy().into_owned()),
        ]);
        assert_eq!(resolve(&lookup_from(map)), Some(expected));
    }

    #[test]
    fn sdk_root_used_when_home_missing() {
        let fixture = FixtureDir::new("sdk-root");
        let expected = fixture.with_sdk_layout();

        let map = HashMap::from([("ANDROID_SDK_ROOT", fixture.0.to_string_lossy().into_owned())]);
        assert_eq!(resolve(&lookup_from(map)), Some(expected));
    }

    #[test]
    fn path_entry_with_platform_tools_dir_is_used() {
        let fixture = FixtureDir::new("on-path");
        let expected = fixture.with_sdk_layout();

        let map = HashMap::from([("PATH", fixture.0.to_string_lossy().into_owned())]);
        assert_eq!(resolve(&lookup_from(map)), Some(expected));
    }

    #[test]
    fn bare_adb_on_path_is_used() {
        let fixture = FixtureDir::new("bare-on-path");
        let expected = fixture.0.join(ADB_BINARY);
        fs::write(&expected, b"").unwrap();

        let map = HashMap::from([("PATH", fixture.0.to_string_lossy().into_owned())]);
        assert_eq!(resolve(&lookup_from(map)), Some(expected));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_home_default_is_used() {
        let fixture = FixtureDir::new("home-default");
        let home = fixture.0.join("Library").join("Android").join("sdk");
        let expected = sdk_adb(&home.to_string_lossy());
        fs::create_dir_all(expected.parent().unwrap()).unwrap();
        fs::write(&expected, b"").unwrap();

        let map = HashMap::from([("HOME", fixture.0.to_string_lossy().into_owned())]);
        assert_eq!(resolve(&lookup_from(map)), Some(expected));
    }

    #[test]
    fn no_source_returns_none() {
        assert_eq!(resolve(&|_| None), None);
    }

    #[test]
    fn broken_android_home_falls_through_to_sdk_root() {
        let broken = FixtureDir::new("broken-home"); // no platform-tools created
        let good = FixtureDir::new("fallback-root");
        let expected = good.with_sdk_layout();

        let map = HashMap::from([
            ("ANDROID_HOME", broken.0.to_string_lossy().into_owned()),
            ("ANDROID_SDK_ROOT", good.0.to_string_lossy().into_owned()),
        ]);
        assert_eq!(resolve(&lookup_from(map)), Some(expected));
    }
}

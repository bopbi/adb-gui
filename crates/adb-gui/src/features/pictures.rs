//! Shared output paths for capture features: the user's Pictures dir with a
//! system-temp fallback. Only capture features write files; this module is
//! where their output paths live.

use std::path::PathBuf;

use adb_core::AdbError;

/// `~/Pictures/adb-gui/` (macOS/Linux) or `%USERPROFILE%\Pictures\adb-gui\`
/// (Windows).
pub fn pictures_dir() -> PathBuf {
    let home = if cfg!(windows) {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    };
    home.unwrap_or_else(std::env::temp_dir)
        .join("Pictures")
        .join("adb-gui")
}

/// Timestamped filename, e.g. `screenshot-20261013-153045.png`. The
/// timestamp is UTC (the civil-date math below has no timezone database) and
/// has second precision, so colliding names are made unique at save time.
pub fn timestamped_filename(prefix: &str, extension: &str) -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day, hour, minute, second) = epoch_to_ymdhms(seconds);
    format!("{prefix}-{year}{month:02}{day:02}-{hour:02}{minute:02}{second:02}.{extension}")
}

/// `dir`/`filename` when absent, else `dir`/`stem-2.ext`, `stem-3.ext`, … so
/// a later capture never overwrites an earlier one.
pub fn unique_path(dir: &std::path::Path, filename: &str) -> PathBuf {
    let path = dir.join(filename);
    // A stat error means "cannot tell"; assume the name is free and let the
    // atomic create_new open be the real guard (a false assumption just
    // fails the open, which callers treat as "try elsewhere").
    if !path.try_exists().unwrap_or(false) {
        return path;
    }
    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str());
    for n in 2u32.. {
        let candidate = match extension {
            Some(ext) => dir.join(format!("{stem}-{n}.{ext}")),
            None => dir.join(format!("{stem}-{n}")),
        };
        if !candidate.try_exists().unwrap_or(false) {
            return candidate;
        }
    }
    unreachable!("u32 counter exhausted")
}

/// Write `bytes` to `dir`/`filename`, falling back to the system temp dir
/// when the Pictures dir is not writable. Returns the path the bytes landed
/// at and whether the fallback was used (the caller surfaces the path when
/// it was). A name that already exists gets a fresh `-N` suffix instead of
/// truncating the earlier file.
pub fn save_with_fallback(
    dir: &std::path::Path,
    filename: &str,
    bytes: &[u8],
) -> Result<(PathBuf, bool), AdbError> {
    fn write(dir: &std::path::Path, filename: &str, bytes: &[u8]) -> Result<PathBuf, AdbError> {
        use std::io::Write as _;

        std::fs::create_dir_all(dir)?;
        let path = unique_path(dir, filename);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(bytes)?;
        Ok(path)
    }

    match write(dir, filename, bytes) {
        Ok(path) => Ok((path, false)),
        Err(_) => write(&std::env::temp_dir(), filename, bytes).map(|path| (path, true)),
    }
}

// Our own dotfile, created and removed inside a candidate dir to test
// writability. `create` (not `create_new`) + `truncate`: a probe left behind
// by a killed process must not poison later probes.
const PROBE_NAME: &str = ".adb-gui-write-probe-";

fn probe_path(dir: &std::path::Path) -> PathBuf {
    dir.join(format!("{PROBE_NAME}{}", std::process::id()))
}

/// Resolve the path a downloaded file should land at: `dir` when creatable
/// and writable, else the system temp dir. The caller performs the transfer
/// itself (e.g. `adb pull`); this only picks the writable destination. The
/// bool reports whether the fallback was used. A name that already exists
/// gets a fresh `-N` suffix so the pulled file never overwrites an earlier
/// one.
pub fn save_target(dir: &std::path::Path, filename: &str) -> (PathBuf, bool) {
    fn writable(dir: &std::path::Path) -> bool {
        std::fs::create_dir_all(dir).is_ok()
            && std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(probe_path(dir))
                .and_then(|_| std::fs::remove_file(probe_path(dir)))
                .is_ok()
    }

    if writable(dir) {
        (unique_path(dir, filename), false)
    } else {
        (unique_path(&std::env::temp_dir(), filename), true)
    }
}

/// Minimal civil-from-epoch for filename timestamps (no chrono dependency;
/// proleptic-Gregorian arithmetic, accurate for 1970..2099).
fn epoch_to_ymdhms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = secs / 86_400;
    let secs_in_day = secs % 86_400;
    let hour = secs_in_day / 3_600;
    let minute = (secs_in_day % 3_600) / 60;
    let second = secs_in_day % 60;
    // 1970-01-01 is Thursday; leap-year count from epoch.
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let year_len = if is_leap(year) { 366 } else { 365 };
        if remaining < year_len {
            break;
        }
        remaining -= year_len;
        year += 1;
    }
    const MONTH_DAYS: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    let mut day = remaining;
    loop {
        let len = MONTH_DAYS[month] + u64::from(month == 1 && is_leap(year));
        if day < len {
            break;
        }
        day -= len;
        month += 1;
    }
    (year, month as u64 + 1, day + 1, hour, minute, second)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamped_filename_has_expected_shape() {
        let name = timestamped_filename("screenshot", "png");
        let pattern = regexless(&name);
        assert!(
            pattern,
            "expected screenshot-YYYYMMDD-HHMMSS.png, got {name}"
        );
    }

    fn regexless(name: &str) -> bool {
        let Some(rest) = name.strip_prefix("screenshot-") else {
            return false;
        };
        let (date, time_ext) = match rest.split_once('-') {
            Some(pair) => pair,
            None => return false,
        };
        let (time, ext) = match time_ext.split_once('.') {
            Some(pair) => pair,
            None => return false,
        };
        date.len() == 8
            && date.chars().all(|c| c.is_ascii_digit())
            && time.len() == 6
            && time.chars().all(|c| c.is_ascii_digit())
            && ext == "png"
    }

    #[test]
    fn epoch_to_ymdhms_matches_known_dates() {
        // 1970-01-01 00:00:00.
        assert_eq!(epoch_to_ymdhms(0), (1970, 1, 1, 0, 0, 0));
        // 2000-02-29 00:00:00 UTC (leap day, Y2K).
        let y2k_leap = 951_782_400;
        assert_eq!(epoch_to_ymdhms(y2k_leap), (2000, 2, 29, 0, 0, 0));
    }

    #[test]
    fn save_with_fallback_writes_to_dir() {
        let dir =
            std::env::temp_dir().join(format!("adb-gui-pictures-test-{}", std::process::id()));
        let (path, fell_back) = save_with_fallback(&dir, "probe.txt", b"hello").unwrap();
        assert!(!fell_back);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_with_fallback_uses_temp_when_dir_unwritable() {
        // A path that cannot be created (file in the way of a directory).
        let blocker =
            std::env::temp_dir().join(format!("adb-gui-pictures-blocker-{}", std::process::id()));
        std::fs::write(&blocker, b"not a dir").unwrap();
        let (path, fell_back) =
            save_with_fallback(&blocker.join("sub"), "probe.txt", b"hi").unwrap();
        assert!(fell_back);
        assert_eq!(path.parent().unwrap(), std::env::temp_dir());
        assert_eq!(std::fs::read(&path).unwrap(), b"hi");
        let _ = std::fs::remove_file(&blocker);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_target_falls_back_with_flag() {
        let dir =
            std::env::temp_dir().join(format!("adb-gui-pictures-target-{}", std::process::id()));
        let (path, fell_back) = save_target(&dir, "rec.mp4");
        assert!(!fell_back);
        assert_eq!(path, dir.join("rec.mp4"));
        let _ = std::fs::remove_dir_all(&dir);

        let blocker = std::env::temp_dir().join(format!(
            "adb-gui-pictures-target-blocker-{}",
            std::process::id()
        ));
        std::fs::write(&blocker, b"not a dir").unwrap();
        let (path, fell_back) = save_target(&blocker, "rec.mp4");
        assert!(fell_back);
        assert_eq!(path, std::env::temp_dir().join("rec.mp4"));
        let _ = std::fs::remove_file(&blocker);
    }

    #[test]
    fn save_with_fallback_makes_colliding_names_unique() {
        let dir =
            std::env::temp_dir().join(format!("adb-gui-pictures-unique-{}", std::process::id()));
        let (first, fell_back) = save_with_fallback(&dir, "probe.txt", b"one").unwrap();
        assert!(!fell_back);
        let (second, fell_back) = save_with_fallback(&dir, "probe.txt", b"two").unwrap();
        assert!(!fell_back);
        assert_ne!(first, second);
        assert_eq!(first, dir.join("probe.txt"));
        assert_eq!(second, dir.join("probe-2.txt"));
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_target_appends_suffix_when_name_exists() {
        let dir = std::env::temp_dir().join(format!(
            "adb-gui-pictures-target-unique-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("rec.mp4"), b"earlier").unwrap();
        let (path, fell_back) = save_target(&dir, "rec.mp4");
        assert!(!fell_back);
        assert_eq!(path, dir.join("rec-2.mp4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn save_target_falls_back_for_read_only_dir() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir =
            std::env::temp_dir().join(format!("adb-gui-pictures-readonly-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let (path, fell_back) = save_target(&dir, "rec.mp4");
        assert!(fell_back);
        assert_eq!(path.parent().unwrap(), std::env::temp_dir());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn save_with_fallback_falls_back_promptly_for_stat_error_dir() {
        use std::os::unix::fs::PermissionsExt as _;

        // No search permission: stat fails inside the dir. unique_path must
        // treat that as "name free" and let the open fail, not spin through
        // suffixes into unreachable!.
        let dir =
            std::env::temp_dir().join(format!("adb-gui-pictures-staterr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        let (path, fell_back) = save_with_fallback(&dir, "probe.txt", b"hi").unwrap();
        assert!(fell_back);
        assert_eq!(path.parent().unwrap(), std::env::temp_dir());
        assert_eq!(std::fs::read(&path).unwrap(), b"hi");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_target_ignores_a_stale_write_probe() {
        let dir =
            std::env::temp_dir().join(format!("adb-gui-pictures-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A probe left behind by a killed process must not make the dir look
        // unwritable.
        std::fs::write(probe_path(&dir), b"stale").unwrap();
        let (path, fell_back) = save_target(&dir, "rec.mp4");
        assert!(!fell_back);
        assert_eq!(path, dir.join("rec.mp4"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

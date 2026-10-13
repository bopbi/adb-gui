//! Batched on-device state reading: one `adb shell` round trip runs every
//! `settings get` / `wm density` query the catalog needs, and each feature
//! maps its slice of the result to a [`FeatureState`] via its own `apply`.
//!
//! The query table is fixed and ORDERED: output lines are zipped to queries
//! by position, so a feature always reads its key, never a line number.

use std::collections::HashMap;

use adb_core::{AdbClient, AdbError};

/// One batched query: the key features read the answer under, and the exact
/// command run inside the device shell.
pub struct Query {
    pub key: &'static str,
    pub command: &'static str,
}

pub const UI_NIGHT_MODE: &str = "ui_night_mode";
pub const FONT_SCALE: &str = "font_scale";
pub const WM_DENSITY: &str = "wm_density";
pub const ACCESSIBILITY_SERVICES: &str = "accessibility_services";
pub const WINDOW_ANIMATION_SCALE: &str = "window_animation_scale";
pub const TRANSITION_ANIMATION_SCALE: &str = "transition_animation_scale";
pub const ANIMATOR_DURATION_SCALE: &str = "animator_duration_scale";
pub const SYSTEM_LOCALES: &str = "system_locales";
pub const DEBUG_LAYOUT: &str = "debug_layout";

/// The fixed, ordered query table — one entry per readable value.
pub const QUERIES: &[Query] = &[
    Query {
        key: UI_NIGHT_MODE,
        command: "settings get secure ui_night_mode",
    },
    Query {
        key: FONT_SCALE,
        command: "settings get system font_scale",
    },
    Query {
        key: WM_DENSITY,
        // `| tail -1`: plain `wm density` prints TWO lines when an override is
        // set ("Physical density: 440" first, "Override density: 400" second),
        // which would shift every later query's value by one in the
        // zip-by-order parse — and the override is the effective density
        // anyway, so it is the line we want. Without an override the physical
        // line is the last (only) line.
        command: "wm density | tail -1",
    },
    Query {
        key: ACCESSIBILITY_SERVICES,
        command: "settings get secure enabled_accessibility_services",
    },
    Query {
        key: WINDOW_ANIMATION_SCALE,
        command: "settings get global window_animation_scale",
    },
    Query {
        key: TRANSITION_ANIMATION_SCALE,
        command: "settings get global transition_animation_scale",
    },
    Query {
        key: ANIMATOR_DURATION_SCALE,
        command: "settings get global animator_duration_scale",
    },
    Query {
        key: SYSTEM_LOCALES,
        command: "settings get system system_locales",
    },
    Query {
        key: DEBUG_LAYOUT,
        command: "getprop debug.layout",
    },
];

/// The answers of one batched read, keyed by query key. Missing entries read
/// as "" so features degrade to off/none instead of failing.
pub struct BatchedState {
    values: HashMap<&'static str, String>,
}

impl BatchedState {
    pub fn get(&self, key: &str) -> &str {
        self.values.get(key).map(String::as_str).unwrap_or("")
    }

    /// The raw value, or None when unset/unknown ("", "null") — the common
    /// "no readable value" case for stepper/choice display.
    pub fn value(&self, key: &str) -> Option<&str> {
        let value = self.get(key).trim();
        (!value.is_empty() && value != "null").then_some(value)
    }
}

/// Run every query in ONE `adb shell` call and zip the output lines to the
/// query table by order. Short output is tolerated: missing queries map to "".
pub fn read(client: &AdbClient, serial: &str) -> Result<BatchedState, AdbError> {
    let joined = QUERIES
        .iter()
        .map(|query| query.command)
        .collect::<Vec<_>>()
        .join("; ");
    let output = client.shell(serial, &[&joined])?;
    Ok(parse(&output))
}

/// Zip query-ordered output lines into a [`BatchedState`]. Pure; unit-tested.
pub fn parse(output: &str) -> BatchedState {
    let lines: Vec<&str> = output.lines().collect();
    let values = QUERIES
        .iter()
        .enumerate()
        .map(|(ix, query)| {
            let line = lines.get(ix).copied().unwrap_or("").trim().to_string();
            (query.key, line)
        })
        .collect();
    BatchedState { values }
}

/// Shared `settings put` pattern for features that write a single setting.
pub fn put_setting(
    client: &AdbClient,
    serial: &str,
    table: &str,
    key: &str,
    value: &str,
) -> Result<(), AdbError> {
    client.shell(serial, &["settings", "put", table, key, value])?;
    Ok(())
}

/// Parse `wm density` output. AOSP prints "Physical density: X" first and,
/// when an override is set, "Override density: Y" second — the override is
/// the effective density and wins. Tolerates anything unknown → None.
pub fn parse_density(output: &str) -> Option<u32> {
    let mut physical = None;
    for line in output.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("density") {
            continue;
        }
        let Some(value) = line
            .split(':')
            .nth(1)
            .map(str::trim)
            .and_then(|raw| raw.parse::<u32>().ok())
        else {
            continue;
        };
        if lower.contains("override") {
            return Some(value);
        }
        physical = physical.or(Some(value));
    }
    physical
}

/// Parse a `settings get … *_scale` value ("0.5", "1.0", "null") → f32.
pub fn parse_scale(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_zips_lines_to_queries_by_order() {
        // One line per query, in table order.
        let output = "2\n1.3\nPhysical density: 440\n\
                      com.google.android.marvin.talkback/com.google.android.marvin.talkback.TalkBackService\n\
                      0.0\n0.0\n0.0\n[en-US]\ntrue\n";
        let state = parse(output);
        assert_eq!(state.get(UI_NIGHT_MODE), "2");
        assert_eq!(state.get(FONT_SCALE), "1.3");
        assert_eq!(state.get(WM_DENSITY), "Physical density: 440");
        assert_eq!(state.value(FONT_SCALE), Some("1.3"));
        assert_eq!(state.value(SYSTEM_LOCALES), Some("[en-US]"));
        assert_eq!(state.get(DEBUG_LAYOUT), "true");
    }

    #[test]
    fn parse_tolerates_short_output_and_nulls() {
        // Device answered only the first two queries, then a null line.
        let state = parse("1\nnull\n");
        assert_eq!(state.get(UI_NIGHT_MODE), "1");
        assert_eq!(state.value(FONT_SCALE), None, "'null' means unset");
        assert_eq!(state.get(WM_DENSITY), "", "missing queries map to empty");
        assert_eq!(state.get(ACCESSIBILITY_SERVICES), "");
    }

    #[test]
    fn parse_empty_output_yields_empty_state() {
        let state = parse("");
        assert_eq!(state.get(UI_NIGHT_MODE), "");
        assert_eq!(state.value(UI_NIGHT_MODE), None);
    }

    #[test]
    fn parse_density_prefers_override_over_physical() {
        // AOSP prints Physical first, Override second; the override is the
        // effective density and must win (H1).
        assert_eq!(
            parse_density("Physical density: 440\nOverride density: 400\n"),
            Some(400)
        );
        assert_eq!(parse_density("Override density: 400"), Some(400));
        // No override: the physical line is the answer.
        assert_eq!(parse_density("Physical density: 440"), Some(440));
        assert_eq!(parse_density("garbage"), None);
        assert_eq!(parse_density(""), None);
    }

    #[test]
    fn parse_scale_tolerates_unknown_values() {
        assert_eq!(parse_scale("0.5"), Some(0.5));
        assert_eq!(parse_scale("1.0"), Some(1.0));
        assert_eq!(parse_scale("null"), None);
        assert_eq!(parse_scale(""), None);
    }

    // G1/H1 regression: with a density override set, plain `wm density` prints
    // two lines and shifted every later query by one. The query table pipes
    // through `tail -1`, so the device shell emits the effective (Override)
    // line when one exists and the Physical line otherwise — always
    // single-line, so the zip-by-order parse stays aligned. This fixture
    // emulates the device shell's `tail -1` semantics and logs the batch it
    // received so the fix itself is pinned.
    #[cfg(unix)]
    #[test]
    fn batched_read_stays_aligned_when_density_override_exists() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("adb-gui-state-reader-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("argv.log");
        let adb = dir.join("fake-adb-state-reader");
        fs::write(
            &adb,
            "#!/bin/sh\n\
             printf '%s\\n' \"$*\" >> \"{log}\"\n\
             printf '1\\n1.0\\n'\n\
             case \"$*\" in\n\
             *'wm density'*) printf 'Override density: 400\\n' ;;\n\
             esac\n\
             printf 'svc:talkback\\n0.0\\n0.0\\n0.0\\n[en-US]\\ntrue\\n'\n"
                .replace("{log}", &log.to_string_lossy()),
        )
        .unwrap();
        let mut perms = fs::metadata(&adb).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&adb, perms).unwrap();

        let client = AdbClient::new(adb);
        let state = read(&client, "serial123").expect("batched read");

        // The fix: the batch itself must reduce wm density to its last line.
        let argv = fs::read_to_string(&log).unwrap();
        assert!(
            argv.contains("wm density | tail -1"),
            "batch must pipe wm density through tail -1, got: {argv}"
        );
        // The density answer is the effective (override) line; the service
        // list must land on its own key, not be shifted by a second line.
        assert_eq!(parse_density(state.get(WM_DENSITY)), Some(400));
        assert_eq!(state.get(ACCESSIBILITY_SERVICES), "svc:talkback");
        assert_eq!(state.value(ANIMATOR_DURATION_SCALE), Some("0.0"));
        assert_eq!(state.get(SYSTEM_LOCALES), "[en-US]");
        assert_eq!(state.get(DEBUG_LAYOUT), "true");
    }
}

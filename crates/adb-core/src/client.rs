use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, ExitStatus, Output, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::device::{Device, parse_devices};
use crate::error::{AdbError, Result};

const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The single owner of the adb process boundary. Construct one from a path
/// obtained via [`crate::find_adb`]; every command it runs goes through the
/// same spawn-and-watchdog [`AdbClient::run`].
#[derive(Debug, Clone)]
pub struct AdbClient {
    pub adb_path: PathBuf,
}

impl AdbClient {
    pub fn new(adb_path: PathBuf) -> Self {
        Self { adb_path }
    }

    /// `adb devices -l`, parsed. Skips malformed rows.
    pub fn devices(&self) -> Result<Vec<Device>> {
        let output = self.run(&["devices", "-l"])?;
        Ok(parse_devices(&String::from_utf8_lossy(&output.stdout)))
    }

    /// `adb -s <serial> shell <args...>`; returns captured stdout as text.
    pub fn shell(&self, serial: &str, args: &[&str]) -> Result<String> {
        let argv = [&["-s", serial, "shell"], args].concat();
        let output = self.run(&argv)?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// `adb -s <serial> exec-out <args...>`; binary-safe (PNG bytes etc.).
    /// Never lossy-converted to UTF-8.
    pub fn exec_out(&self, serial: &str, args: &[&str]) -> Result<Vec<u8>> {
        let argv = [&["-s", serial, "exec-out"], args].concat();
        Ok(self.run(&argv)?.stdout)
    }

    /// `adb -s <serial> emu <args...>` — emulator console commands
    /// (`emu kill`, `geo fix ...`, `sms send ...`). Only meaningful for
    /// emulators; the caller checks [`Device::is_emulator`].
    pub fn emu(&self, serial: &str, args: &[&str]) -> Result<String> {
        let argv = [&["-s", serial, "emu"], args].concat();
        let output = self.run(&argv)?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Spawn `adb_path + args`, watch with a timeout, capture both streams.
    ///
    /// stdout/stderr are drained on dedicated threads so a chatty child can
    /// never deadlock on a full pipe buffer. Non-zero exit maps to
    /// [`AdbError::CommandFailed`].
    fn run(&self, args: &[&str]) -> Result<Output> {
        let command = args.join(" ");
        let mut child: Child = std::process::Command::new(&self.adb_path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = spawn_drain(child.stdout.take().expect("piped stdout"));
        let stderr = spawn_drain(child.stderr.take().expect("piped stderr"));

        let status = wait_with_watchdog(&mut child, &command)?;

        let stdout = stdout.join().expect("stdout drain panicked")?;
        let stderr = stderr.join().expect("stderr drain panicked")?;

        if let Some(code) = status.code()
            && code != 0
        {
            return Err(AdbError::CommandFailed {
                command,
                stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
            });
        }

        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }
}

/// Read a pipe to end on its own thread; returns the raw bytes.
fn spawn_drain<R: Read + Send + 'static>(mut reader: R) -> JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        Ok(buf)
    })
}

/// Poll the child until it exits or the watchdog fires; kill on timeout.
fn wait_with_watchdog(child: &mut Child, command: &str) -> Result<ExitStatus> {
    let deadline = Instant::now() + WATCHDOG_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AdbError::Timeout {
                command: command.to_string(),
            });
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// Write an executable shell script and return its path.
    fn script(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("adb-gui-client-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn devices_runs_devices_l_and_parses() {
        let adb = script(
            "fake-adb-devices",
            r#"if [ "$1" = "devices" ]; then
  printf 'List of devices attached\nemulator-5554  device model:Fake\n'
else
  echo "unexpected: $*" >&2; exit 1
fi"#,
        );
        let client = AdbClient::new(adb);
        let devices = client.devices().unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].serial, "emulator-5554");
        assert_eq!(devices[0].model.as_deref(), Some("Fake"));
    }

    #[test]
    fn shell_forwards_serial_and_args() {
        let adb = script(
            "fake-adb-shell",
            r#"printf '%s\n' "$@" >> "$ADB_GUI_ARGS_LOG"
printf 'hello stdout'"#,
        );
        let log = std::env::temp_dir().join(format!("adb-gui-args-{}", std::process::id()));
        // Safety: the var is read only by the fake adb spawned by this test.
        unsafe { std::env::set_var("ADB_GUI_ARGS_LOG", &log) };
        let client = AdbClient::new(adb);
        let out = client.shell("serial123", &["echo", "ok"]).unwrap();
        assert_eq!(out, "hello stdout");
        let logged = fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = logged.lines().collect();
        assert_eq!(
            lines,
            ["-s", "serial123", "shell", "echo", "ok"],
            "adb argv must be forwarded exactly"
        );
        unsafe { std::env::remove_var("ADB_GUI_ARGS_LOG") };
    }

    #[test]
    fn emu_forwards_args() {
        let adb = script(
            "fake-adb-emu",
            r#"if [ "$3" = "emu" ] && [ "$4" = "kill" ]; then
  printf 'emulator killed'
else
  echo "bad argv: $*" >&2; exit 1
fi"#,
        );
        let client = AdbClient::new(adb);
        let out = client.emu("emulator-5554", &["kill"]).unwrap();
        assert_eq!(out, "emulator killed");
    }

    #[test]
    fn exec_out_preserves_non_utf8_bytes() {
        let adb = script(
            "fake-adb-exec-out",
            r#"if [ "$3" = "exec-out" ]; then
  printf '\x89PNG\r\n\x1a\n\x00\xff\x80binary'
else
  echo "bad argv" >&2; exit 1
fi"#,
        );
        let client = AdbClient::new(adb);
        let bytes = client.exec_out("serial", &["screencap", "-p"]).unwrap();
        assert_eq!(
            bytes, b"\x89PNG\r\n\x1a\n\x00\xff\x80binary",
            "exec_out must stay binary-safe"
        );
    }

    #[test]
    fn nonzero_exit_maps_to_command_failed_with_stderr() {
        let adb = script("fake-adb-fail", r#"echo 'some error detail' >&2; exit 1"#);
        let client = AdbClient::new(adb);
        let err = client.shell("s", &["true"]).unwrap_err();
        match err {
            AdbError::CommandFailed { command, stderr } => {
                assert_eq!(command, "-s s shell true");
                assert!(stderr.contains("some error detail"), "got: {stderr}");
            }
            other => panic!("expected CommandFailed, got {other:?}"),
        }
    }

    #[test]
    fn slow_command_maps_to_timeout() {
        let adb = script("fake-adb-slow", "sleep 60");
        let client = AdbClient::new(adb);
        let start = Instant::now();
        let err = client.shell("s", &["sleep"]).unwrap_err();
        assert!(matches!(err, AdbError::Timeout { .. }));
        assert!(
            start.elapsed() < Duration::from_secs(35),
            "watchdog should fire near 30s, took {:?}",
            start.elapsed()
        );
    }
}

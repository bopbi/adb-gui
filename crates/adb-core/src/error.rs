use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum AdbError {
    #[error("Android SDK not found; set ANDROID_HOME or ANDROID_SDK_ROOT, or put adb on PATH")]
    SdkNotFound,

    #[error("device offline: {serial}")]
    DeviceOffline { serial: String },

    #[error("adb command timed out after {timeout:?}: {command}")]
    Timeout { command: String, timeout: Duration },

    #[error("adb command failed: {command}: {stderr}")]
    CommandFailed { command: String, stderr: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AdbError>;

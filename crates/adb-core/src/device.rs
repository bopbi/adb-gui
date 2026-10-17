use std::fmt;

/// Connection state of an adb device, from `adb devices -l`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Device,
    Offline,
    Unauthorized,
}

impl fmt::Display for DeviceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            DeviceState::Device => "device",
            DeviceState::Offline => "offline",
            DeviceState::Unauthorized => "unauthorized",
        };
        f.write_str(label)
    }
}

/// A single adb device or emulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub state: DeviceState,
    /// Value of the `model:` field from `adb devices -l`, when present.
    pub model: Option<String>,
}

impl Device {
    /// Serials of emulators start with `emulator-` (e.g. `emulator-5554`).
    pub fn is_emulator(&self) -> bool {
        self.serial.starts_with("emulator-")
    }
}

/// Parse `adb devices -l` output into devices. Malformed or blank rows are
/// skipped; the `List of devices attached` header line is ignored. Long-
/// format fields after the state (product:, model:, device:, transport_id:)
/// are parsed for `model:` only.
pub fn parse_devices(output: &str) -> Vec<Device> {
    output.lines().filter_map(parse_device_line).collect()
}

fn parse_device_line(line: &str) -> Option<Device> {
    let mut fields = line.split_whitespace();
    let serial = fields.next()?;
    if serial == "List" {
        return None; // header row
    }
    let state = match fields.next()? {
        "device" => DeviceState::Device,
        "offline" => DeviceState::Offline,
        "unauthorized" => DeviceState::Unauthorized,
        _ => return None, // unknown/unexpected state: skip row
    };
    let model = fields
        .find_map(|field| field.strip_prefix("model:"))
        .map(str::to_string);
    Some(Device {
        serial: serial.to_string(),
        state,
        model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_device_list() {
        let output = "List of devices attached\n\
                      emulator-5554          device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emulator64arm transport_id:1\n\
                      0a1b2c3d4e5f6g7h       device product:graphene model:Pixel_8 device:shiba transport_id:2\n";
        let devices = parse_devices(output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].serial, "emulator-5554");
        assert_eq!(devices[0].state, DeviceState::Device);
        assert_eq!(devices[0].model.as_deref(), Some("sdk_gphone64_arm64"));
        assert!(devices[0].is_emulator());
        assert_eq!(devices[1].serial, "0a1b2c3d4e5f6g7h");
        assert!(!devices[1].is_emulator());
        assert_eq!(devices[1].model.as_deref(), Some("Pixel_8"));
    }

    #[test]
    fn parses_offline_and_unauthorized_rows() {
        let output = "List of devices attached\n\
                      deadbeef               offline\n\
                      cafef00d               unauthorized usb:2-1.2 transport_id:3\n";
        let devices = parse_devices(output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].state, DeviceState::Offline);
        assert_eq!(devices[0].model, None);
        assert_eq!(devices[1].state, DeviceState::Unauthorized);
    }

    #[test]
    fn parses_no_permissions_row() {
        // `adb devices` (without -l) prints a bare "no permissions" note on
        // the same line as the serial in some versions.
        let output = "List of devices attached\n\
                      ????????????           no permissions usb:1-1\n";
        let devices = parse_devices(output);
        // "no" is not a known state -> row skipped, list survives.
        assert!(devices.is_empty());
    }

    #[test]
    fn skips_malformed_rows_but_keeps_valid_ones() {
        let output = "List of devices attached\n\
                      \n\
                      garbage-line\n\
                      serial-only\n\
                      good                     device model:Good_Phone\n";
        let devices = parse_devices(output);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].serial, "good");
        assert_eq!(devices[0].model.as_deref(), Some("Good_Phone"));
    }

    #[test]
    fn empty_output_yields_empty_list() {
        assert!(parse_devices("").is_empty());
        assert!(parse_devices("List of devices attached\n").is_empty());
    }
}

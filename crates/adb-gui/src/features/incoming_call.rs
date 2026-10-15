//! Incoming-call simulation: `gsm call <number>`. The number is validated
//! to a phone-character set and passed as one argv element — never
//! interpolated into a shell string. Emulator console only.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Availability, Feature, FeatureInput, FeatureKind};

pub const INCOMING_CALL_ID: &str = "simulate.incoming-call";

pub struct IncomingCall;

pub fn validate_number(command: &str, number: &str) -> Result<(), AdbError> {
    let valid = !number.is_empty()
        && number
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | ' ' | '(' | ')'));
    if !valid {
        return Err(AdbError::CommandFailed {
            command: command.to_string(),
            stderr: format!("invalid phone number: {number:?}"),
        });
    }
    Ok(())
}

impl Feature for IncomingCall {
    fn id(&self) -> &'static str {
        INCOMING_CALL_ID
    }

    fn title(&self) -> &'static str {
        "Incoming call"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::TextAction
    }

    fn availability(&self) -> Availability {
        Availability::EmulatorOnly
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Text(number) = input else {
            return Ok(());
        };
        let number = number.trim();
        if number.is_empty() {
            return Ok(());
        }
        validate_number("gsm call", number)?;
        client.emu(serial, &["gsm", "call", number])?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn call_sends_the_number_as_one_argv_element() {
        let log = test_util::log_path("gsm-call");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-gsm-call", &log));
        IncomingCall
            .perform(
                &client,
                "emulator-5554",
                FeatureInput::Text("+1 555-1234".to_string()),
            )
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            ["-s emulator-5554 emu gsm call +1 555-1234".to_string()],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn call_rejects_empty_and_non_phone_input() {
        let log = test_util::log_path("gsm-call-bad");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-gsm-call-bad", &log));
        // Empty text is a silent no-op (the app never dispatches it);
        // non-phone shapes must fail clearly.
        IncomingCall
            .perform(&client, "emulator-5554", FeatureInput::Text(String::new()))
            .unwrap();
        for bad in ["rm -rf /", "555;id"] {
            let err = IncomingCall
                .perform(
                    &client,
                    "emulator-5554",
                    FeatureInput::Text(bad.to_string()),
                )
                .unwrap_err();
            assert!(
                err.to_string().contains("gsm call"),
                "bad input {bad:?} must fail clearly: {err}"
            );
        }
        assert!(!log.exists(), "rejected input must not touch the device");
        test_util::clean_up(&log);
    }
}

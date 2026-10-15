//! Incoming-SMS simulation: `sms send <number> '<quoted text>'`. The cell
//! input uses the documented `number: message` convention (split on the
//! first colon); the message goes through adb-core's shell quoting, never
//! string interpolation. Emulator console only.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Availability, Feature, FeatureInput, FeatureKind};
use crate::features::incoming_call;

pub const INCOMING_SMS_ID: &str = "simulate.incoming-sms";

pub struct IncomingSms;

impl Feature for IncomingSms {
    fn id(&self) -> &'static str {
        INCOMING_SMS_ID
    }

    /// The convention is documented in the title, next to the input.
    fn title(&self) -> &'static str {
        "Send SMS (number: message)"
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
        let FeatureInput::Text(text) = input else {
            return Ok(());
        };
        let Some((number, message)) = text.split_once(':') else {
            return Err(AdbError::CommandFailed {
                command: "sms send".to_string(),
                stderr: "expected \"<number>: <message>\"".to_string(),
            });
        };
        let number = number.trim();
        let message = message.trim();
        if message.is_empty() {
            return Err(AdbError::CommandFailed {
                command: "sms send".to_string(),
                stderr: "message must not be empty".to_string(),
            });
        }
        incoming_call::validate_number("sms send", number)?;
        // The console tokenizes on whitespace and takes the rest of the line
        // as the message — it is NOT a shell, so no quoting: pass the raw
        // text as one argv element.
        client.emu(serial, &["sms", "send", number, message])?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn sms_sends_number_and_raw_message_text() {
        // The console is whitespace-tokenized, not a shell: the message
        // arrives verbatim — quotes would have arrived literally (L2).
        let log = test_util::log_path("sms");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-sms", &log));
        IncomingSms
            .perform(
                &client,
                "emulator-5554",
                FeatureInput::Text("5551234: it's $(here)".to_string()),
            )
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            ["-s emulator-5554 emu sms send 5551234 it's $(here)".to_string()],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn sms_rejects_missing_colon_or_empty_message() {
        let log = test_util::log_path("sms-bad");
        let client = AdbClient::new(test_util::fake_adb_emu("fake-adb-sms-bad", &log));
        for bad in ["no colon here", "5551234:", ";: rm"] {
            let err = IncomingSms
                .perform(
                    &client,
                    "emulator-5554",
                    FeatureInput::Text(bad.to_string()),
                )
                .unwrap_err();
            assert!(
                err.to_string().contains("sms send"),
                "bad input {bad:?} must fail clearly: {err}"
            );
        }
        assert!(!log.exists(), "rejected input must not touch the device");
        test_util::clean_up(&log);
    }
}

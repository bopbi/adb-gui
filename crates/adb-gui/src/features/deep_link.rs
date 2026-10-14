//! Open a deep link (URI) via the foreground-independent intent dispatcher.
//! The URI is typed into the cell and shell-quoted so metacharacters in the
//! URL cannot be interpreted by the device shell.

use adb_core::{AdbClient, AdbError};

use crate::feature::{Feature, FeatureInput, FeatureKind};
use crate::features::foreground_app::current_package;

pub const DEEP_LINK_ID: &str = "app.deep-link";

/// Resolve the foreground package so a failure is clear when nothing is
/// foregrounded, matching the other app actions; the intent itself is
/// package-independent.
pub struct DeepLink;

impl Feature for DeepLink {
    fn id(&self) -> &'static str {
        DEEP_LINK_ID
    }

    fn title(&self) -> &'static str {
        "Open deep link"
    }

    fn kind(&self) -> FeatureKind {
        FeatureKind::TextAction
    }

    fn perform(
        &self,
        client: &AdbClient,
        serial: &str,
        input: FeatureInput,
    ) -> Result<(), AdbError> {
        let FeatureInput::Text(uri) = input else {
            return Ok(());
        };
        if uri.trim().is_empty() {
            return Ok(());
        }
        current_package(client, serial)?;
        let quoted = adb_core::shell_quote::quote_for_shell(&uri);
        client.shell(
            serial,
            &[
                "am",
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
                &quoted,
            ],
        )?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::test_util;

    #[test]
    fn deep_link_quotes_the_uri_for_the_device_shell() {
        let log = test_util::log_path("deep-link");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-deep-link", &log));
        DeepLink
            .perform(
                &client,
                "s",
                FeatureInput::Text("https://example.com/x?a=1&b=$(id)".to_string()),
            )
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell dumpsys activity activities".to_string(),
                "-s s shell am start -a android.intent.action.VIEW -d 'https://example.com/x?a=1&b=$(id)'".to_string(),
            ],
        );
        test_util::clean_up(&log);
    }

    #[test]
    fn deep_link_with_apostrophe_is_escaped() {
        let log = test_util::log_path("deep-link-quote");
        let client = AdbClient::new(test_util::fake_adb("fake-adb-deep-link-quote", &log));
        DeepLink
            .perform(
                &client,
                "s",
                FeatureInput::Text("myapp://it's-here".to_string()),
            )
            .unwrap();
        assert_eq!(
            test_util::logged(&log),
            [
                "-s s shell dumpsys activity activities".to_string(),
                "-s s shell am start -a android.intent.action.VIEW -d 'myapp://it'\\''s-here'"
                    .to_string(),
            ],
        );
        test_util::clean_up(&log);
    }
}

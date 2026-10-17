/// POSIX single-quote escaping for text that will be interpolated into an
/// `adb shell` command line (e.g. `input text`, `sms send`). Wrap the result
/// in single quotes at the call site, or use [`quote_for_shell`] which does.
///
/// `'it''s'` is how a literal single quote is embedded inside single quotes.
pub fn escape_single_quoted(text: &str) -> String {
    text.replace('\'', "'\\''")
}

/// Escape `text` and wrap it in single quotes, ready to pass as one shell
/// word to `adb shell`.
pub fn quote_for_shell(text: &str) -> String {
    format!("'{}'", escape_single_quoted(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(escape_single_quoted("hello world"), "hello world");
        assert_eq!(quote_for_shell("hello world"), "'hello world'");
    }

    #[test]
    fn single_quote_is_escaped() {
        assert_eq!(escape_single_quoted("it's"), "it'\\''s");
        assert_eq!(quote_for_shell("it's"), "'it'\\''s'");
    }

    #[test]
    fn double_quotes_stay_literal_inside_single_quotes() {
        assert_eq!(quote_for_shell("say \"hi\""), "'say \"hi\"'");
    }

    #[test]
    fn dollar_and_backtick_cannot_expand() {
        // Inside single quotes $ and ` are literal; escaping must not break that.
        assert_eq!(quote_for_shell("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(quote_for_shell("`id`"), "'`id`'");
    }

    #[test]
    fn backslash_is_literal_inside_single_quotes() {
        assert_eq!(quote_for_shell("C:\\path\\to"), "'C:\\path\\to'");
    }

    #[test]
    fn multiple_and_adjacent_quotes() {
        assert_eq!(quote_for_shell("''"), "''\\'''\\'''");
        assert_eq!(quote_for_shell("a'b'c"), "'a'\\''b'\\''c'");
    }

    #[test]
    fn empty_string() {
        assert_eq!(quote_for_shell(""), "''");
    }
}

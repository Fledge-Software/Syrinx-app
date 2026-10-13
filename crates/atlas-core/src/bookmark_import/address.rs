//! Decides which exported addresses may become saved pages: web addresses only.

use super::SkipReason;
use super::text::{decode_entities, is_bidi_control};
use crate::WebAddressError;

/// Longest scheme name kept in a skip reason; longer names are cut, never echoed whole.
const MAX_REPORTED_SCHEME: usize = 32;

/// Returns the address to save, or why the bookmark is skipped.
pub(super) fn classify(raw: Option<&str>, max_bytes: usize) -> Result<String, SkipReason> {
    // Exports escape `&` in addresses, so decode before checking anything else.
    let address = decode_entities(raw.unwrap_or_default());
    let address = address.trim_matches(|character: char| character.is_ascii_whitespace());
    if address.is_empty() {
        return Err(SkipReason::MissingAddress);
    }
    if address.len() > max_bytes {
        return Err(SkipReason::AddressTooLong);
    }
    let (scheme, remainder) = split_scheme(address).ok_or(SkipReason::InvalidAddress)?;
    match scheme.as_str() {
        // Normalizing can lengthen an address (punycode, escapes), so check the limit again.
        "http" | "https" => web_address(&scheme, remainder)
            .and_then(|saved| within_limit(saved, max_bytes).ok_or(SkipReason::AddressTooLong)),
        "javascript" => Err(SkipReason::ScriptAddress),
        "data" => Err(SkipReason::DataAddress),
        _ => Err(SkipReason::UnsupportedScheme(reported_scheme(scheme))),
    }
}

/// Splits a lower-cased scheme from the rest, if the text starts with a valid scheme.
fn split_scheme(address: &str) -> Option<(String, &str)> {
    let (scheme, remainder) = address.split_once(':')?;
    let mut characters = scheme.chars();
    let starts_with_letter = characters.next()?.is_ascii_alphabetic();
    let valid = characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'));
    (starts_with_letter && valid).then(|| (scheme.to_ascii_lowercase(), remainder))
}

/// Accepts what navigation would open, after refusing hidden characters.
///
/// The address rule itself is `crate::web_address`, shared with navigation (DW-1).
/// Exports encode spaces and controls, so a raw one means a damaged or crafted file.
/// Like navigation's address recognition, a web address must start `scheme://`.
fn web_address(scheme: &str, remainder: &str) -> Result<String, SkipReason> {
    if !remainder.starts_with("//") || remainder.chars().any(is_hidden_or_space) {
        return Err(SkipReason::InvalidAddress);
    }
    crate::web_address(&format!("{scheme}:{remainder}")).map_err(skip_reason)
}

/// Maps the shared rule's refusal to the preview's reason.
pub(super) fn skip_reason(error: WebAddressError) -> SkipReason {
    match error {
        WebAddressError::Credentials => SkipReason::CredentialsInAddress,
        WebAddressError::Invalid | WebAddressError::MissingHost | WebAddressError::NotWeb => {
            SkipReason::InvalidAddress
        }
    }
}

/// Whitespace, control characters and bidirectional overrides never belong in an address.
fn is_hidden_or_space(character: char) -> bool {
    character.is_whitespace() || character.is_control() || is_bidi_control(character)
}

fn within_limit(address: String, max_bytes: usize) -> Option<String> {
    (address.len() <= max_bytes).then_some(address)
}

/// Keeps an unsupported scheme short enough to show safely in a preview.
fn reported_scheme(mut scheme: String) -> String {
    scheme.truncate(MAX_REPORTED_SCHEME);
    scheme
}

#[cfg(test)]
mod tests {
    use super::super::SkipReason;
    use super::classify;

    const LIMIT: usize = 64;

    #[test]
    fn web_addresses_are_normalized_like_navigation() {
        assert_eq!(
            classify(Some(" HTTPS://Example.test/A?b=1&amp;c=2#F "), LIMIT),
            Ok("https://example.test/A?b=1&c=2#F".to_owned())
        );
        assert_eq!(
            classify(Some("http://[::1]:8080/"), LIMIT),
            Ok("http://[::1]:8080/".to_owned())
        );
    }

    #[test]
    fn scripts_data_and_other_schemes_are_refused_with_reasons() {
        for (raw, reason) in [
            ("javascript:void(0)", SkipReason::ScriptAddress),
            ("JavaScript:alert(1)", SkipReason::ScriptAddress),
            ("data:text/html,x", SkipReason::DataAddress),
            (
                "place:sort=8",
                SkipReason::UnsupportedScheme("place".to_owned()),
            ),
            (
                "file:///synthetic.html",
                SkipReason::UnsupportedScheme("file".to_owned()),
            ),
            (
                "chrome://settings",
                SkipReason::UnsupportedScheme("chrome".to_owned()),
            ),
            (
                "about:blank",
                SkipReason::UnsupportedScheme("about".to_owned()),
            ),
        ] {
            assert_eq!(classify(Some(raw), LIMIT), Err(reason), "{raw}");
        }
    }

    #[test]
    fn malformed_missing_and_oversized_addresses_are_refused() {
        for (raw, reason) in [
            (None, SkipReason::MissingAddress),
            (Some("   "), SkipReason::MissingAddress),
            (Some("example.test"), SkipReason::InvalidAddress),
            (Some("https:example.test"), SkipReason::InvalidAddress),
            (Some("https://:80/"), SkipReason::InvalidAddress),
            (Some("https://a b.test/"), SkipReason::InvalidAddress),
            (Some("https://example.test/\0"), SkipReason::InvalidAddress),
            (Some("1http://example.test/"), SkipReason::InvalidAddress),
            (Some("https://[/"), SkipReason::InvalidAddress),
            (Some("https://[zz::1]/"), SkipReason::InvalidAddress),
            (Some("https://x.test:99999/"), SkipReason::InvalidAddress),
            (Some("https://x.test:8o/"), SkipReason::InvalidAddress),
            (Some("https://a%zz/"), SkipReason::InvalidAddress),
            (
                Some("https://x.test/\u{202E}gpj.exe"),
                SkipReason::InvalidAddress,
            ),
            (Some("https://x\u{2066}.test/"), SkipReason::InvalidAddress),
            (
                Some("https://user:secret@example.test/"),
                SkipReason::CredentialsInAddress,
            ),
        ] {
            assert_eq!(classify(raw, LIMIT), Err(reason), "{raw:?}");
        }
        let long = format!("https://example.test/{}", "a".repeat(LIMIT));
        assert_eq!(
            classify(Some(&long), LIMIT),
            Err(SkipReason::AddressTooLong)
        );
    }

    #[test]
    fn an_address_exactly_at_the_byte_limit_is_accepted() {
        let prefix = "https://example.test/";
        let exact = format!("{prefix}{}", "a".repeat(LIMIT - prefix.len()));
        assert_eq!(exact.len(), LIMIT);
        assert_eq!(classify(Some(&exact), LIMIT), Ok(exact.clone()));
        let over = format!("{exact}a");
        assert_eq!(
            classify(Some(&over), LIMIT),
            Err(SkipReason::AddressTooLong)
        );
    }

    #[test]
    fn valid_hosts_ports_and_literals_are_accepted() {
        for (raw, saved) in [
            ("https://x.test:65535/", "https://x.test:65535/"),
            ("https://x.test:/", "https://x.test/"),
            (
                "https://[2001:db8::1]:443/path",
                "https://[2001:db8::1]/path",
            ),
            (
                "https://café.example.test/",
                "https://xn--caf-dma.example.test/",
            ),
            (
                "https://under_score.example.test",
                "https://under_score.example.test/",
            ),
            // The shared rule reads extra slashes and `!` in a host as navigation does.
            ("https:///path", "https://path/"),
            ("https://a!b.test/", "https://a!b.test/"),
        ] {
            assert_eq!(classify(Some(raw), LIMIT), Ok(saved.to_owned()), "{raw}");
        }
    }

    #[test]
    fn an_address_lengthened_by_normalizing_is_still_bounded() {
        let raw = format!("https://café.test/{}", "a".repeat(LIMIT - 24));
        assert!(raw.len() <= LIMIT);
        assert_eq!(classify(Some(&raw), LIMIT), Err(SkipReason::AddressTooLong));
    }

    #[test]
    fn reported_schemes_are_bounded() {
        let raw = format!("{}:x", "s".repeat(100));
        let Err(SkipReason::UnsupportedScheme(scheme)) = classify(Some(&raw), 1000) else {
            panic!("expected an unsupported scheme");
        };
        assert_eq!(scheme.len(), 32);
    }
}

//! Decides which exported addresses may become saved pages: web addresses only.

use super::SkipReason;
use super::text::decode_entities;

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
        "http" | "https" => web_address(&scheme, remainder),
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

/// Accepts `http(s)://host...` without credentials, whitespace or control characters.
fn web_address(scheme: &str, remainder: &str) -> Result<String, SkipReason> {
    if remainder
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(SkipReason::InvalidAddress);
    }
    let after_slashes = remainder
        .strip_prefix("//")
        .ok_or(SkipReason::InvalidAddress)?;
    let authority_end = after_slashes
        .find(['/', '?', '#'])
        .unwrap_or(after_slashes.len());
    let authority = &after_slashes[..authority_end];
    // Navigation refuses credentials in addresses; import must not save them either.
    if authority.contains('@') {
        return Err(SkipReason::CredentialsInAddress);
    }
    if authority.is_empty() || authority.starts_with(':') {
        return Err(SkipReason::InvalidAddress);
    }
    Ok(format!("{scheme}:{remainder}"))
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
    fn web_addresses_keep_their_path_and_lowercase_only_the_scheme() {
        assert_eq!(
            classify(Some(" HTTPS://Example.test/A?b=1&amp;c=2#F "), LIMIT),
            Ok("https://Example.test/A?b=1&c=2#F".to_owned())
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
            (Some("https:///path"), SkipReason::InvalidAddress),
            (Some("https://:80/"), SkipReason::InvalidAddress),
            (Some("https://a b.test/"), SkipReason::InvalidAddress),
            (Some("https://example.test/\0"), SkipReason::InvalidAddress),
            (Some("1http://example.test/"), SkipReason::InvalidAddress),
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
    fn reported_schemes_are_bounded() {
        let raw = format!("{}:x", "s".repeat(100));
        let Err(SkipReason::UnsupportedScheme(scheme)) = classify(Some(&raw), 1000) else {
            panic!("expected an unsupported scheme");
        };
        assert_eq!(scheme.len(), 32);
    }
}

//! Decides whether typed text is the exact start of a known page's address, so Enter opens that
//! page only on a letter-for-letter match and searches the web otherwise (#127).

/// How typed text continues into a known page's address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressPrefix {
    /// The text is not the start of the address.
    No,
    /// The text starts the host but stops before its end, as `cloud.exam` does for
    /// `cloud.example.test`.
    PartialHost,
    /// The text is the whole host, or the host and the start of its path.
    HostAndPath,
}

/// Compares `typed` with `address` after dropping the scheme and one leading `www.` from both,
/// ignoring case. Only a prefix counts, so `example` never matches `myexample.test`.
pub fn address_prefix(typed: &str, address: &str) -> AddressPrefix {
    let Some(typed) = typed_host_and_path(typed) else {
        return AddressPrefix::No;
    };
    let Ok(address) = url::Url::parse(address) else {
        return AddressPrefix::No;
    };
    let Some(host) = address.host_str() else {
        return AddressPrefix::No;
    };
    let host = without_www(host).to_lowercase();
    if host.len() > typed.len() && host.starts_with(&typed) {
        return AddressPrefix::PartialHost;
    }
    let host_and_path = format!("{host}{}", address.path()).to_lowercase();
    if host_and_path.starts_with(&typed) {
        AddressPrefix::HostAndPath
    } else {
        AddressPrefix::No
    }
}

/// The typed text in the same form as a page's host and path: lower case, without a scheme or
/// `www.`. Text with spaces is a search phrase, never an address prefix.
fn typed_host_and_path(typed: &str) -> Option<String> {
    let typed = typed.trim().to_lowercase();
    if typed.chars().any(char::is_whitespace) {
        return None;
    }
    let typed = typed
        .split_once("://")
        .map_or(typed.as_str(), |(_, rest)| rest);
    let typed = without_www(typed);
    (!typed.is_empty()).then(|| typed.to_owned())
}

fn without_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::{AddressPrefix, address_prefix};

    #[test]
    fn a_host_prefix_matches_after_scheme_and_www_are_dropped() {
        for (typed, address) in [
            ("cloudf", "https://www.cloudflare.example/"),
            ("CLOUDF", "https://www.cloudflare.example/"),
            ("www.cloudf", "https://cloudflare.example/"),
            ("https://cloud", "https://www.cloudflare.example/"),
            ("cloud.exam", "https://cloud.example.test/projects"),
            ("  docs  ", "http://docs.example.com/"),
        ] {
            assert_eq!(
                address_prefix(typed, address),
                AddressPrefix::PartialHost,
                "{typed} on {address}"
            );
        }
    }

    #[test]
    fn the_whole_host_or_a_path_prefix_matches_as_host_and_path() {
        for (typed, address) in [
            ("example.test", "https://example.test/"),
            ("example.test/do", "https://example.test/docs"),
            ("Example.Test/Docs", "https://example.test/docs/intro"),
            ("cloudflare.example", "https://www.cloudflare.example/login"),
        ] {
            assert_eq!(
                address_prefix(typed, address),
                AddressPrefix::HostAndPath,
                "{typed} on {address}"
            );
        }
    }

    #[test]
    fn fuzzy_mid_word_and_cross_boundary_text_never_matches() {
        for (typed, address) in [
            ("cloudflare", "https://www.mongo.example/"),
            ("example", "https://myexample.test/"),
            ("ample", "https://example.test/"),
            ("clfr", "https://cloudflare.example/"),
            ("example.test/x", "https://example.test/docs"),
            ("cloud flare", "https://cloudflare.example/"),
            ("", "https://example.test/"),
            ("www.", "https://example.test/"),
            ("example", "about:blank"),
            ("example", "not an address"),
        ] {
            assert_eq!(
                address_prefix(typed, address),
                AddressPrefix::No,
                "{typed} on {address}"
            );
        }
    }
}

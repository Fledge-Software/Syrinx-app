//! Decides whether typed text is the exact start of a known page's address, so Enter opens that
//! page only on a letter-for-letter match and searches the web otherwise (#127).

/// How typed text continues into a known page's address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressPrefix {
    /// The text is not the start of the address.
    No,
    /// The text stops inside a label of the host, as `cloud.exam` does for `cloud.example.test`.
    WithinLabel,
    /// The text is whole leading labels of a longer host, as `cloudflare` is for
    /// `cloudflare.example`. A typed address must not take this as a match: `bank.example`
    /// would otherwise open `bank.example.invalid`.
    WholeLabels,
    /// The text is the whole host, or the host and the start of its path.
    HostAndPath,
}

/// Compares `typed` with `address` after dropping the scheme and one leading `www.` from both.
/// Hosts ignore case and paths do not; only a prefix counts, so `example` never matches
/// `myexample.test`. Ports, queries and fragments are never compared, and a Unicode host never
/// matches the punycode form `url` stores.
pub fn address_prefix(typed: &str, address: &str) -> AddressPrefix {
    let Some((typed_host, typed_path)) = typed_parts(typed) else {
        return AddressPrefix::No;
    };
    let Ok(address) = url::Url::parse(address) else {
        return AddressPrefix::No;
    };
    let Some(host) = address.host_str() else {
        return AddressPrefix::No;
    };
    let host = without_www(host).to_lowercase();
    // Text without a path may stop anywhere inside the host.
    if typed_path.is_none()
        && let Some(rest) = host.strip_prefix(typed_host.as_str())
        && !rest.is_empty()
    {
        return if rest.starts_with('.') {
            AddressPrefix::WholeLabels
        } else {
            AddressPrefix::WithinLabel
        };
    }
    if host != typed_host {
        return AddressPrefix::No;
    }
    match typed_path {
        Some(path) if !address.path().starts_with(path) => AddressPrefix::No,
        _ => AddressPrefix::HostAndPath,
    }
}

/// Whether typed text names only a host, with no scheme, port, path, query or fragment, so it
/// may still be the start of a longer host.
pub fn is_bare_host(typed: &str) -> bool {
    let typed = typed.trim();
    !typed.is_empty()
        && !typed.contains(['/', '\\', '?', '#', ':', '@'])
        && !typed.chars().any(char::is_whitespace)
}

/// The typed host in lower case without a scheme or `www.`, and its path as typed. Text with
/// spaces is a search phrase, never an address prefix.
fn typed_parts(typed: &str) -> Option<(String, Option<&str>)> {
    let typed = typed.trim();
    if typed.chars().any(char::is_whitespace) {
        return None;
    }
    let typed = typed.split_once("://").map_or(typed, |(_, rest)| rest);
    let (host, path) = match typed.find('/') {
        Some(slash) => (&typed[..slash], Some(&typed[slash..])),
        None => (typed, None),
    };
    let host = without_www(&host.to_lowercase()).to_owned();
    (!host.is_empty()).then_some((host, path))
}

fn without_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::{AddressPrefix, address_prefix, is_bare_host};

    fn assert_prefix(expected: AddressPrefix, cases: &[(&str, &str)]) {
        for (typed, address) in cases {
            assert_eq!(
                address_prefix(typed, address),
                expected,
                "{typed} on {address}"
            );
        }
    }

    #[test]
    fn text_ending_inside_a_host_label_matches_after_scheme_and_www_are_dropped() {
        assert_prefix(
            AddressPrefix::WithinLabel,
            &[
                ("cloudf", "https://www.cloudflare.example/"),
                ("CLOUDF", "https://www.cloudflare.example/"),
                ("www.cloudf", "https://cloudflare.example/"),
                ("https://cloud", "https://www.cloudflare.example/"),
                ("cloud.exam", "https://cloud.example.test/projects"),
                ("example.", "https://example.test/"),
            ],
        );
    }

    #[test]
    fn whole_leading_labels_are_told_apart_from_a_label_prefix() {
        assert_prefix(
            AddressPrefix::WholeLabels,
            &[
                ("cloudflare", "https://www.cloudflare.example/"),
                ("  docs  ", "http://docs.example.com/"),
                // Look-alikes: a typed address must never open these.
                ("example.test", "https://example.test.other.invalid/"),
                ("example.co", "https://example.co.uk/"),
                ("example.test", "https://example.test./"),
            ],
        );
    }

    #[test]
    fn the_whole_host_or_a_path_prefix_matches_as_host_and_path() {
        assert_prefix(
            AddressPrefix::HostAndPath,
            &[
                ("example.test", "https://example.test/"),
                ("example.test/do", "https://example.test/docs"),
                ("Example.Test/docs", "https://example.test/docs/intro"),
                ("cloudflare.example", "https://www.cloudflare.example/login"),
                ("192.0.2.10", "https://192.0.2.10:8443/admin"),
            ],
        );
    }

    #[test]
    fn fuzzy_mid_word_and_cross_boundary_text_never_matches() {
        assert_prefix(
            AddressPrefix::No,
            &[
                ("cloudflare", "https://www.mongo.example/"),
                ("example", "https://myexample.test/"),
                ("ample", "https://example.test/"),
                ("clfr", "https://cloudflare.example/"),
                ("example.test/x", "https://example.test/docs"),
                ("example.test/Docs", "https://example.test/docs"),
                ("example.tes/docs", "https://example.test/docs"),
                ("cloud flare", "https://cloudflare.example/"),
                ("", "https://example.test/"),
                ("www.", "https://example.test/"),
                ("example", "about:blank"),
                ("example", "not an address"),
            ],
        );
    }

    #[test]
    fn only_text_without_scheme_port_or_path_is_a_bare_host() {
        for typed in ["cloud.example", "  example  ", "192.0.2.10"] {
            assert!(is_bare_host(typed), "{typed}");
        }
        for typed in [
            "",
            "https://cloud.example",
            "localhost:3000",
            "example.test/docs",
            "example.test?q",
            "user@example.test",
            "two words",
        ] {
            assert!(!is_bare_host(typed), "{typed}");
        }
    }
}

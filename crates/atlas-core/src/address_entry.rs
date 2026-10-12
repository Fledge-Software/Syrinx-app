//! Decides whether address-bar text opens a web address or searches Google (P1-32, D9).
//!
//! Every function here is pure text handling: nothing in this module, or in `atlas-core`, can
//! open a connection, so typing in the address bar or palette stays on the machine.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use crate::{WebAddressError, web_address};

/// Why submitted address-bar text cannot be opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryError {
    /// Nothing but white space was submitted.
    Empty,
    /// The text names a scheme Atlas does not open from the address bar.
    UnsupportedScheme,
    /// The text is an `http`/`https` address that the shared rule refuses.
    Address(WebAddressError),
}

impl EntryError {
    /// The sentence shown to the owner; it never quotes the text.
    pub fn message(self) -> &'static str {
        match self {
            Self::Empty => "Enter an address or a search.",
            Self::UnsupportedScheme => WebAddressError::NotWeb.message(),
            Self::Address(error) => error.message(),
        }
    }
}

impl fmt::Display for EntryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for EntryError {}

/// Where submitted text goes: Alt Enter (`force_search`) always searches Google; otherwise a
/// recognized address opens and any other text is searched.
pub fn entry_destination(input: &str, force_search: bool) -> Result<String, EntryError> {
    let text = submission_text(input)?;
    if force_search {
        return Ok(google_search(text));
    }
    Ok(recognized_address(text)?.unwrap_or_else(|| google_search(text)))
}

/// The address that `text` names, `None` when it should be searched, or an error when it names
/// an address Atlas refuses (so a refused address never silently becomes a search).
pub fn recognized_address(input: &str) -> Result<Option<String>, EntryError> {
    let text = submission_text(input)?;
    // Recognize schemes without changing the case of paths, queries or search text.
    let (scheme, remainder) = text.split_once(':').unwrap_or(("", text));
    if scheme.eq_ignore_ascii_case("about") && remainder == "blank" {
        return Ok(Some("about:blank".to_owned()));
    }
    if (scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
        && remainder.starts_with("//")
    {
        return checked(text).map(Some);
    }
    if text.contains("://")
        || scheme.eq_ignore_ascii_case("javascript")
        || scheme.eq_ignore_ascii_case("data")
    {
        return Err(EntryError::UnsupportedScheme);
    }
    let Some(address) = schemeless_address(text) else {
        return Ok(None);
    };
    checked(&address).map(Some)
}

/// The Google search address for `text`, the one engine D9 chose. Building it sends nothing.
pub fn google_search(text: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("q", text)
        .finish();
    format!("https://www.google.com/search?{query}")
}

// Both submission modes and row classification require nonempty trimmed text.
fn submission_text(input: &str) -> Result<&str, EntryError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(EntryError::Empty);
    }
    Ok(text)
}

fn checked(text: &str) -> Result<String, EntryError> {
    web_address(text).map_err(EntryError::Address)
}

/// The full address for schemeless text whose host is `localhost`, an IP address or a dotted
/// name with a plausible top-level label, or `None` to search it. Loopback hosts open over
/// `http`, like a local development server; every other host opens over `https`.
fn schemeless_address(text: &str) -> Option<String> {
    if text.chars().any(char::is_whitespace) {
        return None;
    }
    let authority = text.split(['/', '?', '#']).next().unwrap_or(text);
    // An e-mail address or `user@host` is searched, never opened with credentials.
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    // A bare IPv6 literal has no port and needs brackets to become an address.
    if let Ok(address) = authority.parse::<Ipv6Addr>() {
        let rest = &text[authority.len()..];
        return Some(format!(
            "{}://[{authority}]{rest}",
            scheme(address.is_loopback())
        ));
    }
    let host = host_without_port(authority)?;
    let local = match host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        Some(literal) => literal.parse::<Ipv6Addr>().ok()?.is_loopback(),
        None => is_local_name(&host.to_lowercase())?,
    };
    Some(format!("{}://{text}", scheme(local)))
}

fn scheme(local: bool) -> &'static str {
    if local { "http" } else { "https" }
}

// Split `host:port`, accepting only a numeric port so `word:word` stays a search.
fn host_without_port(authority: &str) -> Option<&str> {
    if authority.starts_with('[') {
        let end = authority.find(']')?;
        let (host, port) = authority.split_at(end + 1);
        return match port.strip_prefix(':') {
            None if port.is_empty() => Some(host),
            Some(digits) if is_port(digits) => Some(host),
            _ => None,
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if is_port(port) => Some(host),
        Some(_) => None,
        None => Some(authority),
    }
}

fn is_port(digits: &str) -> bool {
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether a named or IPv4 host is loopback, or `None` when it does not look like a host.
fn is_local_name(host: &str) -> Option<bool> {
    if host == "localhost" || host.ends_with(".localhost") {
        return Some(true);
    }
    if let Ok(address) = host.parse::<Ipv4Addr>() {
        return Some(address.is_loopback());
    }
    has_plausible_top_label(host).then_some(false)
}

/// A dotted name whose last label could be a top-level domain: two or more characters, starting
/// with a letter, so `1.5`, `e.g.` and `a.b` are searched. Without the public suffix list a name
/// like `node.js` still opens as an address.
fn has_plausible_top_label(host: &str) -> bool {
    let name = host.strip_suffix('.').unwrap_or(host);
    let mut labels = name.split('.');
    let Some(top) = labels.next_back() else {
        return false;
    };
    let mut rest = labels.peekable();
    if rest.peek().is_none() || rest.any(str::is_empty) {
        return false;
    }
    top.chars().count() >= 2
        && top.chars().next().is_some_and(char::is_alphabetic)
        && top
            .chars()
            .all(|character| character.is_alphanumeric() || character == '-')
}

#[cfg(test)]
mod tests {
    use super::{EntryError, entry_destination, google_search, recognized_address};
    use crate::WebAddressError;

    fn opens(input: &str) -> Option<String> {
        recognized_address(input).expect("synthetic input is accepted")
    }

    #[test]
    fn bare_hosts_ports_and_ip_addresses_navigate() {
        for (input, expected) in [
            ("example.test", "https://example.test/"),
            (
                "example.test/path?q=1#part",
                "https://example.test/path?q=1#part",
            ),
            ("Sub.Example.TEST/Case", "https://sub.example.test/Case"),
            ("example.test:8443", "https://example.test:8443/"),
            ("localhost", "http://localhost/"),
            ("localhost:8080", "http://localhost:8080/"),
            ("LOCALHOST:8080/path", "http://localhost:8080/path"),
            ("app.localhost:3000", "http://app.localhost:3000/"),
            ("127.0.0.1:8080/path", "http://127.0.0.1:8080/path"),
            ("192.0.2.10", "https://192.0.2.10/"),
            ("192.0.2.10:8443/x", "https://192.0.2.10:8443/x"),
            ("[::1]:8443/path", "http://[::1]:8443/path"),
            ("[2001:db8::1]", "https://[2001:db8::1]/"),
            ("[2001:db8::1]:8443/x", "https://[2001:db8::1]:8443/x"),
            ("::1", "http://[::1]/"),
            ("2001:db8::1/path", "https://[2001:db8::1]/path"),
            ("bücher.example", "https://xn--bcher-kva.example/"),
            ("example.xn--p1ai", "https://example.xn--p1ai/"),
        ] {
            assert_eq!(opens(input).as_deref(), Some(expected), "{input}");
            assert_eq!(entry_destination(input, false).as_deref(), Ok(expected));
        }
    }

    #[test]
    fn words_spaces_and_lookalikes_are_searched() {
        for input in [
            "synthetic",
            "two words & symbols",
            "example.test with words",
            "localhosting",
            "localhost-like:8080",
            "1.5",
            "3.14159",
            "e.g.",
            "a.b",
            "version 2.0",
            "word:word",
            "example.test:http",
            "someone@example.test",
            "mailto:someone@example.test",
            ".example",
            "example..test",
            "C++",
        ] {
            assert_eq!(opens(input), None, "{input}");
            assert_eq!(
                entry_destination(input, false),
                Ok(google_search(input)),
                "{input}"
            );
        }
    }

    #[test]
    fn alt_enter_always_searches_with_the_text_intact() {
        for text in [
            "example.test",
            "localhost:8080",
            "HTTPS://example.test/Case?Key=Value#Part",
            "JAVASCRIPT:1",
            "aBoUt:blank",
            "two words & symbols",
        ] {
            let result = entry_destination(text, true).expect("search address");
            let address = url::Url::parse(&result).expect("search address parses");
            assert_eq!(address.host_str(), Some("www.google.com"));
            assert_eq!(address.path(), "/search");
            assert_eq!(
                address.query_pairs().into_owned().collect::<Vec<_>>(),
                [("q".to_owned(), text.to_owned())]
            );
        }
    }

    #[test]
    fn explicit_schemes_keep_their_case_rules() {
        for scheme in ["http", "HTTP", "hTtP", "https", "HTTPS", "hTtPs"] {
            assert_eq!(
                entry_destination(
                    &format!("{scheme}://example.test/Case?Key=Value#Part"),
                    false
                ),
                Ok(format!(
                    "{}://example.test/Case?Key=Value#Part",
                    scheme.to_ascii_lowercase()
                ))
            );
            assert_eq!(
                entry_destination(&format!("{scheme}://synthetic@example.test/"), false),
                Err(EntryError::Address(WebAddressError::Credentials))
            );
            assert!(entry_destination(&format!("{scheme}://[invalid]"), false).is_err());
        }
        assert_eq!(opens("aBoUt:blank").as_deref(), Some("about:blank"));
        assert_eq!(opens("about:Blank"), None);
    }

    #[test]
    fn refused_schemes_and_empty_input_never_become_searches() {
        for text in [
            "javascript:1",
            "JAVASCRIPT:1",
            "jAvAsCrIpT:1",
            "data:text/plain,synthetic",
            "DaTa:text/plain,synthetic",
            "file:///synthetic.txt",
            "FILE:///synthetic.txt",
            "ftp://example.test/",
        ] {
            assert_eq!(
                entry_destination(text, false),
                Err(EntryError::UnsupportedScheme),
                "{text}"
            );
            assert!(entry_destination(text, true).is_ok());
        }
        for text in ["", " \t\n"] {
            assert_eq!(entry_destination(text, false), Err(EntryError::Empty));
            assert_eq!(entry_destination(text, true), Err(EntryError::Empty));
        }
        assert_eq!(
            entry_destination("https://[invalid]", false),
            Err(EntryError::Address(WebAddressError::Invalid))
        );
    }

    #[test]
    fn messages_never_quote_the_text() {
        for error in [
            EntryError::Empty,
            EntryError::UnsupportedScheme,
            EntryError::Address(WebAddressError::Credentials),
        ] {
            assert!(!error.message().contains("example"));
        }
        assert_eq!(
            EntryError::UnsupportedScheme.message(),
            "This preview opens HTTP and HTTPS addresses."
        );
    }
}

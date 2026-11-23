//! The checks on what a plugin puts in a `service` request (a path that names no host, headers
//! that cannot override what the host sets) and on which response headers it may read.

use crate::bindings::syrinx::plugin::service::Header;
use crate::limits;

use super::{Credential, ServiceRefusal};

/// Headers that frame or route the request: only the transport sets them, and a credential
/// sent under one could smuggle a second request past a naive transport.
pub(super) const FRAMING_HEADERS: &[&str] = &[
    "connection",
    "content-length",
    "expect",
    "host",
    "keep-alive",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Besides the framing headers, the credentials only the host may set.
const CREDENTIAL_HEADERS: &[&str] = &["authorization", "cookie"];

/// Refuse a path that is not one absolute path on the approved address.
pub(super) fn check_path(path: &str) -> Result<(), ServiceRefusal> {
    if names_a_host(path) {
        return Err(ServiceRefusal::OtherHost);
    }
    if path.len() > limits::MAX_SERVICE_PATH_BYTES {
        return Err(ServiceRefusal::InvalidPath("it is too long"));
    }
    if !path.starts_with('/') {
        return Err(ServiceRefusal::InvalidPath("it must start with /"));
    }
    if !path.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ServiceRefusal::InvalidPath(
            "it may hold only visible ASCII; percent-encode the rest",
        ));
    }
    if path.contains(['#', '\\']) {
        return Err(ServiceRefusal::InvalidPath("it may not hold # or \\"));
    }
    if has_dot_segment(path) {
        return Err(ServiceRefusal::InvalidPath(
            "it may not hold . or .. segments",
        ));
    }
    Ok(())
}

/// A scheme (`https://other.test/`) or a network-path reference (`//other.test/`).
fn names_a_host(path: &str) -> bool {
    if path.starts_with("//") || path.starts_with("\\\\") {
        return true;
    }
    let before_query = path.split('?').next().unwrap_or(path);
    let first_segment = before_query.split('/').next().unwrap_or(before_query);
    first_segment.contains(':')
}

fn has_dot_segment(path: &str) -> bool {
    let before_query = path.split('?').next().unwrap_or(path);
    before_query
        .split('/')
        .any(|segment| segment == "." || segment == "..")
}

/// Refuse a header the host sets itself, a malformed one, or too many.
pub(super) fn check_headers(
    headers: &[Header],
    credential: Option<&Credential>,
) -> Result<(), ServiceRefusal> {
    if headers.len() > limits::MAX_SERVICE_HEADERS {
        return Err(ServiceRefusal::InvalidHeader("there are too many headers"));
    }
    if header_bytes(headers) > limits::MAX_SERVICE_HEADER_BYTES {
        return Err(ServiceRefusal::InvalidHeader("the headers are too large"));
    }
    for header in headers {
        check_header(header, credential)?;
    }
    Ok(())
}

fn check_header(header: &Header, credential: Option<&Credential>) -> Result<(), ServiceRefusal> {
    if !is_token(&header.name) {
        return Err(ServiceRefusal::InvalidHeader(
            "a header name is not an HTTP token",
        ));
    }
    if !is_header_value(&header.value) {
        return Err(ServiceRefusal::InvalidHeader(
            "a header value holds a line break or control character",
        ));
    }
    if is_reserved_header(&header.name, credential) {
        return Err(ServiceRefusal::ReservedHeader(
            header.name.to_ascii_lowercase(),
        ));
    }
    Ok(())
}

/// The bytes of every header's name and value together.
pub(super) fn header_bytes(headers: &[Header]) -> usize {
    headers
        .iter()
        .map(|header| header.name.len() + header.value.len())
        .sum()
}

/// Whether only the host may set `name`: a framing or credential header, a `proxy-` or `sec-`
/// header, or the credential's own.
fn is_reserved_header(name: &str, credential: Option<&Credential>) -> bool {
    let name = name.to_ascii_lowercase();
    FRAMING_HEADERS.contains(&name.as_str())
        || CREDENTIAL_HEADERS.contains(&name.as_str())
        || name.starts_with("proxy-")
        || name.starts_with("sec-")
        || credential.is_some_and(|credential| credential.header() == name)
}

/// Whether a response header is kept from the plugin: a cookie the server sets, or a header of
/// the credential's name, which a server might echo.
pub(super) fn is_withheld_response_header(name: &str, credential: Option<&Credential>) -> bool {
    let name = name.to_ascii_lowercase();
    name == "set-cookie"
        || name == "set-cookie2"
        || credential.is_some_and(|credential| credential.header() == name)
}

/// An HTTP token (RFC 9110, 5.6.2): the characters a header name may use.
pub(super) fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 256
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// A header value with no line break or other control character but tab.
pub(super) fn is_header_value(text: &str) -> bool {
    text.chars()
        .all(|character| character == '\t' || !character.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(name: &str, value: &str) -> Header {
        Header {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn a_path_that_names_a_host_is_another_host() {
        for path in [
            "//other.example.test/x",
            "https://other.example.test/x",
            "http:/x",
            "\\\\other\\x",
        ] {
            assert_eq!(check_path(path), Err(ServiceRefusal::OtherHost), "{path}");
        }
    }

    #[test]
    fn an_absolute_path_with_a_query_is_accepted() {
        for path in [
            "/",
            "/etapi/notes?search=a%20b&limit=5",
            "/a/b:c",
            "/x?next=//y",
        ] {
            assert_eq!(check_path(path), Ok(()), "{path}");
        }
    }

    #[test]
    fn a_relative_or_odd_path_is_refused() {
        for path in ["notes", "", "/a b", "/a#b", "/a/../b", "/./a", "/é"] {
            assert!(
                matches!(check_path(path), Err(ServiceRefusal::InvalidPath(_))),
                "{path}"
            );
        }
        let long = format!("/{}", "a".repeat(limits::MAX_SERVICE_PATH_BYTES));
        assert!(matches!(
            check_path(&long),
            Err(ServiceRefusal::InvalidPath(_))
        ));
    }

    #[test]
    fn the_host_sets_routing_length_cookies_and_credentials() {
        let credential = Credential::new("x-api-key", "synthetic").expect("valid");
        for name in [
            "Host",
            "Authorization",
            "Cookie",
            "Proxy-Authorization",
            "X-API-Key",
        ] {
            assert_eq!(
                check_headers(&[header(name, "v")], Some(&credential)),
                Err(ServiceRefusal::ReservedHeader(name.to_ascii_lowercase())),
                "{name}"
            );
        }
        assert_eq!(
            check_headers(&[header("accept", "application/json")], Some(&credential)),
            Ok(())
        );
    }

    #[test]
    fn only_cookies_and_the_credential_s_header_are_withheld_from_a_response() {
        let credential = Credential::new("x-api-key", "synthetic").expect("valid");
        for name in ["Set-Cookie", "set-cookie2", "X-API-Key"] {
            assert!(
                is_withheld_response_header(name, Some(&credential)),
                "{name}"
            );
        }
        for name in [
            "content-length",
            "content-type",
            "location",
            "etag",
            "sec-fetch-site",
        ] {
            assert!(
                !is_withheld_response_header(name, Some(&credential)),
                "{name}"
            );
        }
    }

    #[test]
    fn a_header_cannot_split_or_flood_the_request() {
        let split = [header("x-a", "v\r\nhost: other.example.test")];
        assert!(matches!(
            check_headers(&split, None),
            Err(ServiceRefusal::InvalidHeader(_))
        ));
        let many = vec![header("x-a", "v"); limits::MAX_SERVICE_HEADERS + 1];
        assert!(matches!(
            check_headers(&many, None),
            Err(ServiceRefusal::InvalidHeader(_))
        ));
        assert!(matches!(
            check_headers(&[header("bad name", "v")], None),
            Err(ServiceRefusal::InvalidHeader(_))
        ));
    }
}

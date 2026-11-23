//! The one address a plugin's `service` requests go to, chosen outside the plugin, which
//! cannot change it (#280).

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Why an address or credential was refused when the owner (or the app) gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressError {
    /// Neither `https://` nor, for a loopback host, `http://`.
    Scheme,
    /// Plain HTTP to a host that is not this computer: the owner chose HTTPS for remote ones.
    InsecureRemote,
    /// A user name or password in the address; a credential is given separately.
    UserInfo,
    /// A path, query or fragment; the plugin supplies the path of each request.
    Path,
    Host,
    Port,
    /// A credential header name that is not an HTTP token, or a value that could split a header.
    Credential,
}

impl fmt::Display for AddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Scheme => "the address must start with https://",
            Self::InsecureRemote => "only an address on this computer may use plain http://",
            Self::UserInfo => "the address may not carry a user name or password",
            Self::Path => "the address may not carry a path, query or fragment",
            Self::Host => "the address has no valid host",
            Self::Port => "the address has an invalid port",
            Self::Credential => "the credential's header name or value is not valid",
        })
    }
}

impl std::error::Error for AddressError {}

/// Scheme, host and port, checked: HTTPS, or plain HTTP to this computer only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceAddress {
    secure: bool,
    /// Lowercase; an IPv6 literal keeps its brackets.
    host: String,
    port: u16,
}

impl ServiceAddress {
    /// Read an address such as `https://notes.example.test` or `http://127.0.0.1:8080`.
    pub fn parse(text: &str) -> Result<Self, AddressError> {
        let (secure, rest) = split_scheme(text)?;
        let authority = rest.strip_suffix('/').unwrap_or(rest);
        if authority.contains(['/', '?', '#']) {
            return Err(AddressError::Path);
        }
        if authority.contains('@') {
            return Err(AddressError::UserInfo);
        }
        let (host, port) = split_authority(authority, secure)?;
        if !secure && !is_loopback(&host) {
            return Err(AddressError::InsecureRemote);
        }
        Ok(Self { secure, host, port })
    }

    pub fn is_secure(&self) -> bool {
        self.secure
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn is_loopback(&self) -> bool {
        is_loopback(&self.host)
    }

    pub fn scheme(&self) -> &'static str {
        if self.secure { "https" } else { "http" }
    }

    /// The port the scheme implies when an address names none.
    fn default_port(secure: bool) -> u16 {
        if secure { 443 } else { 80 }
    }

    /// Whether `scheme://authority` names this same address.
    pub(super) fn is_same_origin(&self, scheme: &str, authority: &str) -> bool {
        let secure = match scheme.to_ascii_lowercase().as_str() {
            "https" => true,
            "http" => false,
            _ => return false,
        };
        if secure != self.secure || authority.contains('@') {
            return false;
        }
        split_authority(authority, secure)
            .is_ok_and(|(host, port)| host == self.host && port == self.port)
    }
}

impl fmt::Display for ServiceAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}://{}:{}", self.scheme(), self.host, self.port)
    }
}

fn split_scheme(text: &str) -> Result<(bool, &str), AddressError> {
    let lower = text
        .get(..8)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if lower == "https://" {
        return Ok((true, &text[8..]));
    }
    if lower.starts_with("http://") {
        return Ok((false, &text[7..]));
    }
    Err(AddressError::Scheme)
}

/// The lowercase host and the port, the scheme's default when none is written.
fn split_authority(authority: &str, secure: bool) -> Result<(String, u16), AddressError> {
    let (host, port) = match authority.strip_prefix('[') {
        Some(bracketed) => {
            let (inside, after) = bracketed.split_once(']').ok_or(AddressError::Host)?;
            inside.parse::<Ipv6Addr>().map_err(|_| AddressError::Host)?;
            let port = match after {
                "" => None,
                _ => Some(after.strip_prefix(':').ok_or(AddressError::Port)?),
            };
            (format!("[{}]", inside.to_ascii_lowercase()), port)
        }
        None => {
            let (host, port) = match authority.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            };
            check_host_name(host)?;
            (host.to_ascii_lowercase(), port)
        }
    };
    let port = match port {
        None => ServiceAddress::default_port(secure),
        Some(digits) => parse_port(digits)?,
    };
    Ok((host, port))
}

fn parse_port(digits: &str) -> Result<u16, AddressError> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AddressError::Port);
    }
    match digits.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(AddressError::Port),
    }
}

/// A DNS name or dotted IPv4 address: ASCII letters, digits, `-` and `.`, no empty label.
fn check_host_name(host: &str) -> Result<(), AddressError> {
    let well_formed = !host.is_empty()
        && host.len() <= 253
        && host
            .split('.')
            .all(|label| !label.is_empty() && label.len() <= 63)
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.');
    if well_formed {
        return Ok(());
    }
    Err(AddressError::Host)
}

/// This computer: `localhost`, `127.0.0.0/8` or `[::1]`.
fn is_loopback(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    if let Some(inside) = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return inside.parse::<Ipv6Addr>().is_ok_and(|ip| ip.is_loopback());
    }
    host.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_takes_any_host_and_defaults_its_port() {
        let address = ServiceAddress::parse("https://Notes.Example.test/").expect("valid");
        assert_eq!(address.to_string(), "https://notes.example.test:443");
        let address = ServiceAddress::parse("https://[::1]:8443").expect("valid");
        assert_eq!((address.host(), address.port()), ("[::1]", 8443));
    }

    #[test]
    fn plain_http_is_for_this_computer_only() {
        for local in [
            "http://127.0.0.1:37840",
            "http://localhost",
            "http://[::1]:9",
        ] {
            assert!(ServiceAddress::parse(local).is_ok(), "{local}");
        }
        assert_eq!(
            ServiceAddress::parse("http://notes.example.test"),
            Err(AddressError::InsecureRemote)
        );
        assert_eq!(
            ServiceAddress::parse("http://10.0.0.1"),
            Err(AddressError::InsecureRemote)
        );
    }

    #[test]
    fn an_address_carries_no_path_user_or_other_scheme() {
        let cases = [
            ("ftp://example.test", AddressError::Scheme),
            ("example.test", AddressError::Scheme),
            ("https://example.test/api", AddressError::Path),
            ("https://example.test?q=1", AddressError::Path),
            ("https://user:pw@example.test", AddressError::UserInfo),
            ("https://", AddressError::Host),
            ("https://exa mple.test", AddressError::Host),
            ("https://example.test:0", AddressError::Port),
            ("https://example.test:70000", AddressError::Port),
            ("https://example.test:", AddressError::Port),
        ];
        for (text, error) in cases {
            assert_eq!(ServiceAddress::parse(text), Err(error), "{text}");
        }
    }

    #[test]
    fn same_origin_compares_scheme_host_and_port() {
        let address = ServiceAddress::parse("https://notes.example.test").expect("valid");
        assert!(address.is_same_origin("https", "notes.example.test"));
        assert!(address.is_same_origin("HTTPS", "NOTES.example.test:443"));
        assert!(!address.is_same_origin("http", "notes.example.test"));
        assert!(!address.is_same_origin("https", "notes.example.test:8443"));
        assert!(!address.is_same_origin("https", "other.example.test"));
        assert!(!address.is_same_origin("https", "x@notes.example.test"));
    }
}

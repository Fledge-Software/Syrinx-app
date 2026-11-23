//! The credential the host adds to a plugin's `service` requests: given outside the plugin,
//! never shown to it, logged or kept in the repository (#280).

use std::fmt;

use super::AddressError;
use super::request::{FRAMING_HEADERS, is_header_value, is_token};

/// The header the host adds to every request, such as an `authorization` token. Its value is
/// never shown: not by `Debug`, not in an error, a log or the plugin's response.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    header: String,
    value: String,
}

impl Credential {
    pub fn new(header: &str, value: impl Into<String>) -> Result<Self, AddressError> {
        let value = value.into();
        let header = header.to_ascii_lowercase();
        let usable = is_token(&header)
            && !FRAMING_HEADERS.contains(&header.as_str())
            && is_header_value(&value);
        if !usable {
            return Err(AddressError::Credential);
        }
        Ok(Self { header, value })
    }

    /// The header's name, lowercase.
    pub fn header(&self) -> &str {
        &self.header
    }

    /// For the transport only, which must send it and never log it.
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credential")
            .field("header", &self.header)
            .field("value", &"<withheld>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_never_shows_its_value() {
        let credential = Credential::new("Authorization", "synthetic-secret").expect("valid");
        let shown = format!("{credential:?}");
        assert!(!shown.contains("synthetic-secret"), "{shown}");
        assert_eq!(credential.header(), "authorization");
    }

    #[test]
    fn a_credential_cannot_split_or_frame_the_request() {
        let refused = [
            ("x-token", "a\r\nx-other: b"),
            ("bad name", "v"),
            ("Host", "other.example.test"),
            ("content-length", "0"),
            ("Transfer-Encoding", "chunked"),
        ];
        for (header, value) in refused {
            assert_eq!(
                Credential::new(header, value),
                Err(AddressError::Credential),
                "{header}"
            );
        }
        assert!(Credential::new("x-api-key", "synthetic").is_ok());
        assert!(Credential::new("cookie", "session=synthetic").is_ok());
    }
}

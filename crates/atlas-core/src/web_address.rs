//! The one rule for which addresses Atlas opens or saves as web pages (DW-1).

use std::fmt;

/// Why text is not a web address Atlas will open or save.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebAddressError {
    /// The text does not parse as an address.
    Invalid,
    /// The address is not `http` or `https`.
    NotWeb,
    /// The address has no website name.
    MissingHost,
    /// The address carries a user name or password, which Atlas never opens or saves.
    Credentials,
}

/// Parses an `http`/`https` address with a host and no credentials, returning its
/// normalized form (lower-case scheme and host, punycode, default path).
///
/// Navigation and bookmark import both call this, so a page that can be typed can be
/// imported and an imported page can be opened.
pub fn web_address(text: &str) -> Result<String, WebAddressError> {
    let address = url::Url::parse(text).map_err(|_| WebAddressError::Invalid)?;
    if !matches!(address.scheme(), "http" | "https") {
        return Err(WebAddressError::NotWeb);
    }
    if address.host_str().is_none_or(str::is_empty) {
        return Err(WebAddressError::MissingHost);
    }
    if !address.username().is_empty() || address.password().is_some() {
        return Err(WebAddressError::Credentials);
    }
    Ok(address.into())
}

impl WebAddressError {
    /// The sentence shown to the owner; it never quotes the address.
    pub fn message(self) -> &'static str {
        match self {
            Self::Invalid => "That address is not valid.",
            Self::NotWeb => "This preview opens HTTP and HTTPS addresses.",
            Self::MissingHost => "That address needs a website name.",
            Self::Credentials => {
                "Sign in on the website instead of putting credentials in an address."
            }
        }
    }
}

impl fmt::Display for WebAddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for WebAddressError {}

#[cfg(test)]
mod tests {
    use super::{WebAddressError, web_address};

    #[test]
    fn web_addresses_are_normalized() {
        assert_eq!(
            web_address("HTTPS://Example.TEST/Case?Key=Value#Part"),
            Ok("https://example.test/Case?Key=Value#Part".to_owned())
        );
        assert_eq!(
            web_address("http://example.test"),
            Ok("http://example.test/".to_owned())
        );
        assert_eq!(
            web_address("https://[::1]:8443/path"),
            Ok("https://[::1]:8443/path".to_owned())
        );
    }

    #[test]
    fn other_schemes_credentials_and_damage_are_refused() {
        for (text, error) in [
            ("javascript:1", WebAddressError::NotWeb),
            ("file:///synthetic.txt", WebAddressError::NotWeb),
            ("ftp://example.test/", WebAddressError::NotWeb),
            (
                "https://synthetic@example.test/",
                WebAddressError::Credentials,
            ),
            ("https://a:b@example.test/", WebAddressError::Credentials),
            ("https://[invalid]", WebAddressError::Invalid),
            ("https://x.test:99999/", WebAddressError::Invalid),
            ("https://a b.test/", WebAddressError::Invalid),
            ("example.test", WebAddressError::Invalid),
        ] {
            assert_eq!(web_address(text), Err(error), "{text}");
        }
    }
}

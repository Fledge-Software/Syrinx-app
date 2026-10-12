//! Encodes what a later launch asks of the running Atlas, and re-checks every request on arrival.

use std::fmt;

use atlas_core::{WebAddressError, web_address};

/// The longest address a launch may forward; a longer one is refused, never truncated.
pub(super) const MAX_ADDRESS_BYTES: usize = 32 * 1024;

/// The largest request message: one tag byte and the longest address.
pub(super) const MAX_MESSAGE_BYTES: usize = 1 + MAX_ADDRESS_BYTES;

const ACTIVATE_TAG: u8 = b'A';
const OPEN_TAG: u8 = b'O';

/// The running Atlas queued the request.
pub(super) const ACCEPTED: u8 = b'K';
/// The request was malformed or not an http(s) address; retrying cannot help.
pub(super) const REFUSED: u8 = b'R';
/// The running Atlas is closing; the launch retries until it can own the profile itself.
pub(super) const CLOSING: u8 = b'C';

/// An http(s) address that passed `atlas_core::web_address`, in its normalized form.
#[derive(Clone, Eq, PartialEq)]
pub struct CheckedAddress(String);

impl CheckedAddress {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

/// A request the running Atlas answers with [`REFUSED`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Malformed;

/// What a launch asks the running Atlas to do.
#[derive(Clone, Eq, PartialEq)]
pub enum LinkRequest {
    /// Come to the front; a launch without a link sends this.
    Activate,
    /// Open this address in a new tab and come to the front.
    Open(CheckedAddress),
}

impl LinkRequest {
    /// Accepts only what navigation would open: an http(s) address with a host and no
    /// credentials. Anything else is refused here, so nothing is ever executed.
    pub fn open(text: &str) -> Result<Self, WebAddressError> {
        let address = web_address(text)?;
        if address.len() > MAX_ADDRESS_BYTES {
            return Err(WebAddressError::Invalid);
        }
        Ok(Self::Open(CheckedAddress(address)))
    }

    pub(super) fn encode(&self) -> Vec<u8> {
        match self {
            Self::Activate => vec![ACTIVATE_TAG],
            Self::Open(address) => {
                let mut message = Vec::with_capacity(1 + address.0.len());
                message.push(OPEN_TAG);
                message.extend_from_slice(address.0.as_bytes());
                message
            }
        }
    }

    /// Reads a request from another process, checking it as if it had been typed.
    pub(super) fn decode(message: &[u8]) -> Result<Self, Malformed> {
        match message.split_first().ok_or(Malformed)? {
            (&ACTIVATE_TAG, []) => Ok(Self::Activate),
            (&OPEN_TAG, address) => {
                let text = std::str::from_utf8(address).map_err(|_| Malformed)?;
                Self::open(text).map_err(|_| Malformed)
            }
            _ => Err(Malformed),
        }
    }
}

impl fmt::Debug for LinkRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A forwarded address is the owner's browsing; diagnostics name only the kind.
        formatter.write_str(match self {
            Self::Activate => "LinkRequest::Activate",
            Self::Open(_) => "LinkRequest::Open(redacted)",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_survive_encoding_and_are_normalized() {
        let open = LinkRequest::open("HTTPS://Example.TEST/Path?q=1").expect("web address");
        assert_eq!(LinkRequest::decode(&open.encode()), Ok(open.clone()));
        let LinkRequest::Open(address) = open else {
            panic!("open request");
        };
        assert_eq!(address.as_str(), "https://example.test/Path?q=1");
        assert_eq!(
            LinkRequest::decode(&LinkRequest::Activate.encode()),
            Ok(LinkRequest::Activate)
        );
    }

    #[test]
    fn anything_but_an_http_address_is_refused_on_both_sides() {
        for text in [
            "javascript:alert(1)",
            "file:///C:/Windows/System32/cmd.exe",
            "ms-settings:privacy",
            "https://name:secret@example.test/",
            "not an address",
            "",
        ] {
            assert!(LinkRequest::open(text).is_err(), "{text}");
            let mut forged = vec![OPEN_TAG];
            forged.extend_from_slice(text.as_bytes());
            assert_eq!(LinkRequest::decode(&forged), Err(Malformed), "{text}");
        }
        for forged in [
            &[][..],
            &[ACTIVATE_TAG, b'x'][..],
            &b"X"[..],
            &[OPEN_TAG, 0xff, 0xfe][..],
        ] {
            assert_eq!(LinkRequest::decode(forged), Err(Malformed));
        }
    }

    #[test]
    fn overlong_addresses_are_refused_not_truncated() {
        let fits = format!(
            "https://example.test/{}",
            "a".repeat(MAX_ADDRESS_BYTES - 21)
        );
        assert_eq!(fits.len(), MAX_ADDRESS_BYTES);
        assert!(LinkRequest::open(&fits).is_ok());
        let over = format!("{fits}a");
        assert_eq!(LinkRequest::open(&over), Err(WebAddressError::Invalid));
    }

    #[test]
    fn diagnostics_never_show_the_address() {
        let request = LinkRequest::open("https://private-sentinel.example.test/").expect("web");
        assert!(!format!("{request:?}").contains("private-sentinel"));
    }
}

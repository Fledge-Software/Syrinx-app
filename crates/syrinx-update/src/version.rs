//! Release versions with one canonical spelling, compared for rollback protection.

use std::fmt;

/// A `MAJOR.MINOR.PATCH` release version; ordering compares major first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parses the canonical form only: three decimal parts, no leading zeros or suffixes.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let major = parse_part(parts.next()?)?;
        let minor = parse_part(parts.next()?)?;
        let patch = parse_part(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self::new(major, minor, patch))
    }
}

/// Parses one canonical decimal number that fits in 32 bits.
pub(crate) fn parse_part(text: &str) -> Option<u32> {
    parse_canonical_decimal(text).and_then(|value| u32::try_from(value).ok())
}

/// Parses decimal digits without sign, leading zeros or overflow.
pub(crate) fn parse_canonical_decimal(text: &str) -> Option<u64> {
    // Bound the length first so the digit loop below cannot overflow silently.
    if text.is_empty() || text.len() > 20 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if text.len() > 1 && text.starts_with('0') {
        return None;
    }
    text.parse().ok()
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

//! UTC instants with one canonical spelling, for the latest statement's issue and expiry times.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::UpdateError;

/// Seconds in one UTC day; the statement format has no leap seconds.
pub const SECONDS_PER_DAY: u64 = 86_400;

const FIRST_YEAR: u64 = 1970;
const LAST_YEAR: u64 = 9999;
/// `9999-12-31T23:59:59Z`, the last instant the four-digit year can spell.
const LAST_SECOND: u64 = 253_402_300_799;
/// `YYYY-MM-DDTHH:MM:SSZ`.
const SPELLING_BYTES: usize = 20;

/// A whole second in UTC between 1970 and 9999, spelled `YYYY-MM-DDTHH:MM:SSZ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    seconds: u64,
}

impl Timestamp {
    /// An instant given as seconds since 1970, if the format can spell it.
    pub fn from_unix_seconds(seconds: u64) -> Option<Self> {
        (seconds <= LAST_SECOND).then_some(Self { seconds })
    }

    pub fn unix_seconds(self) -> u64 {
        self.seconds
    }

    /// The local clock, rounded down to the second.
    pub fn now() -> Result<Self, UpdateError> {
        Self::from_system_time(SystemTime::now())
    }

    /// Refuse a clock set before 1970 or past 9999 instead of guessing a time.
    pub fn from_system_time(time: SystemTime) -> Result<Self, UpdateError> {
        let elapsed = time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| UpdateError::Clock)?;
        Self::from_unix_seconds(elapsed.as_secs()).ok_or(UpdateError::Clock)
    }

    /// This instant moved later, if the result is still spellable.
    pub fn checked_add_seconds(self, seconds: u64) -> Option<Self> {
        self.seconds
            .checked_add(seconds)
            .and_then(Self::from_unix_seconds)
    }

    /// Parse the canonical spelling only: fixed width, upper-case `T` and `Z`, a real date.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != SPELLING_BYTES || !has_separators(bytes) {
            return None;
        }
        let year = digits(&bytes[0..4])?;
        if !(FIRST_YEAR..=LAST_YEAR).contains(&year) {
            return None;
        }
        let month = digits(&bytes[5..7])?;
        let day = digits(&bytes[8..10])?;
        let hour = digits(&bytes[11..13])?;
        let minute = digits(&bytes[14..16])?;
        let second = digits(&bytes[17..19])?;
        if !is_real_date(year, month, day) || hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        let seconds = days_from_civil(year, month, day) * SECONDS_PER_DAY
            + hour * 3600
            + minute * 60
            + second;
        Self::from_unix_seconds(seconds)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = civil_from_days(self.seconds / SECONDS_PER_DAY);
        let within_day = self.seconds % SECONDS_PER_DAY;
        write!(
            formatter,
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            within_day / 3600,
            within_day / 60 % 60,
            within_day % 60
        )
    }
}

/// The fixed punctuation of `YYYY-MM-DDTHH:MM:SSZ`.
fn has_separators(bytes: &[u8]) -> bool {
    bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
}

/// A fixed-width run of ASCII digits; leading zeros are part of the width.
fn digits(bytes: &[u8]) -> Option<u64> {
    bytes.iter().try_fold(0u64, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u64::from(byte - b'0'))
    })
}

fn is_real_date(year: u64, month: u64, day: u64) -> bool {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days_in_month).contains(&day)
}

/// Days since 1970-01-01 for a valid date from 1970 on (Howard Hinnant's civil algorithm).
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date of a day count since 1970-01-01; the inverse of `days_from_civil`.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

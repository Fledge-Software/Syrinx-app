//! Groups counts into periods of at most 30 days and keeps only the current and previous one.

use std::fmt;

use super::recorder::Counts;

/// A period closes on the first start this many days after it began.
pub const PERIOD_DAYS: i32 = 30;

/// A local calendar day, with no time of day; the only date precision usage counting keeps.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UsageDate {
    /// Days since 1970-01-01 in the proleptic Gregorian calendar.
    days: i32,
}

impl UsageDate {
    /// A valid date from 1970 to 9999; anything else is refused.
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Option<Self> {
        if !(1970..=9999).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        if day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Self {
            days: days_from_civil(year, month, day),
        })
    }

    /// Reads `YYYY-MM-DD` exactly.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let number = |range: std::ops::Range<usize>| -> Option<u32> {
            let digits = &text[range];
            digits
                .bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| digits.parse().ok())?
        };
        let year = i32::try_from(number(0..4)?).ok()?;
        Self::from_ymd(year, number(5..7)?, number(8..10)?)
    }

    pub fn ymd(self) -> (i32, u32, u32) {
        civil_from_days(self.days)
    }

    pub fn plus_days(self, days: i32) -> Self {
        Self {
            days: self.days.saturating_add(days),
        }
    }
}

impl fmt::Display for UsageDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.ymd();
        write!(formatter, "{year:04}-{month:02}-{day:02}")
    }
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Howard Hinnant's `days_from_civil`, for years from 1970 on.
fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = month as i32;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of `days_from_civil`.
fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i32::from(month <= 2);
    (year, month, day)
}

/// The counts of one period, from its start day.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Period {
    pub start: UsageDate,
    /// The last day Syrinx started in this period; only this day is kept, never a list.
    pub last_counted: UsageDate,
    /// Days in the period on which Syrinx started at least once.
    pub days_used: u32,
    pub counts: Counts,
}

impl Period {
    /// A new period beginning today, which counts as a day of use.
    pub fn starting(today: UsageDate) -> Self {
        Self {
            start: today,
            last_counted: today,
            days_used: 1,
            counts: Counts::default(),
        }
    }

    /// The last day of the period's 30-day span, the end a completed period reports.
    pub fn span_end(&self) -> UsageDate {
        self.start.plus_days(PERIOD_DAYS - 1)
    }
}

/// Everything `usage.json` holds: the current period and at most one previous one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageFile {
    pub current: Period,
    pub previous: Option<Period>,
}

impl UsageFile {
    /// What switching counting on, or Reset, starts with: one empty period from today.
    pub fn begin(today: UsageDate) -> Self {
        Self {
            current: Period::starting(today),
            previous: None,
        }
    }

    /// Applies one start of Syrinx on `today`: closes a period that has run 30 days, which
    /// drops the period before it, and otherwise counts a new day of use.
    pub fn start_day(&mut self, today: UsageDate) {
        if today >= self.current.start.plus_days(PERIOD_DAYS) {
            let closed = std::mem::replace(&mut self.current, Period::starting(today));
            self.previous = Some(closed);
            return;
        }
        // A clock set back never counts the same day twice.
        if today > self.current.last_counted {
            self.current.days_used = self.current.days_used.saturating_add(1);
            self.current.last_counted = today;
        }
    }

    /// Adds counts taken from the recorder to the current period.
    pub fn add(&mut self, counts: &Counts) {
        self.current.counts.add(counts);
    }
}

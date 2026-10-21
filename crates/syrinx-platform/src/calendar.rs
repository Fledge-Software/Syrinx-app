//! Reads dates in the owner's own time zone: today's, for the home page header (P1-28) and the
//! day usage counting keeps (AN-01), and a stored backup's, for the recovery screen (#51).

use windows::Win32::Foundation::{E_INVALIDARG, FILETIME, SYSTEMTIME};
use windows::Win32::Globalization::{
    DATE_LONGDATE, ENUM_DATE_FORMATS_FLAGS, GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS,
};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::core::{PCWSTR, w};

/// FILETIME counts 100-nanosecond ticks from 1601-01-01; Unix time counts seconds from 1970.
const TICKS_PER_SECOND: i64 = 10_000_000;
const SECONDS_FROM_1601_TO_1970: i64 = 11_644_473_600;

/// Today's local date in the canvas's form, such as "Friday, October 9", with the weekday and
/// month names in the owner's Windows display language.
///
/// Windows supplies the local calendar date; Rust's standard library knows only UTC, which would
/// show tomorrow's date every evening west of Greenwich.
pub fn local_date_heading() -> windows::core::Result<String> {
    // A null locale name means the owner's default.
    format_date(
        PCWSTR::null(),
        ENUM_DATE_FORMATS_FLAGS(0),
        None,
        w!("dddd, MMMM d"),
    )
}

/// Today's local calendar date as `YYYY-MM-DD`, the day usage counting keeps (AN-01).
pub fn local_iso_date() -> windows::core::Result<String> {
    // The invariant locale (an empty name) always writes Gregorian ASCII digits.
    format_date(
        w!(""),
        ENUM_DATE_FORMATS_FLAGS(0),
        None,
        w!("yyyy'-'MM'-'dd"),
    )
}

/// A Unix timestamp as the owner's long local date and short time, such as
/// "Tuesday, October 20, 2026, 3:04 PM", in their Windows language and time zone.
pub fn local_date_time(unix_seconds: i64) -> windows::core::Result<String> {
    let local = local_system_time(unix_seconds)?;
    // A null locale name means the owner's default; the flag picks its long date form.
    let date = format_date(PCWSTR::null(), DATE_LONGDATE, Some(&local), PCWSTR::null())?;
    let time = format_time(&local)?;
    Ok(format!("{date}, {time}"))
}

/// Converts a Unix timestamp to local wall-clock time, with the daylight rule of that date.
fn local_system_time(unix_seconds: i64) -> windows::core::Result<SYSTEMTIME> {
    let Some(ticks) = filetime_ticks(unix_seconds) else {
        return Err(windows::core::Error::from_hresult(E_INVALIDARG));
    };
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut universal = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY: every pointer names a live local of the type Windows expects.
    unsafe {
        FileTimeToSystemTime(&file_time, &mut universal)?;
        // No zone argument means the owner's current zone, with that date's daylight rule.
        SystemTimeToTzSpecificLocalTime(None, &universal, &mut local)?;
    }
    Ok(local)
}

/// A Unix timestamp as FILETIME ticks, or `None` before 1601 or past FILETIME's range.
fn filetime_ticks(unix_seconds: i64) -> Option<u64> {
    let ticks = unix_seconds
        .checked_add(SECONDS_FROM_1601_TO_1970)?
        .checked_mul(TICKS_PER_SECOND)?;
    u64::try_from(ticks).ok()
}

/// Formats a date; no date argument means today in local time.
fn format_date(
    locale: PCWSTR,
    flags: ENUM_DATE_FORMATS_FLAGS,
    date: Option<&SYSTEMTIME>,
    picture: PCWSTR,
) -> windows::core::Result<String> {
    let mut buffer = [0u16; 128];
    // SAFETY: the picture is null or a static wide string, and the buffer's length is passed.
    let written = unsafe {
        GetDateFormatEx(
            locale,
            flags,
            date.map(|date| date as *const SYSTEMTIME),
            picture,
            Some(&mut buffer),
            None,
        )
    };
    read_formatted(&buffer, written)
}

/// Formats a local time in the owner's short form, such as "3:04 PM".
fn format_time(time: &SYSTEMTIME) -> windows::core::Result<String> {
    let mut buffer = [0u16; 64];
    // SAFETY: the time is a live local and the buffer's length is passed with it.
    let written = unsafe {
        GetTimeFormatEx(
            PCWSTR::null(),
            TIME_NOSECONDS,
            Some(time as *const SYSTEMTIME),
            PCWSTR::null(),
            Some(&mut buffer),
        )
    };
    read_formatted(&buffer, written)
}

/// Reads a formatting call's text, or the thread's error when it wrote nothing.
fn read_formatted(buffer: &[u16], written: i32) -> windows::core::Result<String> {
    if written <= 0 {
        return Err(windows::core::Error::from_thread());
    }
    // Success writes a terminating null, which the text leaves out.
    let length = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..length]))
}

#[cfg(test)]
mod tests {
    use super::{filetime_ticks, local_date_heading, local_date_time, local_iso_date};

    #[test]
    fn the_usage_date_is_iso_digits() {
        let date = local_iso_date().expect("Windows formats the local date");
        let bytes = date.as_bytes();
        assert_eq!(bytes.len(), 10, "{date}");
        assert_eq!((bytes[4], bytes[7]), (b'-', b'-'), "{date}");
        assert!(
            syrinx_core::usage::UsageDate::parse(&date).is_some(),
            "{date}"
        );
    }

    #[test]
    fn the_heading_names_a_weekday_and_day_of_month() {
        let heading = local_date_heading().expect("Windows formats the local date");
        assert!(heading.contains(", "), "{heading}");
        assert!(
            heading
                .chars()
                .last()
                .is_some_and(|character| character.is_numeric()),
            "{heading}"
        );
        assert!(!heading.contains('\0'));
    }

    #[test]
    fn unix_time_maps_onto_filetime_ticks() {
        assert_eq!(filetime_ticks(0), Some(116_444_736_000_000_000));
        assert_eq!(filetime_ticks(1), Some(116_444_736_010_000_000));
        assert_eq!(filetime_ticks(-11_644_473_600), Some(0));
        assert_eq!(filetime_ticks(-11_644_473_601), None);
        assert_eq!(filetime_ticks(i64::MAX), None);
    }

    #[test]
    fn a_stored_time_formats_as_a_local_date_and_time() {
        // The owner's calendar may not be Gregorian, so only the shape is checked: a date, a
        // separator, a time, and an hour later on the same day reads differently.
        let noon = local_date_time(1_784_116_800).expect("Windows formats the stored time");
        let one = local_date_time(1_784_120_400).expect("Windows formats the stored time");
        let (date, time) = noon.rsplit_once(", ").expect("date and time are joined");
        assert!(!date.is_empty() && !time.is_empty(), "{noon}");
        assert_ne!(noon, one);
        assert!(!noon.contains('\0'));
    }

    #[test]
    fn a_time_before_1601_is_refused_rather_than_wrapped() {
        assert!(local_date_time(-20_000_000_000).is_err());
    }
}

//! Reads today's date in the owner's own time zone, for the home page header (P1-28) and the
//! day usage counting keeps (AN-01).

use windows::Win32::Globalization::{ENUM_DATE_FORMATS_FLAGS, GetDateFormatEx};
use windows::core::{PCWSTR, w};

/// Today's local date in the canvas's form, such as "Friday, October 9", with the weekday and
/// month names in the owner's Windows display language.
///
/// Windows supplies the local calendar date; Rust's standard library knows only UTC, which would
/// show tomorrow's date every evening west of Greenwich.
pub fn local_date_heading() -> windows::core::Result<String> {
    // A null locale name means the owner's default.
    format_today(PCWSTR::null(), w!("dddd, MMMM d"))
}

/// Today's local calendar date as `YYYY-MM-DD`, the day usage counting keeps (AN-01).
pub fn local_iso_date() -> windows::core::Result<String> {
    // The invariant locale (an empty name) always writes Gregorian ASCII digits.
    format_today(w!(""), w!("yyyy'-'MM'-'dd"))
}

/// Formats today's local date; no date argument means today in local time.
fn format_today(locale: PCWSTR, picture: PCWSTR) -> windows::core::Result<String> {
    let mut buffer = [0u16; 128];
    // SAFETY: the picture is a static wide string and the buffer's length is passed with it.
    let written = unsafe {
        GetDateFormatEx(
            locale,
            ENUM_DATE_FORMATS_FLAGS(0),
            None,
            picture,
            Some(&mut buffer),
            None,
        )
    };
    if written <= 0 {
        return Err(windows::core::Error::from_thread());
    }
    // Success writes a terminating null, which the heading leaves out.
    let length = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..length]))
}

#[cfg(test)]
mod tests {
    use super::{local_date_heading, local_iso_date};

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
}

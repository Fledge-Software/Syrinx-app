//! Reads dates in the owner's own time zone: today's, for the home page header (P1-28) and the
//! day usage counting keeps (AN-01), and a stored backup's, for the recovery screen (#51).

use std::{ffi::CStr, io, mem::MaybeUninit, sync::Once};

/// Today's local date in the canvas's form, such as "Friday, October 9", in the owner's language.
pub fn local_date_heading() -> io::Result<String> {
    format_local(now()?, c"%A, %B %-d")
}

/// Today's local calendar date as `YYYY-MM-DD`, the day usage counting keeps (AN-01).
pub fn local_iso_date() -> io::Result<String> {
    // Numeric fields only, so the owner's language never changes the digits.
    format_local(now()?, c"%Y-%m-%d")
}

/// A Unix timestamp as the owner's long local date and short time, such as
/// "Tuesday, October 20, 2026, 3:04 PM".
pub fn local_date_time(unix_seconds: i64) -> io::Result<String> {
    let time = libc::time_t::try_from(unix_seconds)
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let date = format_local(time, c"%A, %B %-d, %Y")?;
    let clock = format_local(time, c"%-I:%M %p")?;
    Ok(format!("{date}, {clock}"))
}

fn now() -> io::Result<libc::time_t> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?
        .as_secs();
    libc::time_t::try_from(seconds).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
}

/// Formats `time` in local time, with that date's daylight rule, using the owner's locale.
fn format_local(time: libc::time_t, picture: &CStr) -> io::Result<String> {
    adopt_time_locale();
    let mut local = MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both pointers name live locals of the type the C library expects.
    if unsafe { libc::localtime_r(&time, local.as_mut_ptr()) }.is_null() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let local = unsafe { local.assume_init() };
    let mut buffer = [0u8; 128];
    // SAFETY: the buffer's length is passed and the picture is a terminated constant.
    let written = unsafe {
        libc::strftime(
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            picture.as_ptr(),
            &local,
        )
    };
    if written == 0 {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok(String::from_utf8_lossy(&buffer[..written]).into_owned())
}

/// Rust starts in the C locale; month and day names should follow the owner's language.
fn adopt_time_locale() {
    static ADOPTED: Once = Once::new();
    ADOPTED.call_once(|| {
        // SAFETY: an empty name selects the environment's locale; only the time category changes.
        if unsafe { libc::setlocale(libc::LC_TIME, c"".as_ptr()) }.is_null() {
            eprintln!("Syrinx could not read the owner's date language; it uses English names.");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todays_iso_date_has_the_documented_shape() {
        let date = local_iso_date().expect("today's date");
        let parts: Vec<_> = date.split('-').collect();
        assert_eq!(parts.len(), 3, "{date}");
        assert_eq!(parts[0].len(), 4);
        assert!(
            parts
                .iter()
                .all(|part| part.chars().all(|digit| digit.is_ascii_digit()))
        );
    }

    #[test]
    fn a_stored_time_shows_a_date_and_a_clock() {
        let noon = local_date_time(1_792_497_600).expect("formats the stored time");
        let (date, clock) = noon.rsplit_once(", ").expect("date and time are joined");
        assert!(!date.is_empty() && !clock.is_empty(), "{noon}");
        assert!(!local_date_heading().expect("heading").is_empty());
    }
}

//! Deletes a passing smoke run's synthetic profile and keeps a failing run's as evidence (#86).

use std::{
    io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Every fresh synthetic profile is `<temp>\syrinx-window-smoke-<pid>-<nanos>`.
const PREFIX: &str = "syrinx-window-smoke-";

/// WebView2's child processes can still hold profile files for a moment after shutdown.
const REMOVAL_ATTEMPTS: u32 = 50;
const REMOVAL_PAUSE: Duration = Duration::from_millis(200);

/// Names a fresh synthetic profile folder in the temp directory; nothing is created yet.
pub(crate) fn fresh_directory() -> Result<PathBuf, &'static str> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "The system clock is unavailable.")?
        .as_nanos();
    Ok(std::env::temp_dir().join(format!("{PREFIX}{}-{nonce}", std::process::id())))
}

/// A smoke profile folder this process created itself. Only `reserve` makes one, so a
/// `--test-data-dir` folder or the real profile can never become one.
pub(crate) struct GeneratedProfile {
    directory: PathBuf,
    settled: bool,
}

impl GeneratedProfile {
    /// Creates the folder exclusively, refusing one that already exists.
    pub(crate) fn reserve(directory: PathBuf) -> Result<Self, &'static str> {
        crate::reserve_smoke_directory(&directory)?;
        Ok(Self {
            directory,
            settled: false,
        })
    }

    /// Deletes the folder after a passing run; keeps and names it after a failing one.
    /// A deletion failure is a warning, not a failed run (#86).
    pub(crate) fn settle(mut self, passed: bool) {
        self.settled = true;
        if !passed {
            report_kept(&self.directory);
            return;
        }
        // Refuse anything that is not this run's own temp folder, whatever the caller passed.
        if !is_generated_smoke_profile(&self.directory, &std::env::temp_dir()) {
            eprintln!(
                "Warning: refused to delete {}, which is not a generated smoke profile.",
                self.directory.display()
            );
            return;
        }
        let removal = remove_with_retries(
            || std::fs::remove_dir_all(&self.directory),
            REMOVAL_ATTEMPTS,
            REMOVAL_PAUSE,
        );
        if let Err(error) = removal {
            eprintln!(
                "Warning: could not delete the smoke run's synthetic profile {}: {error}",
                self.directory.display()
            );
        }
    }
}

impl Drop for GeneratedProfile {
    /// A run that ends before `settle` (an early error or a panic) failed, so keep the evidence.
    fn drop(&mut self) {
        if !self.settled {
            report_kept(&self.directory);
        }
    }
}

fn report_kept(directory: &Path) {
    eprintln!(
        "The smoke run failed; its synthetic profile is kept for diagnosis at {}",
        directory.display()
    );
}

/// True only for `<temp>\syrinx-window-smoke-<digits>-<digits>`, directly inside `temp`.
fn is_generated_smoke_profile(directory: &Path, temp: &Path) -> bool {
    if directory.parent() != Some(temp) {
        return false;
    }
    let Some(name) = directory.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some((process, nonce)) = name
        .strip_prefix(PREFIX)
        .and_then(|rest| rest.split_once('-'))
    else {
        return false;
    };
    is_number(process) && is_number(nonce)
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// Retries a removal that WebView2's lingering file handles make fail, then reports the last
/// error. A folder that is already gone counts as removed.
fn remove_with_retries(
    mut remove: impl FnMut() -> io::Result<()>,
    attempts: u32,
    pause: Duration,
) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        let error = match remove() {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => error,
        };
        if attempt >= attempts {
            return Err(error);
        }
        thread::sleep(pause);
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Windows' ERROR_SHARING_VIOLATION, which a file WebView2 still holds produces.
    const SHARING_VIOLATION: i32 = 32;

    fn reserve_synthetic_profile() -> GeneratedProfile {
        let directory = fresh_directory().expect("name a synthetic profile");
        let profile = GeneratedProfile::reserve(directory).expect("reserve synthetic profile");
        let nested = profile.directory.join("EBWebView").join("Default");
        fs::create_dir_all(&nested).expect("create synthetic web view folders");
        fs::write(nested.join("synthetic.txt"), b"synthetic").expect("write synthetic file");
        profile
    }

    #[test]
    fn a_passing_run_deletes_its_generated_profile() {
        let profile = reserve_synthetic_profile();
        let directory = profile.directory.clone();
        profile.settle(true);
        assert!(!directory.exists(), "{} survived", directory.display());
    }

    #[test]
    fn a_failing_run_keeps_its_generated_profile() {
        let profile = reserve_synthetic_profile();
        let directory = profile.directory.clone();
        profile.settle(false);
        assert!(directory.join("EBWebView").exists());
        // Only the reservation above made this test-owned folder.
        fs::remove_dir_all(directory).expect("remove test-owned profile");
    }

    #[test]
    fn only_a_generated_temp_folder_may_be_deleted() {
        let temp = Path::new(r"C:\Users\synthetic\AppData\Local\Temp");
        assert!(is_generated_smoke_profile(
            &temp.join("syrinx-window-smoke-1234-5678"),
            temp
        ));
        for refused in [
            // Caller-supplied test profiles and the real profiles.
            PathBuf::from(r"C:\Synthetic"),
            PathBuf::from(r"C:\Users\synthetic\AppData\Local\SyrinxProductPreview"),
            PathBuf::from(r"C:\Users\synthetic\AppData\Local\Syrinx\data"),
            // The temp folder itself, something inside a profile, and look-alikes.
            temp.to_path_buf(),
            temp.join("syrinx-window-smoke-1234-5678").join("EBWebView"),
            temp.join("syrinx-window-smoke-1234"),
            temp.join("syrinx-window-smoke-12a4-5678"),
            temp.join("syrinx-window-smoke--5678"),
            temp.join("syrinx-smoke-reservation-test-1234-5678"),
            PathBuf::from(r"C:\Synthetic\syrinx-window-smoke-1234-5678"),
        ] {
            assert!(
                !is_generated_smoke_profile(&refused, temp),
                "{}",
                refused.display()
            );
        }
    }

    #[test]
    fn a_held_file_is_retried_until_it_is_released() {
        let mut calls = 0;
        let removal = remove_with_retries(
            || {
                calls += 1;
                if calls < 3 {
                    return Err(io::Error::from_raw_os_error(SHARING_VIOLATION));
                }
                Ok(())
            },
            5,
            Duration::ZERO,
        );
        assert!(removal.is_ok());
        assert_eq!(calls, 3);
    }

    #[test]
    fn a_file_held_past_every_attempt_reports_the_error() {
        let mut calls = 0;
        let removal = remove_with_retries(
            || {
                calls += 1;
                Err(io::Error::from_raw_os_error(SHARING_VIOLATION))
            },
            4,
            Duration::ZERO,
        );
        let error = removal.expect_err("held file stays held");
        assert_eq!(error.raw_os_error(), Some(SHARING_VIOLATION));
        assert_eq!(calls, 4);
    }
}

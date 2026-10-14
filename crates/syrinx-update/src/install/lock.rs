//! One lock file shared by Syrinx and its helper, so only one changes update files at a time.
//!
//! Syrinx holds it from just before starting the helper until it exits, which is how the
//! helper learns that the old process has gone without any Windows process API.

use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::thread;
use std::time::{Duration, Instant};

use super::error::io_error;
use super::files::ensure_plain_directory;
use super::{InstallError, UpdateLayout};

/// Windows' "used by another process" error.
const ERROR_SHARING_VIOLATION: i32 = 32;

/// Held while changing update files; released when dropped or when the process ends.
#[derive(Debug)]
pub struct ApplyLock {
    _file: File,
}

impl ApplyLock {
    /// Take the lock, or `None` while someone else holds it.
    pub fn try_acquire(layout: &UpdateLayout) -> Result<Option<Self>, InstallError> {
        ensure_plain_directory(&layout.updates(), "the Updates folder")?;
        let opened = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(layout.lock());
        match opened {
            Ok(file) => Ok(Some(Self { _file: file })),
            Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => Ok(None),
            Err(error) => Err(io_error("taking the update lock")(error)),
        }
    }

    /// Wait up to `patience` for the lock, checking every `poll`.
    pub fn wait(
        layout: &UpdateLayout,
        patience: Duration,
        poll: Duration,
    ) -> Result<Self, InstallError> {
        let deadline = Instant::now() + patience;
        loop {
            if let Some(lock) = Self::try_acquire(layout)? {
                return Ok(lock);
            }
            if Instant::now() >= deadline {
                return Err(InstallError::TimedOut {
                    what: "Syrinx to exit",
                });
            }
            thread::sleep(poll);
        }
    }
}

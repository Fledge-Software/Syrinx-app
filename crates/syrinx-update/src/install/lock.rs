//! One lock file shared by Syrinx and its helper, so only one changes update files at a time.
//!
//! Syrinx holds it from just before starting the helper until it exits, which is how the
//! helper learns that the old process has gone without any Windows process API.

use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::thread;
use std::time::{Duration, Instant};

use super::error::io_error;
use super::files::ensure_plain_directory;
use super::{InstallError, UpdateLayout};

/// Windows' "used by another process" error.
#[cfg(windows)]
const ERROR_SHARING_VIOLATION: i32 = 32;

/// Held while changing update files; released when dropped or when the process ends.
#[derive(Debug)]
pub struct ApplyLock {
    _file: File,
}

impl ApplyLock {
    /// Take the lock, or `None` while someone else holds it.
    #[cfg(windows)]
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

    /// Take the lock, or `None` while someone else holds it.
    ///
    /// Unix has no exclusive open, so the file is locked with `flock`, which the kernel also
    /// releases when the holding process ends.
    #[cfg(unix)]
    pub fn try_acquire(layout: &UpdateLayout) -> Result<Option<Self>, InstallError> {
        ensure_plain_directory(&layout.updates(), "the Updates folder")?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(layout.lock())
            .map_err(io_error("taking the update lock"))?;
        // A child this process is starting shares a just-released lock until it execs, so a
        // refusal is retried briefly; Syrinx or the helper holds it for far longer than this.
        for attempt in 0..=UNIX_LOCK_RETRIES {
            if try_flock(&file)? {
                return Ok(Some(Self { _file: file }));
            }
            if attempt < UNIX_LOCK_RETRIES {
                thread::sleep(UNIX_LOCK_RETRY_POLL);
            }
        }
        Ok(None)
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

/// How often, and how far apart, a refused Unix lock is retried before it counts as held.
#[cfg(unix)]
const UNIX_LOCK_RETRIES: u32 = 10;
#[cfg(unix)]
const UNIX_LOCK_RETRY_POLL: Duration = Duration::from_millis(10);

/// Take `flock` without waiting: `true` once held, `false` while another holder has it.
#[cfg(unix)]
fn try_flock(file: &File) -> Result<bool, InstallError> {
    // SAFETY: the descriptor belongs to `file`, which outlives this call.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if locked == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        return Ok(false);
    }
    Err(io_error("taking the update lock")(error))
}

//! Identifies this Unix user and holds the profile's instance lock as an advisory file lock.

use std::{
    fs::{File, OpenOptions},
    os::unix::{fs::OpenOptionsExt, io::AsRawFd},
};

use super::InstanceError;

/// The signed-in user, the only one allowed to reach the link socket.
pub(super) struct OwnerOnly {
    uid: libc::uid_t,
    id: String,
}

impl OwnerOnly {
    pub(super) fn current_user() -> Result<Self, InstanceError> {
        let uid = unsafe { libc::getuid() };
        Ok(Self {
            uid,
            id: uid.to_string(),
        })
    }

    pub(super) fn sid(&self) -> &str {
        &self.id
    }

    /// Unix has no logon sessions in the Windows sense; the runtime folder is already per user.
    pub(super) fn session(&self) -> u32 {
        0
    }

    pub(super) fn uid(&self) -> libc::uid_t {
        self.uid
    }
}

/// The profile's instance lock: held by whichever process owns the profile.
pub(super) struct NamedMutex {
    file: File,
    held: bool,
}

impl NamedMutex {
    pub(super) fn open(name: &str, _owner: &OwnerOnly) -> Result<Self, InstanceError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(name)
            .map_err(|_| InstanceError::Mutex)?;
        Ok(Self { file, held: false })
    }

    pub(super) fn try_acquire(&mut self) -> Result<bool, InstanceError> {
        if self.held {
            return Ok(true);
        }
        if unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            self.held = true;
            return Ok(true);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EWOULDBLOCK) => Ok(false),
            _ => Err(InstanceError::Mutex),
        }
    }

    pub(super) fn release(&mut self) {
        if !self.held {
            return;
        }
        // Closing the file also releases the lock, so a failed unlock is reported, not fatal.
        if unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) } != 0 {
            eprintln!("Syrinx could not release its instance lock early; it ends at exit.");
        }
        self.held = false;
    }
}

impl Drop for NamedMutex {
    fn drop(&mut self) {
        self.release();
    }
}

//! Restricts the instance mutex and link pipe to the signed-in user, and owns their handles.

use windows::{
    Win32::{
        Foundation::{
            CloseHandle, HANDLE, HLOCAL, LocalFree, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
            TOKEN_USER, TokenSessionId, TokenUser,
        },
        System::Threading::{
            CreateMutexW, GetCurrentProcess, OpenProcess, OpenProcessToken,
            PROCESS_QUERY_LIMITED_INFORMATION, ReleaseMutex, WaitForSingleObject,
        },
    },
    core::{HSTRING, PWSTR},
};

use super::InstanceError;

/// A kernel handle closed exactly once.
pub(super) struct OwnedHandle(pub(super) HANDLE);

// Kernel handles are process-wide and may be used and closed from any thread.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            // Drop cannot return the failure, and a leaked handle is not owner data.
            eprintln!("Syrinx could not close an instance handle: {error}");
        }
    }
}

/// A security descriptor that grants access to the current user alone.
pub(super) struct OwnerOnly {
    sid: String,
    session: u32,
    descriptor: PSECURITY_DESCRIPTOR,
}

impl OwnerOnly {
    pub(super) fn current_user() -> Result<Self, InstanceError> {
        let token = process_token(unsafe { GetCurrentProcess() })?;
        let sid = token_user_sid(&token)?;
        let session = token_session(&token)?;
        // A protected DACL with one entry: full access for this user, nothing inherited.
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{sid})"));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|_| InstanceError::Security)?;
        Ok(Self {
            sid,
            session,
            descriptor,
        })
    }

    pub(super) fn sid(&self) -> &str {
        &self.sid
    }

    /// The Windows session this process runs in, so two sign-ins of one user stay apart.
    pub(super) fn session(&self) -> u32 {
        self.session
    }

    /// Whether `process` runs as this user; anything that cannot be checked counts as foreign.
    pub(super) fn owns_process(&self, process: u32) -> bool {
        let Ok(handle) =
            (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) })
        else {
            return false;
        };
        let handle = OwnedHandle(handle);
        match process_token(handle.0).and_then(|token| token_user_sid(&token)) {
            Ok(sid) => same_user(&self.sid, &sid),
            Err(_) => false,
        }
    }

    /// Attributes for a new object; they borrow this descriptor and must not outlive it.
    pub(super) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for OwnerOnly {
    fn drop(&mut self) {
        free_local(self.descriptor.0);
    }
}

/// The profile's named mutex; whichever process owns it is the running Syrinx.
pub(super) struct NamedMutex {
    handle: OwnedHandle,
    owned: bool,
}

impl NamedMutex {
    pub(super) fn open(name: &str, owner: &OwnerOnly) -> Result<Self, InstanceError> {
        let attributes = owner.attributes();
        let handle = unsafe { CreateMutexW(Some(&attributes), false, &HSTRING::from(name)) }
            .map_err(|_| InstanceError::Mutex)?;
        Ok(Self {
            handle: OwnedHandle(handle),
            owned: false,
        })
    }

    /// Takes ownership without waiting. A mutex left by a Syrinx that exited without
    /// releasing it is abandoned, and taking it over is exactly what a successor should do.
    pub(super) fn try_acquire(&mut self) -> Result<bool, InstanceError> {
        let waited = unsafe { WaitForSingleObject(self.handle.0, 0) };
        if waited == WAIT_OBJECT_0 || waited == WAIT_ABANDONED {
            self.owned = true;
            return Ok(true);
        }
        if waited == WAIT_TIMEOUT {
            return Ok(false);
        }
        Err(InstanceError::Mutex)
    }

    /// Releases ownership once; from another thread Windows refuses and the mutex is abandoned.
    pub(super) fn release(&mut self) {
        if !self.owned {
            return;
        }
        self.owned = false;
        if let Err(error) = unsafe { ReleaseMutex(self.handle.0) } {
            eprintln!("Syrinx could not release its instance mutex: {error}");
        }
    }
}

impl Drop for NamedMutex {
    fn drop(&mut self) {
        self.release();
    }
}

/// Opens a process's access token for reading its user and session.
fn process_token(process: HANDLE) -> Result<OwnedHandle, InstanceError> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }
        .map_err(|_| InstanceError::Identity)?;
    Ok(OwnedHandle(token))
}

/// The token's user SID in string form, as `S-1-5-21-…`.
fn token_user_sid(token: &OwnedHandle) -> Result<String, InstanceError> {
    // The first call fails by design and reports the size of the user record.
    let mut length = 0u32;
    let sizing = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut length) };
    if sizing.is_ok() || (length as usize) < size_of::<TOKEN_USER>() {
        return Err(InstanceError::Identity);
    }
    // Back the record with u64s so the TOKEN_USER read below is aligned.
    let mut record = vec![0u64; (length as usize).div_ceil(size_of::<u64>())];
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(record.as_mut_ptr().cast()),
            length,
            &mut length,
        )
    }
    .map_err(|_| InstanceError::Identity)?;
    let user = unsafe { &*record.as_ptr().cast::<TOKEN_USER>() };

    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }
        .map_err(|_| InstanceError::Identity)?;
    let sid = unsafe { text.to_string() }.map_err(|_| InstanceError::Identity);
    free_local(text.0.cast());
    sid
}

fn token_session(token: &OwnedHandle) -> Result<u32, InstanceError> {
    let mut session = 0u32;
    let mut length = 0u32;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenSessionId,
            Some((&raw mut session).cast()),
            size_of::<u32>() as u32,
            &mut length,
        )
    }
    .map_err(|_| InstanceError::Identity)?;
    Ok(session)
}

/// SID strings compare without regard to case; an empty SID never matches.
pub(super) fn same_user(expected: &str, actual: &str) -> bool {
    !expected.is_empty() && expected.eq_ignore_ascii_case(actual)
}

/// Frees memory Windows allocated for a conversion result.
fn free_local(memory: *mut core::ffi::c_void) {
    if memory.is_null() {
        return;
    }
    let left = unsafe { LocalFree(Some(HLOCAL(memory))) };
    if !left.0.is_null() {
        eprintln!("Syrinx could not free a Windows security buffer.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_user_has_a_sid_and_an_owner_only_descriptor() {
        let owner = OwnerOnly::current_user().expect("owner-only descriptor");
        assert!(owner.sid().starts_with("S-1-"), "{}", owner.sid());
        assert!(!owner.attributes().lpSecurityDescriptor.is_null());
        assert!(
            owner.owns_process(std::process::id()),
            "this process runs as this user"
        );
    }

    #[test]
    fn only_the_same_user_sid_matches() {
        assert!(same_user("S-1-5-21-1-2-3-1001", "S-1-5-21-1-2-3-1001"));
        assert!(same_user("S-1-5-21-1-2-3-1001", "s-1-5-21-1-2-3-1001"));
        for other in [
            "S-1-5-21-1-2-3-1002",
            "S-1-5-18",
            "S-1-5-21-1-2-3-10011",
            "",
        ] {
            assert!(!same_user("S-1-5-21-1-2-3-1001", other), "{other}");
        }
        assert!(!same_user("", ""));
    }

    #[test]
    fn a_process_that_cannot_be_inspected_is_foreign() {
        let owner = OwnerOnly::current_user().expect("owner-only descriptor");
        // Process 0 is the System Idle Process; ordinary users cannot open its token.
        assert!(!owner.owns_process(0));
        assert!(!owner.owns_process(u32::MAX));
    }
}

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
            TOKEN_USER, TokenUser,
        },
        System::Threading::{
            CreateMutexW, GetCurrentProcess, OpenProcessToken, ReleaseMutex, WaitForSingleObject,
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
            eprintln!("Atlas could not close an instance handle: {error}");
        }
    }
}

/// A security descriptor that grants access to the current user alone.
pub(super) struct OwnerOnly {
    sid: String,
    descriptor: PSECURITY_DESCRIPTOR,
}

impl OwnerOnly {
    pub(super) fn current_user() -> Result<Self, InstanceError> {
        let sid = current_user_sid()?;
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
        Ok(Self { sid, descriptor })
    }

    pub(super) fn sid(&self) -> &str {
        &self.sid
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

/// The profile's named mutex; whichever process owns it is the running Atlas.
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

    /// Takes ownership without waiting. A mutex left by an Atlas that exited without
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
            eprintln!("Atlas could not release its instance mutex: {error}");
        }
    }
}

impl Drop for NamedMutex {
    fn drop(&mut self) {
        self.release();
    }
}

/// The current user's SID in string form, as `S-1-5-21-…`.
fn current_user_sid() -> Result<String, InstanceError> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|_| InstanceError::Identity)?;
    let token = OwnedHandle(token);

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

/// Frees memory Windows allocated for a conversion result.
fn free_local(memory: *mut core::ffi::c_void) {
    if memory.is_null() {
        return;
    }
    let left = unsafe { LocalFree(Some(HLOCAL(memory))) };
    if !left.0.is_null() {
        eprintln!("Atlas could not free a Windows security buffer.");
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
    }
}

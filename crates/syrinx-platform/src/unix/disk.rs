//! Reports a volume's free space, so an update refuses to start without room (P1-39b).

use std::{ffi::CString, io, mem::MaybeUninit, os::unix::ffi::OsStrExt, path::Path};

/// Bytes the current user may still write on the volume holding `folder`, which must exist.
pub fn free_bytes(folder: &Path) -> io::Result<u64> {
    let name = CString::new(folder.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut status = MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(name.as_ptr(), status.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let status = unsafe { status.assume_init() };
    // f_bavail counts blocks free to unprivileged users, in units of f_frsize.
    Ok(widen(status.f_bavail).saturating_mul(widen(status.f_frsize)))
}

/// Block counts are 32-bit on some Unixes and 64-bit on others.
fn widen<Count: Into<u64>>(count: Count) -> u64 {
    count.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_space_on_an_existing_folder_and_refuses_a_missing_one() {
        let folder = std::env::temp_dir();
        assert!(free_bytes(&folder).expect("temporary folder's volume") > 0);
        let missing = folder.join("syrinx-free-space-test-missing-folder-7f3a");
        assert!(free_bytes(&missing).is_err());
    }
}

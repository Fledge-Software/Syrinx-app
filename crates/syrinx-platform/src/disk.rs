//! Reports a volume's free space, so an update refuses to start without room (P1-39b).

use std::io;
use std::path::Path;

use windows::{Win32::Storage::FileSystem::GetDiskFreeSpaceExW, core::HSTRING};

/// Bytes the current user may still write on the volume holding `folder`, which must exist.
pub fn free_bytes(folder: &Path) -> io::Result<u64> {
    let mut available = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(folder), Some(&mut available), None, None) }
        // A Win32 failure arrives as an HRESULT whose low word is the Windows error code.
        .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
    Ok(available)
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

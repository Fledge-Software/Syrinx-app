//! Queries Windows filesystem and registered sync-root metadata without reading content.

mod maintenance;

pub(super) use maintenance::{
    create_movable, open_movable, open_readonly, remove_checked_directory, remove_checked_entry,
    remove_checked_file, rename_no_replace, rename_replacing,
};

use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::AsRawHandle,
        io::FromRawHandle,
    },
    path::{Path, PathBuf},
};

use syrinx_store::{LocationError, LocationOperation};
use windows::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{
            ERROR_FILE_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND,
            ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE, HANDLE, OBJ_CASE_INSENSITIVE,
            OBJ_DONT_REPARSE, RtlNtStatusToDosError, STATUS_SUCCESS, UNICODE_STRING, WIN32_ERROR,
        },
        Storage::FileSystem::{
            CREATE_NEW, CreateFileW, FILE_ACCESS_RIGHTS, FILE_ATTRIBUTE_NORMAL,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES, FILE_ID_INFO,
            FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED,
            FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_MODE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileAttributeTagInfo, FileIdInfo,
            FileStandardInfo, GETFINALPATHNAMEBYHANDLE_FLAGS, GetDriveTypeW,
            GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_ALWAYS, OPEN_EXISTING,
            SYNCHRONIZE, VOLUME_NAME_DOS,
        },
        System::{
            Com::CoTaskMemFree,
            IO::IO_STATUS_BLOCK,
            Registry::{
                HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ, RegCloseKey,
                RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW,
            },
            WindowsProgramming::FILE_CREATED,
        },
        UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
    },
    core::{HRESULT, HSTRING, PCWSTR, PWSTR},
};

use super::{EntryInfo, FileIdentity, LocationProbe};

pub(super) struct WindowsProbe;

impl LocationProbe for WindowsProbe {
    fn drive_type(&self, root: &Path) -> Result<u32, LocationError> {
        // The caller supplies an absolute root, including its trailing separator.
        let wide = wide_path(root);
        Ok(unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) })
    }

    fn sync_roots(&self) -> Result<Vec<PathBuf>, LocationError> {
        registered_roots()
    }
}

pub(super) fn local_data_directory() -> Result<PathBuf, LocationError> {
    // Own the returned allocation before converting its UTF-16 contents.
    let allocation = TaskMemory(unsafe {
        SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None)
            .map_err(|error| os_error(LocationOperation::DiscoverLocalData, error))?
    });
    if allocation.0.is_null() {
        return Err(LocationError::InvalidPath);
    }
    let local = unsafe { OsString::from_wide(allocation.0.as_wide()) };
    Ok(PathBuf::from(local).join("Syrinx").join("data"))
}

pub(super) fn open_directory(path: &Path, follow_link: bool) -> Result<File, LocationError> {
    // Metadata checks remain compatible with existing retained handles.
    let flags = if follow_link {
        FILE_FLAG_BACKUP_SEMANTICS
    } else {
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT
    };
    open_existing(path, flags).map_err(|error| os_error(LocationOperation::OpenDirectory, error))
}

pub(super) fn open_retained_directory(path: &Path) -> Result<File, LocationError> {
    // Attribute-only probes do not participate in Windows' delete-sharing checks.
    open_with_access(
        path,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY,
    )
    .map_err(|error| os_error(LocationOperation::OpenDirectory, error))
}

pub(super) fn create_directory(parent: &File, component: &OsStr) -> Result<File, LocationError> {
    // Atomic creation retains the new child itself, without reopening its mutable name.
    super::validate_component(component)?;
    let mut wide: Vec<_> = component.encode_wide().collect();
    let bytes = wide
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or(LocationError::PathTooLong)?;
    let bytes = u16::try_from(bytes).map_err(|_| LocationError::PathTooLong)?;
    let name = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(wide.as_mut_ptr()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: file_handle(parent),
        ObjectName: &name,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..OBJECT_ATTRIBUTES::default()
    };
    let mut handle = HANDLE::default();
    let mut completion = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | SYNCHRONIZE,
            &attributes,
            &mut completion,
            None,
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_CREATE,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if status.0 < 0 {
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(os_error(
            LocationOperation::CreateDirectory,
            windows::core::Error::from_hresult(HRESULT::from_win32(code)),
        ));
    }
    if handle.is_invalid() {
        return Err(LocationError::IdentityChanged);
    }
    let file = unsafe { File::from_raw_handle(handle.0) };
    if status != STATUS_SUCCESS
        || unsafe { completion.Anonymous.Status } != STATUS_SUCCESS
        || completion.Information != FILE_CREATED as usize
    {
        return Err(LocationError::IdentityChanged);
    }
    Ok(file)
}

pub(super) fn optional_directory(path: &Path) -> Result<Option<File>, LocationError> {
    // Retain each present directory; absence is allowed only after its parents pass.
    match open_with_access(
        path,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY,
    ) {
        Ok(file) => Ok(Some(file)),
        Err(error)
            if error.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
                || error.code() == HRESULT::from_win32(ERROR_PATH_NOT_FOUND.0) =>
        {
            Ok(None)
        }
        Err(error) => Err(os_error(LocationOperation::OpenDirectory, error)),
    }
}

pub(super) fn optional_file(
    path: &Path,
    operation: LocationOperation,
) -> Result<Option<File>, LocationError> {
    optional_file_with_access(path, operation, FILE_READ_ATTRIBUTES)
}

pub(super) fn optional_retained_file(
    path: &Path,
    operation: LocationOperation,
) -> Result<Option<File>, LocationError> {
    optional_file_with_access(path, operation, FILE_READ_ATTRIBUTES | FILE_READ_DATA)
}

fn optional_file_with_access(
    path: &Path,
    operation: LocationOperation,
    access: FILE_ACCESS_RIGHTS,
) -> Result<Option<File>, LocationError> {
    // Only an absent file is optional; a missing parent or access failure is not.
    match open_with_access(
        path,
        FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
        access,
    ) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0) => Ok(None),
        Err(error) => Err(os_error(operation, error)),
    }
}

pub(super) fn create_database(path: &Path) -> Result<File, LocationError> {
    // CREATE_NEW cannot truncate an existing database, including a racing creator.
    let wide = wide_file_path(path);
    let created = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            CREATE_NEW,
            FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    };
    match created {
        Ok(handle) => Ok(unsafe { File::from_raw_handle(handle.0) }),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_FILE_EXISTS.0) => {
            optional_retained_file(path, LocationOperation::OpenDatabase)?
                .ok_or(LocationError::IdentityChanged)
        }
        Err(error) => Err(os_error(LocationOperation::CreateDatabase, error)),
    }
}

pub(super) fn create_new_file(
    path: &Path,
    operation: LocationOperation,
    shared_write: bool,
) -> Result<File, LocationError> {
    // Fresh outputs must never adopt a pre-existing file after a creation race.
    let share = if shared_write {
        FILE_SHARE_READ | FILE_SHARE_WRITE
    } else {
        FILE_SHARE_READ
    };
    let wide = wide_file_path(path);
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            share,
            None,
            CREATE_NEW,
            FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|error| os_error(operation, error))?;
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

pub(super) fn open_control(path: &Path) -> Result<File, LocationError> {
    control_file(path, false, LocationOperation::OpenControl)
}

pub(super) fn create_control(path: &Path) -> Result<File, LocationError> {
    control_file(path, true, LocationOperation::OpenControl)
}

/// Flushes a directory so a newly created entry's name survives a power loss.
pub(super) fn flush_directory(
    path: &Path,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    let wide = wide_file_path(path);
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|error| os_error(operation, error))?;
    let directory = unsafe { File::from_raw_handle(handle.0) };
    directory
        .sync_all()
        .map_err(|error| io_error(operation, error))
}

/// Opens or creates the deletion journal with the same exclusive gate as the control file.
pub(super) fn open_journal(path: &Path) -> Result<File, LocationError> {
    control_file(path, false, LocationOperation::OpenDeletionJournal)
}

fn control_file(
    path: &Path,
    create_new: bool,
    operation: LocationOperation,
) -> Result<File, LocationError> {
    // No sharing excludes another reader, writer or deleter until this gate closes.
    let wide = wide_file_path(path);
    let opened = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_MODE(0),
            None,
            if create_new { CREATE_NEW } else { OPEN_ALWAYS },
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    };
    match opened {
        Ok(handle) => Ok(unsafe { File::from_raw_handle(handle.0) }),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_SHARING_VIOLATION.0) => {
            Err(LocationError::StorageBusy)
        }
        Err(error) => Err(os_error(operation, error)),
    }
}

pub(super) fn entry_info(file: &File) -> Result<EntryInfo, LocationError> {
    // Pair each information class with its documented Windows structure.
    let attributes: FILE_ATTRIBUTE_TAG_INFO = file_information(file, FileAttributeTagInfo)?;
    let standard: FILE_STANDARD_INFO = file_information(file, FileStandardInfo)?;
    let identity: FILE_ID_INFO = file_information(file, FileIdInfo)?;
    Ok(EntryInfo {
        identity: FileIdentity {
            volume: identity.VolumeSerialNumber,
            id: identity.FileId.Identifier,
        },
        directory: standard.Directory,
        reparse: attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0,
        delete_pending: standard.DeletePending,
        links: standard.NumberOfLinks,
    })
}

pub(super) fn final_path(file: &File) -> Result<PathBuf, LocationError> {
    // Grow a bounded buffer; Windows includes a terminator in its retry length.
    let mut wide = vec![0; 260];
    for _ in 0..3 {
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file_handle(file),
                &mut wide,
                GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0),
            )
        } as usize;
        if length == 0 {
            return Err(io_error(
                LocationOperation::ResolvePath,
                io::Error::last_os_error(),
            ));
        }
        if length < wide.len() {
            return Ok(PathBuf::from(OsString::from_wide(&wide[..length])));
        }
        if length >= 32_768 {
            return Err(LocationError::PathTooLong);
        }
        wide.resize(length + 1, 0);
    }
    Err(LocationError::IdentityChanged)
}

pub(super) fn io_error(operation: LocationOperation, error: io::Error) -> LocationError {
    LocationError::Os {
        operation,
        code: error.raw_os_error(),
    }
}

fn open_existing(path: &Path, flags: FILE_FLAGS_AND_ATTRIBUTES) -> windows::core::Result<File> {
    // Metadata probes can inspect names already retained by an exclusive control handle.
    open_with_access(path, flags, FILE_READ_ATTRIBUTES)
}

fn open_with_access(
    path: &Path,
    flags: FILE_FLAGS_AND_ATTRIBUTES,
    access: FILE_ACCESS_RIGHTS,
) -> windows::core::Result<File> {
    // Data/list access engages sharing checks; metadata-only callers inspect identities.
    let wide = wide_file_path(path);
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )?
    };
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

fn file_information<Information: Default>(
    file: &File,
    class: FILE_INFO_BY_HANDLE_CLASS,
) -> Result<Information, LocationError> {
    let mut information = Information::default();
    // The private callers supply the exact structure required by each class.
    unsafe {
        GetFileInformationByHandleEx(
            file_handle(file),
            class,
            (&mut information as *mut Information).cast(),
            size_of::<Information>() as u32,
        )
    }
    .map_err(|error| os_error(LocationOperation::InspectEntry, error))?;
    Ok(information)
}

fn file_handle(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}

fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn wide_file_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    // Native opens always use the extended spelling, including for short paths.
    // Every caller has already validated a normal absolute DOS path.
    r"\\?\"
        .encode_utf16()
        .chain(path.as_os_str().encode_wide())
        .chain(Some(0))
        .collect()
}

/// Where Windows records each sync provider's registered roots, one subkey per provider.
const SYNC_ROOT_MANAGER: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager";

fn registered_roots() -> Result<Vec<PathBuf>, LocationError> {
    // Read the registrations instead of calling GetCurrentSyncRoots, which crashed the
    // process on Windows Server and on Windows 11 build 26300 (owner, 2026-10-31).
    let Some(manager) = RegistryKey::open(HKEY_LOCAL_MACHINE, SYNC_ROOT_MANAGER)? else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    for provider in manager.subkey_names()? {
        let Some(user_roots) = manager.open_child(&provider, "UserSyncRoots")? else {
            continue;
        };
        paths.extend(user_roots.string_values()?.into_iter().map(PathBuf::from));
    }
    Ok(paths)
}

/// A registry key opened for reading, closed when dropped.
struct RegistryKey(HKEY);

impl RegistryKey {
    /// Opens `parent\path`, or returns `None` when the key does not exist.
    fn open(parent: HKEY, path: &str) -> Result<Option<Self>, LocationError> {
        let mut opened = HKEY::default();
        let status =
            unsafe { RegOpenKeyExW(parent, &HSTRING::from(path), None, KEY_READ, &mut opened) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        registry_status(status)?;
        Ok(Some(Self(opened)))
    }

    fn open_child(&self, name: &OsStr, child: &str) -> Result<Option<Self>, LocationError> {
        let path = Path::new(name).join(child);
        Self::open(self.0, &path.to_string_lossy())
    }

    /// The names of this key's direct subkeys.
    fn subkey_names(&self) -> Result<Vec<OsString>, LocationError> {
        let mut names = Vec::new();
        for index in 0.. {
            // Registry key names are at most 255 characters.
            let mut buffer = [0u16; 256];
            let mut length = buffer.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            registry_status(status)?;
            names.push(OsString::from_wide(&buffer[..length as usize]));
        }
        Ok(names)
    }

    /// The data of every string value; Windows names each root's value by the user's SID.
    fn string_values(&self) -> Result<Vec<OsString>, LocationError> {
        // Every user's roots count: refusing another account's synced folder is harmless.
        let mut values = Vec::new();
        for index in 0.. {
            let mut name = [0u16; 256];
            let mut name_length = name.len() as u32;
            let mut kind = 0u32;
            let mut data = vec![0u8; 2 * 32_768];
            let mut data_length = data.len() as u32;
            let status = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut name_length,
                    None,
                    Some(&mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_length),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            registry_status(status)?;
            if kind == REG_SZ.0 || kind == REG_EXPAND_SZ.0 {
                values.push(registry_string(&data[..data_length as usize]));
            }
        }
        Ok(values)
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // Closing a key opened for reading cannot lose anything, so its status is not needed.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// Decodes registry string data, which may or may not end in a terminating NUL.
fn registry_string(bytes: &[u8]) -> OsString {
    let wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    OsString::from_wide(&wide)
}

fn registry_status(status: WIN32_ERROR) -> Result<(), LocationError> {
    status
        .ok()
        .map_err(|error| os_error(LocationOperation::EnumerateSyncRoots, error))
}

fn os_error(operation: LocationOperation, error: windows::core::Error) -> LocationError {
    LocationError::Os {
        operation,
        code: Some(error.code().0),
    }
}

struct TaskMemory(PWSTR);

impl Drop for TaskMemory {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.0.cast())) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_bytes(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn registry_strings_decode_with_or_without_a_terminating_nul() {
        let path = r"C:\Users\example\Synced Ünïcode";
        let mut terminated = utf16_bytes(path);
        terminated.extend([0, 0, 0, 0]);

        assert_eq!(registry_string(&utf16_bytes(path)), OsString::from(path));
        assert_eq!(registry_string(&terminated), OsString::from(path));
        assert_eq!(registry_string(&[]), OsString::new());
    }

    #[test]
    fn a_missing_key_reads_as_absent_not_as_an_error() {
        let missing = RegistryKey::open(HKEY_LOCAL_MACHINE, r"SOFTWARE\Syrinx-test-absent.invalid");

        assert!(matches!(missing, Ok(None)));
    }

    #[test]
    fn listing_this_machines_sync_roots_returns_instead_of_crashing() {
        // The WinRT lookup this replaced faulted the whole test process on some builds.
        assert!(registered_roots().is_ok());
    }
}

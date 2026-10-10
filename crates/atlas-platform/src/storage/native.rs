//! Queries Windows filesystem and registered sync-root metadata without reading content.

use std::{
    ffi::OsString,
    fs::File,
    io,
    mem::size_of,
    os::windows::{ffi::OsStringExt, io::AsRawHandle, io::FromRawHandle},
    path::{Path, PathBuf},
    thread,
};

use atlas_store::{LocationError, LocationOperation};
use windows::{
    Storage::Provider::StorageProviderSyncRootManager,
    Win32::{
        Foundation::{
            ERROR_FILE_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, GENERIC_READ,
            GENERIC_WRITE, HANDLE,
        },
        Storage::FileSystem::{
            CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES,
            FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileAttributeTagInfo,
            FileIdInfo, FileStandardInfo, GETFINALPATHNAMEBYHANDLE_FLAGS, GetDriveTypeW,
            GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_EXISTING,
            VOLUME_NAME_DOS,
        },
        System::{
            Com::CoTaskMemFree,
            WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        },
        UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
    },
    core::{HRESULT, PCWSTR, PWSTR},
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
        // A separate thread leaves the caller's COM apartment unchanged.
        thread::Builder::new()
            .name("atlas-storage-roots".to_owned())
            .spawn(registered_roots)
            .map_err(|error| io_error(LocationOperation::StartSyncRootCheck, error))?
            .join()
            .map_err(|_| LocationError::SyncRootCheckPanicked)?
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
    Ok(PathBuf::from(local).join("Atlas").join("data"))
}

pub(super) fn open_directory(path: &Path, follow_link: bool) -> Result<File, LocationError> {
    // Candidate ancestors are inspected themselves, not followed through links.
    let flags = if follow_link {
        FILE_FLAG_BACKUP_SEMANTICS
    } else {
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT
    };
    open_existing(path, flags).map_err(|error| os_error(LocationOperation::OpenDirectory, error))
}

pub(super) fn optional_directory(path: &Path) -> Result<Option<File>, LocationError> {
    // An absent component can be created only after its existing parents pass.
    match open_existing(
        path,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
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
    // Only an absent file is optional; a missing parent or access failure is not.
    match open_existing(
        path,
        FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
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
            optional_file(path, LocationOperation::OpenDatabase)?
                .ok_or(LocationError::IdentityChanged)
        }
        Err(error) => Err(os_error(LocationOperation::CreateDatabase, error)),
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
    // Excluding delete sharing retains the inspected name until the handle closes.
    let wide = wide_file_path(path);
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
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

fn registered_roots() -> Result<Vec<PathBuf>, LocationError> {
    // Declare the apartment first so every WinRT object is released before it.
    let _apartment = RuntimeApartment::initialize()?;
    let roots = StorageProviderSyncRootManager::GetCurrentSyncRoots()
        .map_err(|error| os_error(LocationOperation::EnumerateSyncRoots, error))?;
    let length = roots
        .Size()
        .map_err(|error| os_error(LocationOperation::EnumerateSyncRoots, error))?;
    let mut paths = Vec::new();
    for index in 0..length {
        let root = roots
            .GetAt(index)
            .map_err(|error| os_error(LocationOperation::EnumerateSyncRoots, error))?;
        let folder = root
            .Path()
            .map_err(|error| os_error(LocationOperation::EnumerateSyncRoots, error))?;
        let path = folder
            .Path()
            .map_err(|error| os_error(LocationOperation::EnumerateSyncRoots, error))?;
        paths.push(PathBuf::from(OsString::from_wide(&path)));
    }
    Ok(paths)
}

fn os_error(operation: LocationOperation, error: windows::core::Error) -> LocationError {
    LocationError::Os {
        operation,
        code: Some(error.code().0),
    }
}

struct RuntimeApartment;

impl RuntimeApartment {
    fn initialize() -> Result<Self, LocationError> {
        // The generated Result preserves both successful initialization statuses.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|error| os_error(LocationOperation::InitializeRuntime, error))?;
        Ok(Self)
    }
}

impl Drop for RuntimeApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct TaskMemory(PWSTR);

impl Drop for TaskMemory {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.0.cast())) };
    }
}

//! Moves or removes checked maintenance files without replacing another filesystem entry.

use std::{
    fs::File,
    mem::{offset_of, size_of},
    os::windows::io::FromRawHandle,
    path::Path,
};

use atlas_store::{LocationError, LocationOperation};
use windows::{
    Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE},
        Storage::FileSystem::{
            CREATE_NEW, CreateFileW, DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO, FILE_SHARE_MODE, FILE_SHARE_READ,
            FileDispositionInfo, FileRenameInfo, OPEN_EXISTING, SetFileInformationByHandle,
        },
    },
    core::PCWSTR,
};

use super::super::{FileIdentity, check_entry};
use super::{entry_info, file_handle, os_error, wide_file_path};

pub(in crate::storage) fn open_readonly(
    path: &Path,
    operation: LocationOperation,
) -> Result<File, LocationError> {
    open_checked(path, GENERIC_READ.0, false, operation)
}

pub(in crate::storage) fn open_movable(
    path: &Path,
    operation: LocationOperation,
) -> Result<File, LocationError> {
    open_checked(
        path,
        GENERIC_READ.0 | GENERIC_WRITE.0 | DELETE.0,
        false,
        operation,
    )
}

fn open_checked(
    path: &Path,
    access: u32,
    directory: bool,
    operation: LocationOperation,
) -> Result<File, LocationError> {
    // Parent identities are retained by the caller; inspect this entry without following links.
    let wide = wide_file_path(path);
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(|error| os_error(operation, error))?;
    let file = unsafe { File::from_raw_handle(handle.0) };
    check_entry(&entry_info(&file)?, directory)?;
    Ok(file)
}

pub(in crate::storage) fn rename_no_replace(
    file: &File,
    destination: &Path,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    rename_handle(file, destination, false, operation)
}

/// Atomically replaces `destination` with the retained handle's file; the destination must
/// not be open without delete sharing.
pub(in crate::storage) fn rename_replacing(
    file: &File,
    destination: &Path,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    rename_handle(file, destination, true, operation)
}

/// Creates a fresh, unshared file that can later be renamed through its own handle.
pub(in crate::storage) fn create_movable(
    path: &Path,
    operation: LocationOperation,
) -> Result<File, LocationError> {
    let wide = wide_file_path(path);
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0 | DELETE.0,
            FILE_SHARE_MODE(0),
            None,
            CREATE_NEW,
            FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|error| os_error(operation, error))?;
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

/// Removes a regular file left by an interrupted write, through a checked handle.
pub(in crate::storage) fn remove_checked_entry(
    path: &Path,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    let file = open_movable(path, operation)?;
    dispose(&file, operation)
}

fn rename_handle(
    file: &File,
    destination: &Path,
    replace: bool,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    // FILE_RENAME_INFO targets the retained handle; `replace` decides an existing destination.
    let wide = wide_file_path(destination);
    let filename_bytes = (wide.len() - 1)
        .checked_mul(size_of::<u16>())
        .ok_or(LocationError::PathTooLong)?;
    let terminated_bytes = filename_bytes
        .checked_add(size_of::<u16>())
        .ok_or(LocationError::PathTooLong)?;
    let buffer_bytes = offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(terminated_bytes)
        .ok_or(LocationError::PathTooLong)?;
    let buffer_length = u32::try_from(buffer_bytes).map_err(|_| LocationError::PathTooLong)?;
    let mut buffer =
        vec![FILE_RENAME_INFO::default(); buffer_bytes.div_ceil(size_of::<FILE_RENAME_INFO>())];
    buffer[0].FileNameLength =
        u32::try_from(filename_bytes).map_err(|_| LocationError::PathTooLong)?;
    buffer[0].Anonymous.ReplaceIfExists = replace;
    // The typed allocation has the API structure's alignment and room for the trailing UTF-16 name.
    unsafe {
        let filename = buffer
            .as_mut_ptr()
            .cast::<u8>()
            .add(offset_of!(FILE_RENAME_INFO, FileName))
            .cast::<u16>();
        std::ptr::copy_nonoverlapping(wide.as_ptr(), filename, wide.len());
        SetFileInformationByHandle(
            file_handle(file),
            FileRenameInfo,
            buffer.as_ptr().cast(),
            buffer_length,
        )
    }
    .map_err(|error| os_error(operation, error))
}

pub(in crate::storage) fn remove_checked_file(
    path: &Path,
    identity: &FileIdentity,
) -> Result<(), LocationError> {
    let file = open_movable(path, LocationOperation::RemoveBackup)?;
    remove_identity(file, identity)
}

pub(in crate::storage) fn remove_checked_directory(
    path: &Path,
    identity: &FileIdentity,
) -> Result<(), LocationError> {
    // Windows refuses disposition of a directory containing any remaining child.
    let file = open_checked(
        path,
        GENERIC_READ.0 | DELETE.0,
        true,
        LocationOperation::RemoveBackup,
    )?;
    remove_identity(file, identity)
}

fn remove_identity(file: File, identity: &FileIdentity) -> Result<(), LocationError> {
    if entry_info(&file)?.identity != *identity {
        return Err(LocationError::IdentityChanged);
    }
    dispose(&file, LocationOperation::RemoveBackup)
}

fn dispose(file: &File, operation: LocationOperation) -> Result<(), LocationError> {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // Removal occurs only for the checked handle, never a path resolved after validation.
    unsafe {
        SetFileInformationByHandle(
            file_handle(file),
            FileDispositionInfo,
            (&raw const disposition).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    }
    .map_err(|error| os_error(operation, error))
}

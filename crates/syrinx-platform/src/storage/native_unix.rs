//! Holds the storage guard's checked handles on Linux and macOS through POSIX descriptors.

use std::{
    ffi::{CString, OsStr},
    fs::{File, OpenOptions},
    io,
    mem::MaybeUninit,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawFd, FromRawFd},
    },
    path::{Path, PathBuf},
};

use syrinx_store::{LocationError, LocationOperation};

use super::{EntryInfo, FileIdentity, LOCAL_DRIVE, LocationProbe, check_entry};

/// The `drive_type` answer for a network, FUSE or otherwise non-local filesystem.
const REMOTE_DRIVE: u32 = 4;

/// Folders the common sync clients create; a database inside one is refused like a Windows sync root.
const SYNC_FOLDERS: [&str; 7] = [
    "Dropbox",
    "OneDrive",
    "Nextcloud",
    "ownCloud",
    "pCloudDrive",
    "Google Drive",
    "MEGA",
];

pub(super) struct NativeProbe;

impl LocationProbe for NativeProbe {
    fn drive_type(&self, root: &Path) -> Result<u32, LocationError> {
        if is_local_filesystem(root)? {
            Ok(LOCAL_DRIVE)
        } else {
            Ok(REMOTE_DRIVE)
        }
    }

    fn sync_roots(&self) -> Result<Vec<PathBuf>, LocationError> {
        // Only folders that exist are roots; a missing one cannot hold the database.
        let Some(home) = home_directory() else {
            return Ok(Vec::new());
        };
        Ok(SYNC_FOLDERS
            .iter()
            .map(|name| home.join(name))
            .filter(|path| path.is_dir())
            .collect())
    }
}

pub(super) fn local_data_directory() -> Result<PathBuf, LocationError> {
    Ok(local_app_data()?.join("Syrinx").join("data"))
}

/// The per-user, non-synced data folder: XDG on Linux, Application Support on macOS.
pub fn local_app_data() -> Result<PathBuf, LocationError> {
    #[cfg(target_os = "macos")]
    {
        let home = home_directory().ok_or(LocationError::InvalidPath)?;
        Ok(home.join("Library").join("Application Support"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(data) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from)
            && data.is_absolute()
        {
            return Ok(data);
        }
        let home = home_directory().ok_or(LocationError::InvalidPath)?;
        Ok(home.join(".local").join("share"))
    }
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
}

pub(super) fn open_directory(path: &Path, follow_link: bool) -> Result<File, LocationError> {
    let flags = if follow_link {
        libc::O_DIRECTORY
    } else {
        libc::O_DIRECTORY | libc::O_NOFOLLOW
    };
    open_with(path, OpenOptions::new().read(true), flags)
        .map_err(|error| link_or_io(LocationOperation::OpenDirectory, error))
}

pub(super) fn open_retained_directory(path: &Path) -> Result<File, LocationError> {
    open_directory(path, false)
}

pub(super) fn create_directory(parent: &File, component: &OsStr) -> Result<File, LocationError> {
    // Create and open the child relative to the retained parent, never through its name again.
    super::validate_component(component)?;
    let name = c_name(component)?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        return Err(io_error(
            LocationOperation::CreateDirectory,
            io::Error::last_os_error(),
        ));
    }
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if descriptor < 0 {
        return Err(io_error(
            LocationOperation::CreateDirectory,
            io::Error::last_os_error(),
        ));
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

pub(super) fn optional_directory(path: &Path) -> Result<Option<File>, LocationError> {
    // Only an absent directory is optional; a link or access failure is not.
    match open_with(
        path,
        OpenOptions::new().read(true),
        libc::O_DIRECTORY | libc::O_NOFOLLOW,
    ) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(link_or_io(LocationOperation::OpenDirectory, error)),
    }
}

pub(super) fn optional_file(
    path: &Path,
    operation: LocationOperation,
) -> Result<Option<File>, LocationError> {
    // A metadata probe needs no write access, so it also inspects a locked control file.
    match open_with(path, OpenOptions::new().read(true), libc::O_NOFOLLOW) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(link_or_io(operation, error)),
    }
}

pub(super) fn optional_retained_file(
    path: &Path,
    operation: LocationOperation,
) -> Result<Option<File>, LocationError> {
    optional_file(path, operation)
}

pub(super) fn create_database(path: &Path) -> Result<File, LocationError> {
    // Exclusive creation cannot truncate an existing database, including a racing creator.
    match create_new_file(path, LocationOperation::CreateDatabase, true) {
        Ok(file) => Ok(file),
        Err(LocationError::Os {
            code: Some(libc::EEXIST),
            ..
        }) => optional_retained_file(path, LocationOperation::OpenDatabase)?
            .ok_or(LocationError::IdentityChanged),
        Err(error) => Err(error),
    }
}

pub(super) fn create_new_file(
    path: &Path,
    operation: LocationOperation,
    _shared_write: bool,
) -> Result<File, LocationError> {
    // Fresh outputs must never adopt a pre-existing file after a creation race.
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true).mode(0o600);
    open_with(path, &mut options, libc::O_NOFOLLOW).map_err(|error| link_or_io(operation, error))
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
    open_directory(path, false)?
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
    // An exclusive advisory lock stands in for Windows' no-sharing open.
    let mut options = OpenOptions::new();
    options.read(true).write(true).mode(0o600);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true);
    }
    let file = open_with(path, &mut options, libc::O_NOFOLLOW)
        .map_err(|error| link_or_io(operation, error))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            return Err(LocationError::StorageBusy);
        }
        return Err(io_error(operation, error));
    }
    Ok(file)
}

pub(super) fn entry_info(file: &File) -> Result<EntryInfo, LocationError> {
    let metadata = file
        .metadata()
        .map_err(|error| io_error(LocationOperation::InspectEntry, error))?;
    let mut id = [0; 16];
    id[..8].copy_from_slice(&metadata.ino().to_le_bytes());
    // An unlinked entry keeps working through its descriptor but has no name left.
    let links = u32::try_from(metadata.nlink()).unwrap_or(u32::MAX);
    Ok(EntryInfo {
        identity: FileIdentity {
            volume: metadata.dev(),
            id,
        },
        directory: metadata.is_dir(),
        reparse: metadata.file_type().is_symlink(),
        delete_pending: links == 0,
        links,
    })
}

pub(super) fn final_path(file: &File) -> Result<PathBuf, LocationError> {
    #[cfg(target_os = "macos")]
    {
        let mut buffer = vec![0u8; libc::PATH_MAX as usize];
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) } != 0 {
            return Err(io_error(
                LocationOperation::ResolvePath,
                io::Error::last_os_error(),
            ));
        }
        let length = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        Ok(PathBuf::from(OsStr::from_bytes(&buffer[..length])))
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
            .map_err(|error| io_error(LocationOperation::ResolvePath, error))
    }
}

pub(super) fn io_error(operation: LocationOperation, error: io::Error) -> LocationError {
    LocationError::Os {
        operation,
        code: error.raw_os_error(),
    }
}

pub(super) fn open_readonly(
    path: &Path,
    operation: LocationOperation,
) -> Result<File, LocationError> {
    open_checked(path, OpenOptions::new().read(true), false, operation)
}

pub(super) fn open_movable(
    path: &Path,
    operation: LocationOperation,
) -> Result<File, LocationError> {
    open_checked(
        path,
        OpenOptions::new().read(true).write(true),
        false,
        operation,
    )
}

pub(super) fn rename_no_replace(
    file: &File,
    destination: &Path,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    rename_retained(file, destination, false, operation)
}

/// Atomically replaces `destination` with the retained descriptor's file.
pub(super) fn rename_replacing(
    file: &File,
    destination: &Path,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    rename_retained(file, destination, true, operation)
}

/// Creates a fresh file that can later be renamed through its own descriptor.
pub(super) fn create_movable(
    path: &Path,
    operation: LocationOperation,
) -> Result<File, LocationError> {
    create_new_file(path, operation, false)
}

/// Removes a regular file left by an interrupted write, after checking the entry.
pub(super) fn remove_checked_entry(
    path: &Path,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    let file = open_movable(path, operation)?;
    unlink_retained(&file, false, operation)
}

pub(super) fn remove_checked_file(
    path: &Path,
    identity: &FileIdentity,
) -> Result<(), LocationError> {
    let file = open_movable(path, LocationOperation::RemoveBackup)?;
    remove_identity(&file, identity, false)
}

pub(super) fn remove_checked_directory(
    path: &Path,
    identity: &FileIdentity,
) -> Result<(), LocationError> {
    // rmdir refuses a directory that still holds any child, as Windows' disposition does.
    let file = open_checked(
        path,
        OpenOptions::new().read(true),
        true,
        LocationOperation::RemoveBackup,
    )?;
    remove_identity(&file, identity, true)
}

fn open_checked(
    path: &Path,
    options: &OpenOptions,
    directory: bool,
    operation: LocationOperation,
) -> Result<File, LocationError> {
    // Parent identities are retained by the caller; inspect this entry without following links.
    let flags = if directory {
        libc::O_DIRECTORY | libc::O_NOFOLLOW
    } else {
        libc::O_NOFOLLOW
    };
    let mut options = options.clone();
    let file =
        open_with(path, &mut options, flags).map_err(|error| link_or_io(operation, error))?;
    check_entry(&entry_info(&file)?, directory)?;
    Ok(file)
}

fn rename_retained(
    file: &File,
    destination: &Path,
    replace: bool,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    // Resolve the descriptor's current name, then refuse if that name now holds another entry.
    let source = final_path(file)?;
    let before = entry_info(file)?.identity;
    let named = open_with(&source, OpenOptions::new().read(true), libc::O_NOFOLLOW)
        .map_err(|error| link_or_io(operation, error))?;
    if entry_info(&named)?.identity != before {
        return Err(LocationError::IdentityChanged);
    }
    let from = c_path(&source)?;
    let to = c_path(destination)?;
    if !rename_paths(&from, &to, replace) {
        return Err(io_error(operation, io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn rename_paths(from: &CString, to: &CString, replace: bool) -> bool {
    let flags = if replace { 0 } else { libc::RENAME_NOREPLACE };
    unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            flags,
        ) == 0
    }
}

#[cfg(target_os = "macos")]
fn rename_paths(from: &CString, to: &CString, replace: bool) -> bool {
    let flags = if replace { 0 } else { libc::RENAME_EXCL };
    unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), flags) == 0 }
}

fn remove_identity(
    file: &File,
    identity: &FileIdentity,
    directory: bool,
) -> Result<(), LocationError> {
    if entry_info(file)?.identity != *identity {
        return Err(LocationError::IdentityChanged);
    }
    unlink_retained(file, directory, LocationOperation::RemoveBackup)
}

fn unlink_retained(
    file: &File,
    directory: bool,
    operation: LocationOperation,
) -> Result<(), LocationError> {
    // Remove only the name that still identifies the checked descriptor.
    let path = final_path(file)?;
    let named = open_with(&path, OpenOptions::new().read(true), libc::O_NOFOLLOW)
        .map_err(|error| link_or_io(operation, error))?;
    if entry_info(&named)?.identity != entry_info(file)?.identity {
        return Err(LocationError::IdentityChanged);
    }
    let removed = if directory {
        std::fs::remove_dir(&path)
    } else {
        std::fs::remove_file(&path)
    };
    removed.map_err(|error| io_error(operation, error))
}

fn open_with(path: &Path, options: &mut OpenOptions, flags: libc::c_int) -> io::Result<File> {
    let opened = options.custom_flags(flags | libc::O_CLOEXEC).open(path);
    // Linux answers ENOTDIR, not ELOOP, when a refused link stands where a folder was asked for.
    match opened {
        Err(error) if error.raw_os_error() == Some(libc::ENOTDIR) && is_link(path) => {
            Err(io::Error::from_raw_os_error(libc::ELOOP))
        }
        opened => opened,
    }
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

/// A link met where none may be followed is the Unix form of a reparse point.
fn link_or_io(operation: LocationOperation, error: io::Error) -> LocationError {
    match error.raw_os_error() {
        Some(libc::ELOOP) => LocationError::ReparsePoint,
        Some(libc::ENOTDIR) => LocationError::UnexpectedEntryKind,
        _ => io_error(operation, error),
    }
}

fn is_local_filesystem(path: &Path) -> Result<bool, LocationError> {
    let name = c_path(path)?;
    let mut status = MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::statfs(name.as_ptr(), status.as_mut_ptr()) } != 0 {
        return Err(io_error(
            LocationOperation::InspectEntry,
            io::Error::last_os_error(),
        ));
    }
    let status = unsafe { status.assume_init() };
    Ok(is_local_kind(&status))
}

#[cfg(target_os = "linux")]
fn is_local_kind(status: &libc::statfs) -> bool {
    // NFS, SMB/CIFS, FUSE (sync clients and network mounts), AFS, Ceph and 9p.
    const REMOTE_MAGIC: [i64; 9] = [
        0x6969,
        0x517B,
        0xFF53_4D42,
        0xFE53_4D42,
        0x6573_5546,
        0x5346_414F,
        0x00C3_6400,
        0x0102_1997,
        0x0BD0_0BD0,
    ];
    let magic = signed(status.f_type);
    !REMOTE_MAGIC.contains(&magic)
}

/// `f_type`'s width differs between C libraries and architectures.
#[cfg(target_os = "linux")]
fn signed<Kind: Into<i64>>(kind: Kind) -> i64 {
    kind.into()
}

#[cfg(target_os = "macos")]
fn is_local_kind(status: &libc::statfs) -> bool {
    status.f_flags & libc::MNT_LOCAL as u32 != 0
}

fn c_name(component: &OsStr) -> Result<CString, LocationError> {
    CString::new(component.as_bytes()).map_err(|_| LocationError::InvalidPath)
}

fn c_path(path: &Path) -> Result<CString, LocationError> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| LocationError::InvalidPath)
}

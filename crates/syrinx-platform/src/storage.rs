//! Retains checked directory and database identities for the store's lifetime.

mod backup;
mod cache;
mod deletions;
#[cfg_attr(unix, path = "storage/native_unix.rs")]
mod native;
mod recovery;
mod session;
mod startup;

pub use cache::CacheFolder;
pub use deletions::{DeletionJournal, JournalError, read_deletion_log};
#[cfg(unix)]
pub use native::local_app_data;
pub use recovery::{RecoveryError, RecoveryOffer, RecoveryOutcome, offer_recovery};
pub use startup::{StartupStorage, prepare_startup};

#[cfg(unix)]
#[path = "storage/paths_unix.rs"]
mod paths_unix;
#[cfg(test)]
mod tests;
#[cfg(all(test, unix))]
#[path = "storage/tests_unix.rs"]
mod tests_unix;
#[cfg(unix)]
use paths_unix::{
    normal_absolute_path, require_local_drive, sqlite_database_path, validate_component,
};

use std::{
    ffi::{OsStr, OsString},
    fmt,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
#[cfg(windows)]
use std::{
    os::windows::ffi::OsStrExt,
    path::{Component, Prefix},
};

use syrinx_store::{
    BackupArchive, DirectoryLease, LocationError, LocationOperation, SessionControl,
};
#[cfg(windows)]
use windows::Win32::{Foundation::MAX_PATH, System::WindowsProgramming::DRIVE_FIXED};

/// The probe's answer for a fixed local disk, the only place the database may live.
#[cfg(windows)]
const LOCAL_DRIVE: u32 = DRIVE_FIXED;
#[cfg(unix)]
const LOCAL_DRIVE: u32 = 3;

const DATABASE_NAME: &str = "syrinx.db";
const SIDECAR_NAMES: [&str; 3] = ["syrinx.db-wal", "syrinx.db-shm", "syrinx.db-journal"];

/// Owns the handles that keep a checked storage location in place.
pub struct WindowsDirectoryLease {
    database: GuardedEntry,
    sqlite_path: PathBuf,
    directories: Arc<Vec<GuardedEntry>>,
    control: session::LifecycleControl,
    probe: Arc<dyn LocationProbe>,
    archive: Arc<backup::WindowsBackupArchive>,
}

#[derive(Clone)]
struct PreparedDirectory {
    path: PathBuf,
    directories: Arc<Vec<GuardedEntry>>,
    control: session::LifecycleControl,
    probe: Arc<dyn LocationProbe>,
}

impl WindowsDirectoryLease {
    /// Discovers the production local app-data location, then applies the guard.
    pub fn discover() -> Result<Self, LocationError> {
        Self::prepare(&native::local_data_directory()?)
    }

    /// Applies the same guard to an explicit directory, creating missing children.
    ///
    /// This may create the directory and an empty database, but never replaces
    /// an existing file. The caller must retain the lease while SQLite is open.
    pub fn prepare(data_directory: &Path) -> Result<Self, LocationError> {
        Self::prepare_with_probe(data_directory, Arc::new(native::NativeProbe))
    }

    fn prepare_with_probe(
        data_directory: &Path,
        probe: Arc<dyn LocationProbe>,
    ) -> Result<Self, LocationError> {
        let prepared = prepare_directory(data_directory, probe)?;
        // Normal startup must refuse unfinished recovery before it can create a primary.
        prepared.control.revalidate()?;
        inspect_sidecars(&prepared.path)?;

        // Preserve an existing database; exclusive creation handles a missing one.
        let database_path = prepared.path.join(DATABASE_NAME);
        let database_file = match native::optional_retained_file(
            &database_path,
            LocationOperation::OpenDatabase,
        )? {
            Some(file) => file,
            None => native::create_database(&database_path)?,
        };
        let database = GuardedEntry::regular_file(database_path, database_file)?;
        Self::from_prepared(prepared, database)
    }

    fn from_prepared(
        prepared: PreparedDirectory,
        database: GuardedEntry,
    ) -> Result<Self, LocationError> {
        let archive = Arc::new(backup::WindowsBackupArchive::new(prepared.clone()));
        let PreparedDirectory {
            directories,
            control,
            probe,
            ..
        } = prepared;
        let lease = Self {
            sqlite_path: sqlite_database_path(&database.path),
            database,
            directories,
            control,
            probe,
            archive,
        };
        lease.revalidate_entries()?;
        Ok(lease)
    }

    fn revalidate_entries(&self) -> Result<(), LocationError> {
        // Both the retained handle and its current name must identify the same entry.
        self.control.revalidate()?;
        self.database
            .revalidate(false, LocationOperation::OpenDatabase)?;
        inspect_sidecars(
            self.database
                .path
                .parent()
                .ok_or(LocationError::InvalidPath)?,
        )
    }
}

impl DirectoryLease for WindowsDirectoryLease {
    fn database_path(&self) -> &Path {
        &self.sqlite_path
    }

    fn revalidate(&self) -> Result<(), LocationError> {
        // Registration and metadata can change after preparation; refuse uncertainty.
        require_local_drive(&self.database.path, self.probe.as_ref())?;
        self.revalidate_entries()?;
        refuse_sync_roots(&self.directories, self.probe.as_ref())
    }

    fn session_control(&self) -> Option<&dyn SessionControl> {
        Some(&self.control)
    }

    fn backup_archive(&self) -> Option<Arc<dyn BackupArchive>> {
        Some(self.archive.clone())
    }
}

impl fmt::Debug for WindowsDirectoryLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WindowsDirectoryLease { location: [redacted] }")
    }
}

impl PreparedDirectory {
    fn revalidate(&self) -> Result<(), LocationError> {
        self.revalidate_entries()?;
        refuse_sync_roots(&self.directories, self.probe.as_ref())
    }

    fn revalidate_entries(&self) -> Result<(), LocationError> {
        require_local_drive(&self.path, self.probe.as_ref())?;
        self.control.revalidate_location()
    }
}

fn prepare_directory(
    data_directory: &Path,
    probe: Arc<dyn LocationProbe>,
) -> Result<PreparedDirectory, LocationError> {
    prepare_directory_with_creation(data_directory, probe, false).map(|(prepared, _)| prepared)
}

fn prepare_directory_with_creation(
    data_directory: &Path,
    probe: Arc<dyn LocationProbe>,
    exclusive_fresh_control: bool,
) -> Result<(PreparedDirectory, bool), LocationError> {
    // Check the supplied path before any directory or database is created.
    let requested = normal_absolute_path(data_directory)?;
    if requested.file_name().is_none() {
        return Err(LocationError::InvalidPath);
    }
    require_local_drive(&requested, probe.as_ref())?;
    let (mut directories, missing) = lock_existing_ancestors(&requested)?;
    let nearest = directories.last().ok_or(LocationError::InvalidPath)?;

    // A drive alias can hide physical ancestors, including a registered sync root.
    let physical_parent = normal_absolute_path(&native::final_path(&nearest.file)?)?;
    require_local_drive(&physical_parent, probe.as_ref())?;
    let (physical_directories, physical_missing) = lock_existing_ancestors(&physical_parent)?;
    if !physical_missing.is_empty() {
        return Err(LocationError::IdentityChanged);
    }
    directories.extend(physical_directories);
    refuse_sync_roots(&directories, probe.as_ref())?;

    // Create one checked child at a time while retaining every parent handle.
    let mut destination = physical_parent;
    let mut created_final = false;
    let final_index = missing.len().checked_sub(1);
    for (index, component) in missing.into_iter().enumerate() {
        destination.push(&component);
        if exclusive_fresh_control && Some(index) == final_index {
            let parent = directories.last().ok_or(LocationError::InvalidPath)?;
            let file = native::create_directory(&parent.file, &component)?;
            directories.push(GuardedEntry::from_directory_file(&destination, file)?);
            created_final = true;
        } else {
            create_directory(&destination)?;
            directories.push(GuardedEntry::directory(&destination)?);
        }
    }
    refuse_sync_roots(&directories, probe.as_ref())?;

    // Maintenance can inspect an interrupted recovery without creating an empty primary.
    let directories = Arc::new(directories);
    let control = if exclusive_fresh_control && created_final {
        session::LifecycleControl::acquire_new(&destination, Arc::clone(&directories))?
    } else {
        session::LifecycleControl::acquire(&destination, Arc::clone(&directories))?
    };
    control.revalidate_location()?;
    Ok((
        PreparedDirectory {
            path: destination,
            directories,
            control,
            probe,
        },
        created_final,
    ))
}

trait LocationProbe: Send + Sync {
    fn drive_type(&self, root: &Path) -> Result<u32, LocationError>;
    fn sync_roots(&self) -> Result<Vec<PathBuf>, LocationError>;
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct FileIdentity {
    volume: u64,
    id: [u8; 16],
}

struct EntryInfo {
    identity: FileIdentity,
    directory: bool,
    reparse: bool,
    delete_pending: bool,
    links: u32,
}

struct GuardedEntry {
    path: PathBuf,
    file: File,
    identity: FileIdentity,
}

impl GuardedEntry {
    fn directory(path: &Path) -> Result<Self, LocationError> {
        let file = native::open_retained_directory(path)?;
        Self::from_directory_file(path, file)
    }

    fn from_directory_file(path: &Path, file: File) -> Result<Self, LocationError> {
        let information = native::entry_info(&file)?;
        check_entry(&information, true)?;
        Ok(Self {
            path: path.to_owned(),
            file,
            identity: information.identity,
        })
    }

    fn regular_file(path: PathBuf, file: File) -> Result<Self, LocationError> {
        let information = native::entry_info(&file)?;
        check_entry(&information, false)?;
        Ok(Self {
            path,
            file,
            identity: information.identity,
        })
    }

    fn revalidate(
        &self,
        directory: bool,
        operation: LocationOperation,
    ) -> Result<(), LocationError> {
        let retained = native::entry_info(&self.file)?;
        check_entry(&retained, directory)?;
        let named_file = if directory {
            native::open_directory(&self.path, false)?
        } else {
            native::optional_file(&self.path, operation)?.ok_or(LocationError::IdentityChanged)?
        };
        let named = native::entry_info(&named_file)?;
        check_entry(&named, directory)?;
        if retained.identity != self.identity || named.identity != self.identity {
            return Err(LocationError::IdentityChanged);
        }
        Ok(())
    }
}

fn lock_existing_ancestors(
    path: &Path,
) -> Result<(Vec<GuardedEntry>, Vec<OsString>), LocationError> {
    let mut directories = Vec::new();
    let mut missing = Vec::new();
    let ancestors: Vec<_> = path.ancestors().collect();
    for ancestor in ancestors.into_iter().rev() {
        // Once a component is missing, no descendant may be queried through it.
        if !missing.is_empty() {
            missing.push(
                ancestor
                    .file_name()
                    .ok_or(LocationError::InvalidPath)?
                    .to_owned(),
            );
            continue;
        }
        match native::optional_directory(ancestor)? {
            Some(file) => directories.push(GuardedEntry::from_directory_file(ancestor, file)?),
            None => missing.push(
                ancestor
                    .file_name()
                    .ok_or(LocationError::InvalidPath)?
                    .to_owned(),
            ),
        }
    }
    Ok((directories, missing))
}

fn create_directory(path: &Path) -> Result<(), LocationError> {
    // A racing creator is accepted only after the caller checks the resulting entry.
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(native::io_error(LocationOperation::CreateDirectory, error)),
    }
}

fn inspect_sidecars(directory: &Path) -> Result<(), LocationError> {
    for filename in SIDECAR_NAMES {
        // Release this handle before SQLite needs to remove or replace the sidecar.
        let Some(file) =
            native::optional_file(&directory.join(filename), LocationOperation::OpenSidecar)?
        else {
            continue;
        };
        check_entry(&native::entry_info(&file)?, false)?;
    }
    Ok(())
}

fn check_entry(information: &EntryInfo, directory: bool) -> Result<(), LocationError> {
    if information.reparse {
        return Err(LocationError::ReparsePoint);
    }
    if information.directory != directory {
        return Err(LocationError::UnexpectedEntryKind);
    }
    if information.delete_pending {
        return Err(LocationError::DeletePending);
    }
    if !directory && information.links != 1 {
        return Err(LocationError::HardLinkedFile);
    }
    Ok(())
}

#[cfg(windows)]
fn require_local_drive(path: &Path, probe: &dyn LocationProbe) -> Result<(), LocationError> {
    let root = path.ancestors().last().ok_or(LocationError::InvalidPath)?;
    if probe.drive_type(root)? != LOCAL_DRIVE {
        return Err(LocationError::NonLocalDrive);
    }
    Ok(())
}

fn refuse_sync_roots(
    directories: &[GuardedEntry],
    probe: &dyn LocationProbe,
) -> Result<(), LocationError> {
    refuse_root_identities(directories, &sync_root_identities(probe)?)
}

fn sync_root_identities(probe: &dyn LocationProbe) -> Result<Vec<FileIdentity>, LocationError> {
    let mut identities = Vec::new();
    for root in probe.sync_roots()? {
        // Resolve registered roots by identity, including aliases of a checked ancestor.
        let root = normal_absolute_path(&root)?;
        require_local_drive(&root, probe)?;
        let file = native::open_directory(&root, true)?;
        let information = native::entry_info(&file)?;
        if !information.directory || information.delete_pending {
            return Err(LocationError::UnexpectedEntryKind);
        }
        identities.push(information.identity);
    }
    Ok(identities)
}

fn refuse_root_identities(
    directories: &[GuardedEntry],
    roots: &[FileIdentity],
) -> Result<(), LocationError> {
    if directories
        .iter()
        .any(|entry| roots.contains(&entry.identity))
    {
        return Err(LocationError::SyncedDirectory);
    }
    Ok(())
}

#[cfg(windows)]
fn normal_absolute_path(path: &Path) -> Result<PathBuf, LocationError> {
    // Keep one normal DOS spelling for identity checks and native handle opens.
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(LocationError::InvalidPath);
    };
    let drive = match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
        _ => return Err(LocationError::InvalidPath),
    };
    if components.next() != Some(Component::RootDir) {
        return Err(LocationError::InvalidPath);
    }
    let mut normalized = PathBuf::from(format!("{}:\\", char::from(drive.to_ascii_uppercase())));
    for component in components {
        let Component::Normal(name) = component else {
            return Err(LocationError::InvalidPath);
        };
        validate_component(name)?;
        normalized.push(name);
    }
    Ok(normalized)
}

#[cfg(windows)]
fn sqlite_database_path(path: &Path) -> PathBuf {
    // Keep ordinary DOS spelling when the database and its longest sidecar fit.
    // Include the sidecar suffix and terminator before choosing extended-path spelling.
    let longest_name_units = path.as_os_str().encode_wide().count() + "-journal".len() + 1;
    if longest_name_units <= MAX_PATH as usize {
        return path.to_owned();
    }
    let mut extended = OsString::from(r"\\?\");
    extended.push(path);
    PathBuf::from(extended)
}

#[cfg(windows)]
fn validate_component(name: &OsStr) -> Result<(), LocationError> {
    // DOS aliases, alternate streams and stripped trailing characters change identity.
    let wide: Vec<_> = name.encode_wide().collect();
    if wide.is_empty()
        || wide
            .last()
            .is_some_and(|last| *last == u16::from(b'.') || *last == u16::from(b' '))
        || wide.iter().any(|value| {
            *value < 32 || b"<>:\"/\\|?*".iter().any(|byte| *value == u16::from(*byte))
        })
    {
        return Err(LocationError::InvalidPath);
    }
    if name.to_str().is_some_and(is_reserved_name) {
        return Err(LocationError::InvalidPath);
    }
    Ok(())
}

#[cfg(windows)]
fn is_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return true;
    }
    let Some(prefix) = stem.get(..3) else {
        return false;
    };
    // Windows also reserves these three superscript digits in device names.
    (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
        && matches!(
            stem.get(3..),
            Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
        )
}

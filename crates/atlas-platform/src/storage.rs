//! Retains checked directory and database identities for the store's lifetime.

mod native;

#[cfg(test)]
mod tests;

use std::{
    ffi::{OsStr, OsString},
    fmt,
    fs::{self, File},
    io,
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf, Prefix},
    sync::Arc,
};

use atlas_store::{DirectoryLease, LocationError, LocationOperation};
use windows::Win32::{Foundation::MAX_PATH, System::WindowsProgramming::DRIVE_FIXED};

const DATABASE_NAME: &str = "atlas.db";
const SIDECAR_NAMES: [&str; 3] = ["atlas.db-wal", "atlas.db-shm", "atlas.db-journal"];

/// Owns the handles that keep a checked storage location in place.
pub struct WindowsDirectoryLease {
    database: GuardedEntry,
    sqlite_path: PathBuf,
    directories: Vec<GuardedEntry>,
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
        Self::prepare_with_probe(data_directory, Arc::new(native::WindowsProbe))
    }

    fn prepare_with_probe(
        data_directory: &Path,
        probe: Arc<dyn LocationProbe>,
    ) -> Result<Self, LocationError> {
        // Check the supplied path before any directory or database is created.
        let requested = normal_absolute_path(data_directory)?;
        if requested.file_name().is_none() {
            return Err(LocationError::InvalidPath);
        }
        require_local_drive(&requested, probe.as_ref())?;
        let (mut directories, missing) = lock_existing_ancestors(&requested)?;
        let nearest = directories.last().ok_or(LocationError::InvalidPath)?;

        // A drive alias can hide physical ancestors, including a sync root.
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
        for component in missing {
            destination.push(component);
            create_directory(&destination)?;
            directories.push(GuardedEntry::directory(&destination)?);
        }
        refuse_sync_roots(&directories, probe.as_ref())?;
        inspect_sidecars(&destination)?;

        // Preserve an existing database; exclusive creation handles a missing one.
        let database_path = destination.join(DATABASE_NAME);
        let database_file =
            match native::optional_file(&database_path, LocationOperation::OpenDatabase)? {
                Some(file) => file,
                None => native::create_database(&database_path)?,
            };
        let database = GuardedEntry::database(database_path, database_file)?;
        let lease = Self {
            sqlite_path: sqlite_database_path(&database.path),
            database,
            directories,
            probe,
        };
        lease.revalidate_entries()?;
        Ok(lease)
    }

    fn revalidate_entries(&self) -> Result<(), LocationError> {
        // Both the retained handle and its current name must identify the same entry.
        for directory in &self.directories {
            directory.revalidate(true)?;
        }
        self.database.revalidate(false)?;
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
}

impl fmt::Debug for WindowsDirectoryLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WindowsDirectoryLease { location: [redacted] }")
    }
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
        let file = native::open_directory(path, false)?;
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

    fn database(path: PathBuf, file: File) -> Result<Self, LocationError> {
        let information = native::entry_info(&file)?;
        check_entry(&information, false)?;
        Ok(Self {
            path,
            file,
            identity: information.identity,
        })
    }

    fn revalidate(&self, directory: bool) -> Result<(), LocationError> {
        let retained = native::entry_info(&self.file)?;
        check_entry(&retained, directory)?;
        let named_file = if directory {
            native::open_directory(&self.path, false)?
        } else {
            native::optional_file(&self.path, LocationOperation::OpenDatabase)?
                .ok_or(LocationError::IdentityChanged)?
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

fn require_local_drive(path: &Path, probe: &dyn LocationProbe) -> Result<(), LocationError> {
    let root = path.ancestors().last().ok_or(LocationError::InvalidPath)?;
    if probe.drive_type(root)? != DRIVE_FIXED {
        return Err(LocationError::NonLocalDrive);
    }
    Ok(())
}

fn refuse_sync_roots(
    directories: &[GuardedEntry],
    probe: &dyn LocationProbe,
) -> Result<(), LocationError> {
    for root in probe.sync_roots()? {
        // Resolve registered roots by identity, including aliases of a checked ancestor.
        let root = normal_absolute_path(&root)?;
        require_local_drive(&root, probe)?;
        let file = native::open_directory(&root, true)?;
        let information = native::entry_info(&file)?;
        if !information.directory || information.delete_pending {
            return Err(LocationError::UnexpectedEntryKind);
        }
        if directories
            .iter()
            .any(|entry| entry.identity == information.identity)
        {
            return Err(LocationError::SyncedDirectory);
        }
    }
    Ok(())
}

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

fn sqlite_database_path(path: &Path) -> PathBuf {
    // SQLite 3.53.2 gives extended paths slower WAL locking; use them only when required.
    // Include its longest sidecar suffix and the terminator in Windows's limit.
    let longest_name_units = path.as_os_str().encode_wide().count() + "-journal".len() + 1;
    if longest_name_units <= MAX_PATH as usize {
        return path.to_owned();
    }
    let mut extended = OsString::from(r"\\?\");
    extended.push(path);
    PathBuf::from(extended)
}

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

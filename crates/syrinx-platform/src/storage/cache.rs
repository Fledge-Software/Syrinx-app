//! Holds a cache folder beside the database under the database's own location checks (D13, D40).

use std::{
    ffi::OsStr,
    fmt, fs,
    path::{Path, PathBuf},
};

use syrinx_store::{DirectoryLease, LocationError, LocationOperation};

use super::{
    GuardedEntry, WindowsDirectoryLease, create_directory, native, refuse_root_identities,
    sync_root_identities, validate_component,
};

/// A checked folder beside `syrinx.db` that backups never copy, such as the full-text index.
///
/// Its handle is retained, so while this value lives the folder cannot be renamed, replaced
/// or swapped for a link; the files inside stay the cache's own to create and delete.
pub struct CacheFolder {
    directory: GuardedEntry,
}

impl CacheFolder {
    pub fn path(&self) -> &Path {
        &self.directory.path
    }

    /// Deletes every file inside, so a damaged cache can start afresh.
    ///
    /// Every entry is checked first: a subfolder or a link is refused before anything is
    /// deleted, so nothing outside this folder can be reached through one.
    pub fn empty(&self) -> Result<(), LocationError> {
        self.directory
            .revalidate(true, LocationOperation::OpenDirectory)?;
        for file in self.plain_files()? {
            fs::remove_file(&file)
                .map_err(|error| native::io_error(LocationOperation::EmptyCacheFolder, error))?;
        }
        Ok(())
    }

    /// Every entry inside, refusing anything but a plain file.
    fn plain_files(&self) -> Result<Vec<PathBuf>, LocationError> {
        let failed = |error| native::io_error(LocationOperation::EmptyCacheFolder, error);
        let mut files = Vec::new();
        for entry in fs::read_dir(self.path()).map_err(failed)? {
            let entry = entry.map_err(failed)?;
            // The entry's own type, never a link's target.
            if !entry.file_type().map_err(failed)?.is_file() {
                return Err(LocationError::UnexpectedEntryKind);
            }
            files.push(entry.path());
        }
        Ok(files)
    }
}

impl fmt::Debug for CacheFolder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CacheFolder { location: [redacted] }")
    }
}

impl WindowsDirectoryLease {
    /// Opens or creates the folder `name` beside the database.
    ///
    /// The checks are the database's: a local drive, no registered sync root above it, and
    /// neither a link nor a file in its place. A synced location is refused before creation.
    pub fn cache_folder(&self, name: &str) -> Result<CacheFolder, LocationError> {
        // One plain component: no separator, device name, or `.` and `..` (trailing dots).
        validate_component(OsStr::new(name))?;
        // The lease's own checks run first, so nothing is created in a refused location.
        self.revalidate()?;
        let sync_roots = sync_root_identities(self.probe.as_ref())?;
        let parent = self
            .database
            .path
            .parent()
            .ok_or(LocationError::InvalidPath)?;
        let directory = open_or_create(&parent.join(name))?;
        refuse_root_identities(std::slice::from_ref(&directory), &sync_roots)?;
        directory.revalidate(true, LocationOperation::OpenDirectory)?;
        Ok(CacheFolder { directory })
    }
}

/// Retains the folder at `path`, creating it when absent; a link or a file is refused.
fn open_or_create(path: &Path) -> Result<GuardedEntry, LocationError> {
    let file = match native::optional_directory(path)? {
        Some(file) => file,
        None => {
            create_directory(path)?;
            native::open_retained_directory(path)?
        }
    };
    GuardedEntry::from_directory_file(path, file)
}

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
    ///
    /// Every other file is tried before the index's metadata, which is deleted only once they
    /// are all gone. So if one cannot be deleted, `meta.json` still names the deleted files,
    /// the next open of the index finds it damaged, and that open empties the folder again.
    pub fn empty(&self) -> Result<(), LocationError> {
        self.directory
            .revalidate(true, LocationOperation::OpenDirectory)?;
        let (mut metadata, others): (Vec<_>, Vec<_>) = self
            .plain_files()?
            .into_iter()
            .partition(|file| metadata_rank(file).is_some());
        remove_each(&others)?;
        metadata.sort_by_key(|file| metadata_rank(file));
        // Stops at the first failure, so `.managed.json` outlives `meta.json`, never the reverse.
        for file in &metadata {
            remove(file)?;
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

/// The full-text index's metadata, Tantivy's `META_FILEPATH` and `MANAGED_FILEPATH`, in the
/// order `empty` deletes it after every other file.
///
/// While `meta.json` exists, an open reads the old index instead of creating a fresh one
/// beside its leftovers, which Tantivy's garbage collection would never track (D40). Once
/// it is gone, a leftover `.managed.json` only names missing files, which Tantivy skips.
const METADATA_DELETED_LAST: [&str; 2] = ["meta.json", ".managed.json"];

/// Deletes every file in `files`, trying each even after a failure, so a file that cannot be
/// deleted leaves no more behind than itself; the first failure is returned.
fn remove_each(files: &[PathBuf]) -> Result<(), LocationError> {
    let mut first_failure = Ok(());
    for file in files {
        let removed = remove(file);
        if first_failure.is_ok() {
            first_failure = removed;
        }
    }
    first_failure
}

fn remove(file: &Path) -> Result<(), LocationError> {
    fs::remove_file(file)
        .map_err(|error| native::io_error(LocationOperation::EmptyCacheFolder, error))
}

/// The file's place in `METADATA_DELETED_LAST`, or `None` for every other file.
fn metadata_rank(file: &Path) -> Option<usize> {
    let name = file.file_name()?;
    // Windows names are case-insensitive, so `META.JSON` is the same file.
    METADATA_DELETED_LAST
        .iter()
        .position(|metadata| name.eq_ignore_ascii_case(metadata))
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

//! Keeps a copy of the owner's excluded sites beside the database, outside every copy a
//! restore can replace, so restoring a backup cannot make an excluded site capturable (#199).

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use syrinx_store::{DirectoryLease, LocationError, LocationOperation};

use super::{GuardedEntry, WindowsDirectoryLease, check_entry, native};

/// Fixed name beside `syrinx.db` and `syrinx.deletions`; backups copy only the database.
pub(super) const EXCLUDED_SITES_NAME: &str = "syrinx.excluded-sites";
/// A save writes the new list here, then renames it over the old one.
const REPLACEMENT_SUFFIX: &str = ".new";
/// Far more than any owner's list; a larger file is refused rather than read in part.
const SIZE_LIMIT: u64 = 1024 * 1024;

/// The checked data folder that holds the excluded-sites file, retained for the session.
pub struct ExcludedSitesFile {
    directory: GuardedEntry,
}

impl WindowsDirectoryLease {
    /// The excluded-sites file beside this lease's database; the file itself may not exist yet.
    pub fn excluded_sites_file(&self) -> Result<ExcludedSitesFile, LocationError> {
        self.revalidate()?;
        let parent = self
            .database
            .path
            .parent()
            .ok_or(LocationError::InvalidPath)?;
        Ok(ExcludedSitesFile {
            directory: GuardedEntry::directory(parent)?,
        })
    }
}

impl ExcludedSitesFile {
    /// The saved list's bytes, or `None` before the first save.
    pub fn read(&self) -> Result<Option<Vec<u8>>, LocationError> {
        self.revalidate()?;
        // A plain probe opens for attributes only on Windows; reading needs data access.
        let Some(file) =
            native::optional_retained_file(&self.path(), LocationOperation::OpenExcludedSites)?
        else {
            return Ok(None);
        };
        check_entry(&native::entry_info(&file)?, false)?;
        read_bounded(&file).map(Some)
    }

    /// Durably replaces the saved list: written aside, flushed, renamed over the old one, so
    /// a crash leaves either the old list or the new one, never a torn one.
    pub fn save(&self, bytes: &[u8]) -> Result<(), LocationError> {
        self.revalidate()?;
        let path = self.path();
        // An existing entry must be a plain file: never replace a link or a folder.
        if let Some(existing) = native::optional_file(&path, LocationOperation::OpenExcludedSites)?
        {
            check_entry(&native::entry_info(&existing)?, false)?;
        }
        let replacement = write_replacement(&replacement_path(&path), bytes)?;
        native::rename_replacing(&replacement, &path, LocationOperation::WriteExcludedSites)?;
        drop(replacement);
        native::flush_directory(&self.directory.path, LocationOperation::FlushExcludedSites)
    }

    fn path(&self) -> PathBuf {
        self.directory.path.join(EXCLUDED_SITES_NAME)
    }

    fn revalidate(&self) -> Result<(), LocationError> {
        self.directory
            .revalidate(true, LocationOperation::OpenDirectory)
    }
}

/// Writes and flushes the new list, removing any earlier attempt's file first.
fn write_replacement(path: &Path, bytes: &[u8]) -> Result<File, LocationError> {
    let operation = LocationOperation::WriteExcludedSites;
    if native::optional_file(path, operation)?.is_some() {
        native::remove_checked_entry(path, operation)?;
    }
    let file = native::create_movable(path, operation)?;
    let mut writer = &file;
    writer
        .write_all(bytes)
        .map_err(|error| native::io_error(operation, error))?;
    file.sync_all()
        .map_err(|error| native::io_error(LocationOperation::FlushExcludedSites, error))?;
    Ok(file)
}

fn replacement_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(REPLACEMENT_SUFFIX);
    name.into()
}

/// Reads the whole file, refusing one past the size limit.
fn read_bounded(file: &File) -> Result<Vec<u8>, LocationError> {
    let operation = LocationOperation::ReadExcludedSites;
    let mut reader = file;
    let mut bytes = Vec::new();
    reader
        .seek(SeekFrom::Start(0))
        .and_then(|_| reader.take(SIZE_LIMIT + 1).read_to_end(&mut bytes))
        .map_err(|error| native::io_error(operation, error))?;
    if bytes.len() as u64 > SIZE_LIMIT {
        return Err(LocationError::Os {
            operation,
            code: None,
        });
    }
    Ok(bytes)
}

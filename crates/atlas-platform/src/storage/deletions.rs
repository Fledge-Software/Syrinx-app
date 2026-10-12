//! Keeps D47's deletion journal beside the database, outside every copy a restore can replace.

use std::{
    error::Error,
    fmt,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

use atlas_store::{
    DirectoryLease, LocationError, LocationOperation,
    deletion::{DeletionError, DeletionLog, DeletionRecord, EMPTY_DELETION_LOG},
};

use super::{GuardedEntry, WindowsDirectoryLease, check_entry, native};

/// Fixed name beside `atlas.db` and `atlas.lifecycle`; backups copy only the database.
pub(super) const JOURNAL_NAME: &str = "atlas.deletions";

/// Reading stops one byte past the codec's limit, so an oversized file is refused, not truncated.
const READ_LIMIT: u64 = 16 * 1024 * 1024 + 1;

/// Structural journal failures; none carries a path, range or record content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalError {
    Location(LocationError),
    Deletion(DeletionError),
    /// An append failed part-way, so this handle refuses further appends until restart.
    Poisoned,
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "history deletion record failed: {self:?}")
    }
}

impl Error for JournalError {}

impl From<LocationError> for JournalError {
    fn from(error: LocationError) -> Self {
        Self::Location(error)
    }
}

impl From<DeletionError> for JournalError {
    fn from(error: DeletionError) -> Self {
        Self::Deletion(error)
    }
}

/// The open, exclusively held journal of a running session.
pub struct DeletionJournal {
    entry: GuardedEntry,
    log: DeletionLog,
    poisoned: bool,
}

impl fmt::Debug for DeletionJournal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeletionJournal")
            .field("log", &self.log)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

impl DeletionJournal {
    /// Opens the journal beside a ready lease's database, creating it when absent.
    ///
    /// The caller still compares the result with the database's applied generation: a journal
    /// that is missing records shows up there as `DeletionError::MissingGeneration`.
    pub fn open(lease: &WindowsDirectoryLease) -> Result<Self, JournalError> {
        lease.revalidate()?;
        let directory = lease
            .database
            .path
            .parent()
            .ok_or(LocationError::InvalidPath)?;
        let path = directory.join(JOURNAL_NAME);

        // Inspect an existing entry without following a link before taking the exclusive handle.
        if let Some(file) = native::optional_file(&path, LocationOperation::OpenDeletionJournal)? {
            check_entry(&native::entry_info(&file)?, false)?;
        }
        let file = native::open_journal(&path)?;
        let entry = GuardedEntry::regular_file(path, file)?;
        let mut bytes = read_bounded(&entry.file)?;
        if bytes.is_empty() {
            // A new or never-initialized file holds no record, so writing the header loses nothing.
            persist(&entry.file, 0, EMPTY_DELETION_LOG)?;
            bytes = EMPTY_DELETION_LOG.to_vec();
        }
        let log = DeletionLog::decode(&bytes, 0)?;
        Ok(Self {
            entry,
            log,
            poisoned: false,
        })
    }

    pub fn log(&self) -> &DeletionLog {
        &self.log
    }

    /// Durably appends a record, then accepts it; a refused or failed record changes nothing.
    pub fn append(&mut self, record: DeletionRecord) -> Result<(), JournalError> {
        if self.poisoned {
            return Err(JournalError::Poisoned);
        }
        let mut next = self.log.clone();
        next.accept(record)?;
        self.entry
            .revalidate(false, LocationOperation::OpenDeletionJournal)?;
        let length = self
            .entry
            .file
            .metadata()
            .map_err(|error| native::io_error(LocationOperation::ReadDeletionJournal, error))?
            .len();
        if let Err(error) = persist(&self.entry.file, length, &record.encode()) {
            // A torn tail is refused by the codec at the next start; never append after it.
            self.poisoned = true;
            return Err(error.into());
        }
        self.log = next;
        Ok(())
    }
}

/// Reads the journal for a recovery offer without creating or locking it; absent means empty.
pub fn read_deletion_log(directory: &Path) -> Result<DeletionLog, JournalError> {
    let path = directory.join(JOURNAL_NAME);
    let Some(file) = native::optional_retained_file(&path, LocationOperation::OpenDeletionJournal)
        .or_else(|error| missing_directory(error, directory))?
    else {
        return Ok(DeletionLog::default());
    };
    check_entry(&native::entry_info(&file)?, false)?;
    let bytes = read_bounded(&file)?;
    if bytes.is_empty() {
        return Ok(DeletionLog::default());
    }
    Ok(DeletionLog::decode(&bytes, 0)?)
}

fn missing_directory(
    error: LocationError,
    directory: &Path,
) -> Result<Option<File>, LocationError> {
    // Before the first start there is no directory, and so no clear to honor.
    match native::optional_directory(directory)? {
        None => Ok(None),
        Some(_) => Err(error),
    }
}

fn read_bounded(file: &File) -> Result<Vec<u8>, LocationError> {
    let mut reader = file;
    let mut bytes = Vec::new();
    reader
        .seek(SeekFrom::Start(0))
        .and_then(|_| reader.take(READ_LIMIT).read_to_end(&mut bytes))
        .map_err(|error| native::io_error(LocationOperation::ReadDeletionJournal, error))?;
    Ok(bytes)
}

fn persist(file: &File, offset: u64, bytes: &[u8]) -> Result<(), LocationError> {
    // The record is flushed before the caller may act on it (request before deleting).
    let mut writer = file;
    writer
        .seek(SeekFrom::Start(offset))
        .and_then(|_| writer.write_all(bytes))
        .map_err(|error| native::io_error(LocationOperation::WriteDeletionJournal, error))?;
    file.sync_all().map_err(|error: io::Error| {
        native::io_error(LocationOperation::FlushDeletionJournal, error)
    })
}

#[cfg(test)]
mod tests;

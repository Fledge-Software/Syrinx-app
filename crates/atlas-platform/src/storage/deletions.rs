//! Keeps D47's deletion journal beside the database, outside every copy a restore can replace.

use std::{
    error::Error,
    fmt,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use atlas_store::{
    DirectoryLease, LocationError, LocationOperation,
    deletion::{
        DELETION_RECORD_BYTES, DeletionCategory, DeletionError, DeletionLog, DeletionRange,
        DeletionRecord, EMPTY_DELETION_LOG,
    },
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
    ClockUnavailable,
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The owner reads these; structural detail stays in `Debug`.
        formatter.write_str(match self {
            Self::Location(_) => "Atlas could not read or write its record of cleared history",
            Self::Deletion(DeletionError::MissingGeneration) => {
                "Atlas's record of cleared history is missing entries"
            }
            Self::Deletion(DeletionError::InvalidRange) => "That time range is empty",
            Self::Deletion(
                DeletionError::PendingClear
                | DeletionError::NoPendingClear
                | DeletionError::CompletionMismatch,
            ) => "Another history clear has not finished; restart Atlas to finish it",
            Self::Deletion(_) => "Atlas's record of cleared history is damaged",
            Self::ClockUnavailable => "The system clock is unavailable",
            Self::Poisoned => {
                "Atlas could not finish writing its record of cleared history; restart Atlas to finish the clear"
            }
        })
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
    /// A torn final append is cut off, since nothing acted on it; a damaged complete record
    /// is refused. The caller still compares the result with the database's generation.
    pub fn open(lease: &WindowsDirectoryLease) -> Result<Self, JournalError> {
        let (entry, existed) = exclusive_entry(lease)?;
        let mut bytes = read_bounded(&entry.file)?;
        if EMPTY_DELETION_LOG.starts_with(&bytes) {
            // A new file, or one torn while writing its header, holds no record.
            persist(&entry.file, 0, EMPTY_DELETION_LOG)?;
            bytes = EMPTY_DELETION_LOG.to_vec();
        }
        let complete = complete_length(&bytes);
        if complete < bytes.len() {
            truncate(&entry.file, complete)?;
            bytes.truncate(complete);
        }
        if !existed {
            flush_parent(&entry.path)?;
        }
        let log = DeletionLog::decode(&bytes, 0)?;
        Ok(Self {
            entry,
            log,
            poisoned: false,
        })
    }

    /// Replaces an unusable journal with `generation` completed all-time clears.
    ///
    /// Only for an owner-confirmed repair: every restore then clears all history, so no
    /// backup can bring back what the lost records covered. The old bytes are kept beside it.
    pub fn rebuild(lease: &WindowsDirectoryLease, generation: u64) -> Result<Self, JournalError> {
        let (entry, _) = exclusive_entry(lease)?;
        let damaged = read_bounded(&entry.file)?;
        if !damaged.is_empty() {
            preserve_damaged(&entry.path, &damaged)?;
        }
        let mut log = DeletionLog::default();
        let mut bytes = EMPTY_DELETION_LOG.to_vec();
        for _ in 0..generation {
            let request = log.request(DeletionCategory::History, DeletionRange::all_time())?;
            log.accept(request)?;
            let completion = log.completion()?;
            log.accept(completion)?;
            bytes.extend_from_slice(&request.encode());
            bytes.extend_from_slice(&completion.encode());
        }
        truncate(&entry.file, 0)?;
        persist(&entry.file, 0, &bytes)?;
        flush_parent(&entry.path)?;
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
            // A torn tail is cut off at the next start; never append after it now.
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
    if EMPTY_DELETION_LOG.starts_with(&bytes) {
        return Ok(DeletionLog::default());
    }
    // A torn final append never took effect; the next session open cuts it off.
    Ok(DeletionLog::decode(&bytes[..complete_length(&bytes)], 0)?)
}

/// Opens the journal exclusively after inspecting any existing entry without following links.
fn exclusive_entry(lease: &WindowsDirectoryLease) -> Result<(GuardedEntry, bool), JournalError> {
    lease.revalidate()?;
    let directory = lease
        .database
        .path
        .parent()
        .ok_or(LocationError::InvalidPath)?;
    let path = directory.join(JOURNAL_NAME);
    let existed = match native::optional_file(&path, LocationOperation::OpenDeletionJournal)? {
        Some(file) => {
            check_entry(&native::entry_info(&file)?, false)?;
            true
        }
        None => false,
    };
    let file = native::open_journal(&path)?;
    Ok((GuardedEntry::regular_file(path, file)?, existed))
}

/// The length of the header plus every whole record; anything after is a torn append.
fn complete_length(bytes: &[u8]) -> usize {
    let Some(records) = bytes.len().checked_sub(EMPTY_DELETION_LOG.len()) else {
        return bytes.len();
    };
    bytes.len() - records % DELETION_RECORD_BYTES
}

fn preserve_damaged(journal: &Path, bytes: &[u8]) -> Result<(), JournalError> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| JournalError::ClockUnavailable)?
        .as_nanos();
    let mut name = journal.as_os_str().to_owned();
    name.push(format!(".damaged-{stamp:032x}"));
    let copy = native::create_new_file(
        Path::new(&name),
        LocationOperation::WriteDeletionJournal,
        false,
    )?;
    persist(&copy, 0, bytes)?;
    Ok(())
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

fn flush_parent(path: &Path) -> Result<(), LocationError> {
    let directory = path.parent().ok_or(LocationError::InvalidPath)?;
    native::flush_directory(directory, LocationOperation::FlushDeletionJournal)
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

fn truncate(file: &File, length: usize) -> Result<(), LocationError> {
    file.set_len(length as u64)
        .and_then(|()| file.sync_all())
        .map_err(|error| native::io_error(LocationOperation::WriteDeletionJournal, error))
}

fn persist(file: &File, offset: u64, bytes: &[u8]) -> Result<(), LocationError> {
    // The record is flushed before the caller may act on it (request before deleting).
    let mut writer = file;
    writer
        .seek(SeekFrom::Start(offset))
        .and_then(|_| writer.write_all(bytes))
        .map_err(|error| native::io_error(LocationOperation::WriteDeletionJournal, error))?;
    file.sync_all()
        .map_err(|error| native::io_error(LocationOperation::FlushDeletionJournal, error))
}

#[cfg(test)]
mod tests;

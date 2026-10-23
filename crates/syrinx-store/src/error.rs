//! Reports storage failures without retaining paths, SQL text or record contents.

use std::{error::Error, fmt};

use syrinx_core::SnapshotError;

use crate::LocationError;
use crate::deletion::DeletionError;

/// The operation that failed, independent of its SQL or file path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreOperation {
    Open,
    Configure,
    InspectSchema,
    QuickCheck,
    IntegrityCheck,
    Backup,
    Migrate,
    BeginRead,
    BeginWrite,
    Read,
    Write,
    Commit,
    Rollback,
    Optimize,
    Close,
    StartWorker,
}

/// The kind of persisted field whose representation is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodingField {
    TabId,
    SavedPageId,
    ActiveTabId,
    Revision,
    TabWatermark,
    SavedPageWatermark,
    SavedFolderId,
    SavedFolderWatermark,
    SavedLayout,
    SavedOrder,
    LiveOrder,
    TabKind,
    Metadata,
    SessionState,
    Setting,
    Destination,
    History,
    Permission,
    SiteZoom,
}

/// All diagnostic fields are structural; SQLite's message strings are discarded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreError {
    Location(LocationError),
    Database {
        operation: StoreOperation,
        code: Option<i32>,
    },
    WorkerStart {
        code: Option<i32>,
    },
    ForeignDatabase,
    FutureSchema {
        version: i64,
    },
    SchemaMismatch,
    IntegrityCheckFailed,
    BackupUnavailable,
    BackupVerificationFailed,
    InvalidBackupPath,
    /// The deletion journal cannot account for this database's applied clears (D47).
    Deletion(DeletionError),
    /// A database's recorded clear generation differs from its backup's completion record.
    DeletionGenerationMismatch,
    InvalidEncoding(EncodingField),
    InvalidSnapshot(SnapshotError),
    StaleRevision {
        expected: u64,
        actual: u64,
    },
    WatermarkRegression,
    RevisionExhausted,
    TooManyRecords,
    WorkerUnavailable,
    WorkerPanicked,
    ReplyAbandoned,
    ReaderPoisoned,
    Closed,
    CleanupFailed {
        primary: Box<StoreError>,
        cleanup: Box<StoreError>,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Even compound failures contain no original database or OS messages.
        write!(formatter, "storage operation failed: {self:?}")
    }
}

impl Error for StoreError {}

impl From<LocationError> for StoreError {
    fn from(error: LocationError) -> Self {
        Self::Location(error)
    }
}

impl From<DeletionError> for StoreError {
    fn from(error: DeletionError) -> Self {
        Self::Deletion(error)
    }
}

impl From<SnapshotError> for StoreError {
    fn from(error: SnapshotError) -> Self {
        Self::InvalidSnapshot(error)
    }
}

pub(crate) fn database_error(operation: StoreOperation, error: rusqlite::Error) -> StoreError {
    // Keep the SQLite extended code while discarding its content-bearing message.
    StoreError::Database {
        operation,
        code: error.sqlite_error().map(|error| error.extended_code),
    }
}

pub(crate) fn with_cleanup(primary: StoreError, cleanup: Result<(), StoreError>) -> StoreError {
    match cleanup {
        Ok(()) => primary,
        Err(cleanup) => StoreError::CleanupFailed {
            primary: Box::new(primary),
            cleanup: Box::new(cleanup),
        },
    }
}

pub(crate) fn combine_results(
    first: Result<(), StoreError>,
    second: Result<(), StoreError>,
) -> Result<(), StoreError> {
    match first {
        Ok(()) => second,
        Err(primary) => Err(with_cleanup(primary, second)),
    }
}

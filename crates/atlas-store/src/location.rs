//! Keeps the storage-location contract independent of Windows bindings.

use std::{error::Error, fmt, path::Path, sync::Arc};

/// Keeps a checked database location alive until every connection has closed.
pub trait DirectoryLease: Send + Sync {
    fn database_path(&self) -> &Path;
    fn revalidate(&self) -> Result<(), LocationError>;

    /// Supplies exclusive session tracking when the platform can prove it.
    fn session_control(&self) -> Option<&dyn SessionControl> {
        None
    }

    /// Supplies snapshot storage within the same guarded local data location.
    fn backup_archive(&self) -> Option<Arc<dyn crate::BackupArchive>> {
        None
    }
}

/// Claims a session and records it as running before SQLite can open any file.
pub trait SessionControl: Send + Sync {
    fn begin(&self) -> Result<Box<dyn StorageSession>, LocationError>;
}

/// Keeps the session claimed until all database connections have closed.
pub trait StorageSession: Send + Sync {
    fn previous_shutdown(&self) -> ShutdownState;

    /// Records a clean exit only after the caller's complete, successful shutdown.
    /// Dropping without completion must release ownership without recording clean status.
    fn complete(self: Box<Self>) -> Result<(), LocationError>;
}

/// A missing or malformed lifecycle record supplies no proof of a clean exit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownState {
    Clean,
    Unclean,
}

/// Identifies a failed check without exposing the owner's paths or providers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocationOperation {
    DiscoverLocalData,
    StartSyncRootCheck,
    InitializeRuntime,
    EnumerateSyncRoots,
    OpenDirectory,
    OpenDatabase,
    OpenSidecar,
    CreateDirectory,
    CreateDatabase,
    InspectEntry,
    ResolvePath,
    OpenControl,
    ReadControl,
    WriteControl,
    FlushControl,
    LockDirectory,
    OpenRecoveryIntent,
    EnumerateBackups,
    CreateBackup,
    ReadBackup,
    FlushBackup,
    PublishBackup,
    RemoveBackup,
    PreserveDatabase,
    InstallDatabase,
}

/// Reports structural failures; native error messages may contain private data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocationError {
    InvalidPath,
    PathTooLong,
    NonLocalDrive,
    SyncedDirectory,
    ReparsePoint,
    UnexpectedEntryKind,
    DeletePending,
    HardLinkedFile,
    IdentityChanged,
    SyncRootCheckPanicked,
    StorageBusy,
    SessionAlreadyActive,
    ControlPoisoned,
    RecoveryPending,
    InvalidBackup,
    BackupStatePoisoned,
    BackupIdentityExhausted,
    Os {
        operation: LocationOperation,
        code: Option<i32>,
    },
}

impl fmt::Display for LocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Error variants contain only structural categories and numeric codes.
        write!(formatter, "storage location check failed: {self:?}")
    }
}

impl Error for LocationError {}

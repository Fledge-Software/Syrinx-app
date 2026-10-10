//! Keeps the storage-location contract independent of Windows bindings.

use std::{error::Error, fmt, path::Path};

/// Keeps a checked database location alive until every connection has closed.
pub trait DirectoryLease: Send + Sync {
    fn database_path(&self) -> &Path;
    fn revalidate(&self) -> Result<(), LocationError>;
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

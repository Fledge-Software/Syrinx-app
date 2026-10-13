//! Keeps backup filesystem ownership at the platform boundary.

use std::path::Path;

use crate::LocationError;

/// An opaque archive-local identity, unrelated to any saved address or file path.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BackupId(pub u64);

/// Completion evidence written only after the snapshot passes full validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupMetadata {
    pub id: BackupId,
    pub created_at: i64,
    pub revision: u64,
    pub deletion_generation: u64,
}

/// A guarded fixed-name file; implementations must refuse aliases and unexpected sidecars.
pub trait BackupFile: Send + Sync {
    fn id(&self) -> BackupId;
    fn database_path(&self) -> &Path;
    fn revalidate(&self) -> Result<(), LocationError>;
    fn flush(&self) -> Result<(), LocationError>;

    /// Publishes a fresh completion record; never overwrites an existing record.
    fn publish(&self, metadata: BackupMetadata) -> Result<(), LocationError>;

    /// Removes only this recognized bundle after rechecking its retained identities.
    fn remove(self: Box<Self>) -> Result<(), LocationError>;
}

/// A candidate whose completion record parsed successfully; SQLite validation is still required.
pub struct BackupEntry {
    pub metadata: BackupMetadata,
    pub file: Box<dyn BackupFile>,
}

/// Supplies fresh destinations and recognized candidates without opening SQLite.
pub trait BackupArchive: Send + Sync {
    fn reserve(&self) -> Result<Box<dyn BackupFile>, LocationError>;
    fn candidates(&self) -> Result<Vec<BackupEntry>, LocationError>;
}

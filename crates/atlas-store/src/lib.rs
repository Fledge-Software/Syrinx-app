//! Persists Atlas state through a checked storage location and a serialized writer.

mod backup;
mod backup_location;
pub mod deletion;
mod engine;
mod error;
mod location;
mod permissions;
mod records;
pub use permissions::PermissionKind;
mod schema;

pub use backup::{BackupOutcome, validate_backup, validate_database};
pub use backup_location::{BackupArchive, BackupEntry, BackupFile, BackupId, BackupMetadata};
pub use engine::{Store, StoredSnapshot};
pub use error::{EncodingField, StoreError, StoreOperation};
pub use location::{
    DirectoryLease, LocationError, LocationOperation, SessionControl, ShutdownState, StorageSession,
};

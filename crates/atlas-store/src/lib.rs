//! Persists Atlas state through a checked storage location and a serialized writer.

mod backup;
mod backup_location;
pub mod deletion;
mod engine;
mod error;
mod history;
mod location;
mod permissions;
mod preferences;
mod records;
mod replay;
mod schema;

pub use backup::{BackupOutcome, validate_backup, validate_database};
pub use backup_location::{BackupArchive, BackupEntry, BackupFile, BackupId, BackupMetadata};
pub use engine::{Store, StoredSnapshot};
pub use error::{EncodingField, StoreError, StoreOperation};
pub use history::{HistoryDeletion, HistoryEntry, HistoryVisit, RecordedVisit};
pub use location::{
    DirectoryLease, LocationError, LocationOperation, SessionControl, ShutdownState, StorageSession,
};
pub use permissions::PermissionKind;
pub use preferences::{MAX_SETTING_KEY_BYTES, MAX_SETTING_VALUE_BYTES, StoredPreferences};
pub use replay::replay_history_deletions;

//! Persists Syrinx state through a checked storage location and a serialized writer.

mod backup;
mod backup_location;
pub mod deletion;
mod engine;
mod error;
mod history;
mod location;
mod permissions;
mod plugins;
mod preferences;
mod records;
mod replay;
mod schema;
mod site_zoom;

/// SQLite's file layer: Windows needs the long-path VFS (see the storage traps); Unix uses the default.
#[cfg(windows)]
const SQLITE_VFS: &str = "win32-longpath";
#[cfg(unix)]
const SQLITE_VFS: &str = "unix";

pub use backup::{BackupOutcome, validate_backup, validate_database};
pub use backup_location::{BackupArchive, BackupEntry, BackupFile, BackupId, BackupMetadata};
pub use engine::{Store, StoredSnapshot};
pub use error::{EncodingField, StoreError, StoreOperation};
pub use history::{HistoryDeletion, HistoryEntry, HistoryVisit, RecordedVisit};
pub use location::{
    DirectoryLease, LocationError, LocationOperation, SessionControl, ShutdownState, StorageSession,
};
pub use permissions::{PermissionKind, RememberedPermission};
pub use plugins::{
    FolderAccess, MAX_PLUGIN_SETTING_KEY_BYTES, MAX_PLUGIN_SETTING_VALUE_BYTES,
    MAX_PLUGIN_SETTINGS, PluginFolder, PluginGrant, PluginInstall, PluginState, StoredGrant,
};
pub use preferences::{MAX_SETTING_KEY_BYTES, MAX_SETTING_VALUE_BYTES, StoredPreferences};
pub use replay::{AppliedDeletions, replay_deletions};
pub use site_zoom::{MAX_SITE_ZOOM, MIN_SITE_ZOOM, is_default_zoom};

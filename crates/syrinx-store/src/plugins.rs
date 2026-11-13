//! Stores what the owner installed and granted each plugin, and each plugin's own settings,
//! keyed by plugin id in Syrinx's database, never in the plugin's files (#136, decision 9).

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{EncodingField, StoreError, StoreOperation, error::database_error};

/// One row per installed plugin. Removing it removes everything below by cascade.
pub(crate) const INSTALL_TABLE: &str = "CREATE TABLE plugin_install (
    plugin_id TEXT PRIMARY KEY NOT NULL CHECK(length(plugin_id) BETWEEN 1 AND 64),
    version TEXT NOT NULL CHECK(length(version) BETWEEN 1 AND 64),
    module_sha256 BLOB NOT NULL CHECK(length(module_sha256) = 32),
    manifest_sha256 BLOB NOT NULL CHECK(length(manifest_sha256) = 32),
    signer_key TEXT NOT NULL CHECK(length(signer_key) BETWEEN 1 AND 256),
    state INTEGER NOT NULL CHECK(state BETWEEN 1 AND 3),
    installed_at INTEGER NOT NULL CHECK(installed_at >= 0),
    updated_at INTEGER NOT NULL CHECK(updated_at >= installed_at)
) STRICT";

/// One row per granted capability. No history is kept: D39 rejects timelines.
pub(crate) const GRANT_TABLE: &str = "CREATE TABLE plugin_grant (
    grant_id INTEGER PRIMARY KEY NOT NULL,
    plugin_id TEXT NOT NULL REFERENCES plugin_install(plugin_id) ON DELETE CASCADE,
    capability TEXT NOT NULL CHECK(length(capability) BETWEEN 1 AND 32),
    scope_json TEXT NOT NULL CHECK(length(scope_json) <= 4096 AND json_valid(scope_json)),
    granted_at INTEGER NOT NULL CHECK(granted_at >= 0),
    granted_for_manifest BLOB NOT NULL CHECK(length(granted_for_manifest) = 32),
    UNIQUE(plugin_id, capability),
    UNIQUE(grant_id, plugin_id)
) STRICT";

/// Folder paths live apart from scopes, so a path never sits in a scope string. A folder
/// belongs to one grant of the same plugin and goes with it.
pub(crate) const FOLDER_TABLE: &str = "CREATE TABLE plugin_folder (
    plugin_id TEXT NOT NULL,
    grant_id INTEGER NOT NULL,
    path TEXT NOT NULL CHECK(length(path) BETWEEN 1 AND 32767),
    access INTEGER NOT NULL CHECK(access IN (1, 2)),
    PRIMARY KEY(grant_id, path),
    FOREIGN KEY(grant_id, plugin_id) REFERENCES plugin_grant(grant_id, plugin_id) ON DELETE CASCADE
) STRICT";

/// Each plugin's own namespace; values are opaque bytes.
pub(crate) const SETTING_TABLE: &str = "CREATE TABLE plugin_setting (
    plugin_id TEXT NOT NULL REFERENCES plugin_install(plugin_id) ON DELETE CASCADE,
    key TEXT NOT NULL CHECK(length(key) BETWEEN 1 AND 64),
    value BLOB NOT NULL CHECK(length(value) <= 65536),
    PRIMARY KEY(plugin_id, key)
) STRICT";

/// The largest settings key and value one plugin may store, and how many keys it may hold.
pub const MAX_PLUGIN_SETTING_KEY_BYTES: usize = 64;
pub const MAX_PLUGIN_SETTING_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_PLUGIN_SETTINGS: usize = 256;

/// Whether an installed plugin may run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i64)]
pub enum PluginState {
    Enabled = 1,
    Disabled,
    /// An update that widens a capability waits, inactive, for the owner (note 10, section 3).
    UpdateHeld,
}

impl PluginState {
    const ALL: [Self; 3] = [Self::Enabled, Self::Disabled, Self::UpdateHeld];

    fn from_stored(value: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|state| *state as i64 == value)
    }
}

/// What Syrinx recorded when the owner installed or updated a plugin. Times are Unix seconds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginInstall {
    pub plugin_id: String,
    pub version: String,
    pub module_sha256: [u8; 32],
    pub manifest_sha256: [u8; 32],
    pub signer_key: String,
    pub state: PluginState,
    pub installed_at: i64,
    pub updated_at: i64,
}

/// One capability the owner granted. `capability` is the manifest's kind name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginGrant {
    pub capability: String,
    /// The grant's scope as JSON; never a path, which lives in `PluginFolder`.
    pub scope_json: String,
    pub granted_at: i64,
    /// The manifest the owner reviewed when granting it.
    pub granted_for_manifest: [u8; 32],
}

/// A stored grant and its row id, which folder grants refer to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredGrant {
    pub grant_id: i64,
    pub plugin_id: String,
    pub grant: PluginGrant,
}

/// What a plugin may do inside a granted folder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i64)]
pub enum FolderAccess {
    Read = 1,
    ReadWrite,
}

impl FolderAccess {
    fn from_stored(value: i64) -> Option<Self> {
        [Self::Read, Self::ReadWrite]
            .into_iter()
            .find(|access| *access as i64 == value)
    }
}

/// A folder the owner granted under one grant. The path is owner data.
#[derive(Clone, Eq, PartialEq)]
pub struct PluginFolder {
    pub grant_id: i64,
    pub path: PathBuf,
    pub access: FolderAccess,
}

impl fmt::Debug for PluginFolder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Folder paths never enter diagnostics (D39).
        formatter
            .debug_struct("PluginFolder")
            .field("grant_id", &self.grant_id)
            .field("access", &self.access)
            .finish_non_exhaustive()
    }
}

/// One change to plugin records, serialized with every other durable write.
pub(crate) enum PluginWrite {
    Install(PluginInstall),
    Grant {
        plugin_id: String,
        grant: PluginGrant,
    },
    Revoke {
        plugin_id: String,
        capability: String,
    },
    Folder {
        plugin_id: String,
        folder: PluginFolder,
    },
    Setting {
        plugin_id: String,
        key: String,
        value: Vec<u8>,
    },
    Remove {
        plugin_id: String,
    },
}

/// What a plugin write reports: a grant's row id, or whether a row was there to remove.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PluginWritten {
    Saved,
    Grant(i64),
    Removed(bool),
}

fn invalid() -> StoreError {
    StoreError::InvalidEncoding(EncodingField::Plugin)
}

fn read_error(error: rusqlite::Error) -> StoreError {
    database_error(StoreOperation::Read, error)
}

fn write_error(error: rusqlite::Error) -> StoreError {
    database_error(StoreOperation::Write, error)
}

/// Plugin ids and capability names are short lowercase ASCII words; anything else is refused.
fn validate_name(name: &str, longest: usize) -> Result<(), StoreError> {
    let allowed = |ch: char| ch.is_ascii_lowercase() || ch.is_ascii_digit() || "-._".contains(ch);
    if name.is_empty() || name.len() > longest || !name.chars().all(allowed) {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn validate_plugin_id(plugin_id: &str) -> Result<(), StoreError> {
    validate_name(plugin_id, 64)
}

fn validate_install(install: &PluginInstall) -> Result<(), StoreError> {
    validate_plugin_id(&install.plugin_id)?;
    let text_ok = |text: &str, longest: usize| !text.is_empty() && text.len() <= longest;
    let times_ok = install.installed_at >= 0 && install.updated_at >= install.installed_at;
    if !text_ok(&install.version, 64) || !text_ok(&install.signer_key, 256) || !times_ok {
        return Err(invalid());
    }
    Ok(())
}

fn validate_grant(grant: &PluginGrant) -> Result<(), StoreError> {
    validate_name(&grant.capability, 32)?;
    if grant.scope_json.len() > 4096 || grant.granted_at < 0 {
        return Err(invalid());
    }
    Ok(())
}

/// A folder path is stored as absolute UTF-8 text; any other path is refused, never altered.
fn folder_text(path: &Path) -> Result<&str, StoreError> {
    let text = path.to_str().ok_or_else(invalid)?;
    if !path.is_absolute() || text.len() > 32767 {
        return Err(invalid());
    }
    Ok(text)
}

pub(crate) fn validate_setting(key: &str, value: &[u8]) -> Result<(), StoreError> {
    let key_ok = !key.is_empty() && key.len() <= MAX_PLUGIN_SETTING_KEY_BYTES;
    if !key_ok || value.len() > MAX_PLUGIN_SETTING_VALUE_BYTES {
        return Err(invalid());
    }
    Ok(())
}

/// Checks a write before it is queued, so a refused value never reaches the writer.
pub(crate) fn validate_write(write: &PluginWrite) -> Result<(), StoreError> {
    match write {
        PluginWrite::Install(install) => validate_install(install),
        PluginWrite::Grant { plugin_id, grant } => {
            validate_plugin_id(plugin_id)?;
            validate_grant(grant)
        }
        PluginWrite::Revoke {
            plugin_id,
            capability,
        } => {
            validate_plugin_id(plugin_id)?;
            validate_name(capability, 32)
        }
        PluginWrite::Folder { plugin_id, folder } => {
            validate_plugin_id(plugin_id)?;
            folder_text(&folder.path).map(drop)
        }
        PluginWrite::Setting {
            plugin_id,
            key,
            value,
        } => {
            validate_plugin_id(plugin_id)?;
            validate_setting(key, value)
        }
        PluginWrite::Remove { plugin_id } => validate_plugin_id(plugin_id),
    }
}

/// Applies one checked write on the writer connection.
pub(crate) fn write(
    connection: &Connection,
    write: &PluginWrite,
) -> Result<PluginWritten, StoreError> {
    validate_write(write)?;
    match write {
        PluginWrite::Install(install) => save_install(connection, install),
        PluginWrite::Grant { plugin_id, grant } => save_grant(connection, plugin_id, grant),
        PluginWrite::Revoke {
            plugin_id,
            capability,
        } => {
            let removed = connection
                .execute(
                    "DELETE FROM plugin_grant WHERE plugin_id = ?1 AND capability = ?2",
                    params![plugin_id, capability],
                )
                .map_err(write_error)?;
            Ok(PluginWritten::Removed(removed > 0))
        }
        PluginWrite::Folder { plugin_id, folder } => save_folder(connection, plugin_id, folder),
        PluginWrite::Setting {
            plugin_id,
            key,
            value,
        } => save_setting(connection, plugin_id, key, value),
        PluginWrite::Remove { plugin_id } => {
            // Grants, folders and settings go with the install row by cascade.
            let removed = connection
                .execute(
                    "DELETE FROM plugin_install WHERE plugin_id = ?1",
                    params![plugin_id],
                )
                .map_err(write_error)?;
            Ok(PluginWritten::Removed(removed > 0))
        }
    }
}

fn save_install(
    connection: &Connection,
    install: &PluginInstall,
) -> Result<PluginWritten, StoreError> {
    connection
        .execute(
            "INSERT INTO plugin_install(plugin_id, version, module_sha256, manifest_sha256,
                 signer_key, state, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(plugin_id) DO UPDATE SET
                 version = excluded.version, module_sha256 = excluded.module_sha256,
                 manifest_sha256 = excluded.manifest_sha256, signer_key = excluded.signer_key,
                 state = excluded.state, updated_at = excluded.updated_at",
            params![
                install.plugin_id,
                install.version,
                install.module_sha256,
                install.manifest_sha256,
                install.signer_key,
                install.state as i64,
                install.installed_at,
                install.updated_at,
            ],
        )
        .map_err(write_error)?;
    Ok(PluginWritten::Saved)
}

/// Grants or re-grants a capability; a re-grant keeps its row id, so its folders stay.
fn save_grant(
    connection: &Connection,
    plugin_id: &str,
    grant: &PluginGrant,
) -> Result<PluginWritten, StoreError> {
    let grant_id = connection
        .query_row(
            "INSERT INTO plugin_grant(plugin_id, capability, scope_json, granted_at,
                 granted_for_manifest)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(plugin_id, capability) DO UPDATE SET
                 scope_json = excluded.scope_json, granted_at = excluded.granted_at,
                 granted_for_manifest = excluded.granted_for_manifest
             RETURNING grant_id",
            params![
                plugin_id,
                grant.capability,
                grant.scope_json,
                grant.granted_at,
                grant.granted_for_manifest,
            ],
            |row| row.get(0),
        )
        .map_err(write_error)?;
    Ok(PluginWritten::Grant(grant_id))
}

fn save_folder(
    connection: &Connection,
    plugin_id: &str,
    folder: &PluginFolder,
) -> Result<PluginWritten, StoreError> {
    connection
        .execute(
            "INSERT INTO plugin_folder(plugin_id, grant_id, path, access) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(grant_id, path) DO UPDATE SET access = excluded.access",
            params![
                plugin_id,
                folder.grant_id,
                folder_text(&folder.path)?,
                folder.access as i64
            ],
        )
        .map_err(write_error)?;
    Ok(PluginWritten::Saved)
}

/// Saves one key in the plugin's own namespace; a new key past the limit is refused.
fn save_setting(
    connection: &Connection,
    plugin_id: &str,
    key: &str,
    value: &[u8],
) -> Result<PluginWritten, StoreError> {
    let exists = setting(connection, plugin_id, key)?.is_some();
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM plugin_setting WHERE plugin_id = ?1",
            params![plugin_id],
            |row| row.get(0),
        )
        .map_err(read_error)?;
    if !exists && count >= MAX_PLUGIN_SETTINGS as i64 {
        return Err(StoreError::TooManyRecords);
    }
    connection
        .execute(
            "INSERT INTO plugin_setting(plugin_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(plugin_id, key) DO UPDATE SET value = excluded.value",
            params![plugin_id, key, value],
        )
        .map_err(write_error)?;
    Ok(PluginWritten::Saved)
}

/// One key from one plugin's namespace; no query can name another plugin's.
pub(crate) fn setting(
    connection: &Connection,
    plugin_id: &str,
    key: &str,
) -> Result<Option<Vec<u8>>, StoreError> {
    validate_plugin_id(plugin_id)?;
    validate_setting(key, &[])?;
    connection
        .query_row(
            "SELECT value FROM plugin_setting WHERE plugin_id = ?1 AND key = ?2",
            params![plugin_id, key],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)
}

/// Every installed plugin; one damaged row refuses the whole list rather than guess.
pub(crate) fn installs(connection: &Connection) -> Result<Vec<PluginInstall>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT plugin_id, version, module_sha256, manifest_sha256, signer_key, state,
                 installed_at, updated_at
             FROM plugin_install ORDER BY plugin_id",
        )
        .map_err(read_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .map_err(read_error)?;
    rows.map(|row| {
        let (plugin_id, version, module, manifest, signer_key, state, installed_at, updated_at) =
            row.map_err(read_error)?;
        let install = PluginInstall {
            plugin_id,
            version,
            module_sha256: digest(module)?,
            manifest_sha256: digest(manifest)?,
            signer_key,
            state: PluginState::from_stored(state).ok_or_else(invalid)?,
            installed_at,
            updated_at,
        };
        validate_install(&install)?;
        Ok(install)
    })
    .collect()
}

/// Every grant of every plugin, read at start-up so each call is checked in memory.
pub(crate) fn grants(connection: &Connection) -> Result<Vec<StoredGrant>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT grant_id, plugin_id, capability, scope_json, granted_at, granted_for_manifest
             FROM plugin_grant ORDER BY plugin_id, capability",
        )
        .map_err(read_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Vec<u8>>(5)?,
            ))
        })
        .map_err(read_error)?;
    rows.map(|row| {
        let (grant_id, plugin_id, capability, scope_json, granted_at, manifest) =
            row.map_err(read_error)?;
        let grant = PluginGrant {
            capability,
            scope_json,
            granted_at,
            granted_for_manifest: digest(manifest)?,
        };
        validate_plugin_id(&plugin_id)?;
        validate_grant(&grant)?;
        Ok(StoredGrant {
            grant_id,
            plugin_id,
            grant,
        })
    })
    .collect()
}

/// The folders granted to one plugin.
pub(crate) fn folders(
    connection: &Connection,
    plugin_id: &str,
) -> Result<Vec<PluginFolder>, StoreError> {
    validate_plugin_id(plugin_id)?;
    let mut statement = connection
        .prepare(
            "SELECT grant_id, path, access FROM plugin_folder
             WHERE plugin_id = ?1 ORDER BY grant_id, path",
        )
        .map_err(read_error)?;
    let rows = statement
        .query_map(params![plugin_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(read_error)?;
    rows.map(|row| {
        let (grant_id, path, access) = row.map_err(read_error)?;
        let folder = PluginFolder {
            grant_id,
            path: PathBuf::from(path),
            access: FolderAccess::from_stored(access).ok_or_else(invalid)?,
        };
        folder_text(&folder.path)?;
        Ok(folder)
    })
    .collect()
}

fn digest(bytes: Vec<u8>) -> Result<[u8; 32], StoreError> {
    bytes.try_into().map_err(|_| invalid())
}

/// Counts the rows of every plugin table; the version 8 migration checks that they start empty.
pub(crate) fn count(connection: &Connection) -> Result<i64, StoreError> {
    connection
        .query_row(
            "SELECT (SELECT count(*) FROM plugin_install) + (SELECT count(*) FROM plugin_grant)
                 + (SELECT count(*) FROM plugin_folder) + (SELECT count(*) FROM plugin_setting)",
            [],
            |row| row.get(0),
        )
        .map_err(read_error)
}

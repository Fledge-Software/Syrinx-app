//! Stores explicit site permission choices independently of the tab session revision.

use std::fmt;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{EncodingField, StoreError, StoreOperation, error::database_error};

pub(crate) const TABLE: &str = "CREATE TABLE site_permissions (
    origin TEXT NOT NULL CHECK(length(origin) BETWEEN 1 AND 2048),
    kind INTEGER NOT NULL CHECK(kind BETWEEN 1 AND 12),
    allowed INTEGER NOT NULL CHECK(allowed IN (0, 1)),
    PRIMARY KEY(origin, kind)
) STRICT";

/// Stable Syrinx identifiers, independent of native engine enum values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum PermissionKind {
    Microphone = 1,
    Camera,
    Location,
    Notifications,
    Clipboard,
    Sensors,
    Midi,
    Downloads,
    FileSystem,
    Autoplay,
    Fonts,
    WindowManagement,
}

impl PermissionKind {
    const ALL: [Self; 12] = [
        Self::Microphone,
        Self::Camera,
        Self::Location,
        Self::Notifications,
        Self::Clipboard,
        Self::Sensors,
        Self::Midi,
        Self::Downloads,
        Self::FileSystem,
        Self::Autoplay,
        Self::Fonts,
        Self::WindowManagement,
    ];

    /// The kind a stored identifier names, or `None` for a value Syrinx never writes.
    fn from_stored(value: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| *kind as i32 == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Microphone => "use your microphone",
            Self::Camera => "use your camera",
            Self::Location => "access your location",
            Self::Notifications => "show notifications",
            Self::Clipboard => "read your clipboard",
            Self::Sensors => "access device sensors",
            Self::Midi => "access MIDI devices",
            Self::Downloads => "download multiple files",
            Self::FileSystem => "access files",
            Self::Autoplay => "autoplay media",
            Self::Fonts => "read local fonts",
            Self::WindowManagement => "manage windows",
        }
    }
}

/// One remembered choice, as the settings list shows it.
#[derive(Clone, Eq, PartialEq)]
pub struct RememberedPermission {
    pub origin: String,
    pub kind: PermissionKind,
    pub allowed: bool,
}

impl fmt::Debug for RememberedPermission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The origin is browsing data, so it stays out of logs and panic messages.
        formatter
            .debug_struct("RememberedPermission")
            .field("kind", &self.kind)
            .field("allowed", &self.allowed)
            .finish_non_exhaustive()
    }
}

// Callers supply a URL-parser-produced origin, never a page address or opaque origin.
pub(crate) fn validate_origin(origin: &str) -> Result<(), StoreError> {
    let host = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"));
    if origin.len() > 2048
        || host.is_none_or(|host| host.is_empty() || host.contains(['/', '?', '#', '@']))
        || origin
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(StoreError::InvalidEncoding(EncodingField::Permission));
    }
    Ok(())
}

pub(crate) fn load(
    connection: &Connection,
    origin: &str,
    kind: PermissionKind,
) -> Result<Option<bool>, StoreError> {
    validate_origin(origin)?;
    let value: Option<i32> = connection
        .query_row(
            "SELECT allowed FROM site_permissions WHERE origin = ?1 AND kind = ?2",
            params![origin, kind as i32],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    value.map(decode_allowed).transpose()
}

/// Reads a stored allow/deny flag; anything else is damage, never a grant.
fn decode_allowed(value: i32) -> Result<bool, StoreError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(StoreError::InvalidEncoding(EncodingField::Permission)),
    }
}

/// Every remembered choice, ordered by origin and kind; one damaged row refuses the list.
pub(crate) fn list(connection: &Connection) -> Result<Vec<RememberedPermission>, StoreError> {
    let read_error = |error| database_error(StoreOperation::Read, error);
    let mut statement = connection
        .prepare("SELECT origin, kind, allowed FROM site_permissions ORDER BY origin, kind")
        .map_err(read_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, i32>(2)?,
            ))
        })
        .map_err(read_error)?;
    rows.map(|row| {
        let (origin, kind, allowed) = row.map_err(read_error)?;
        decode_row(origin, kind, allowed)
    })
    .collect()
}

fn decode_row(origin: String, kind: i32, allowed: i32) -> Result<RememberedPermission, StoreError> {
    validate_origin(&origin)?;
    let kind = PermissionKind::from_stored(kind)
        .ok_or(StoreError::InvalidEncoding(EncodingField::Permission))?;
    Ok(RememberedPermission {
        origin,
        kind,
        allowed: decode_allowed(allowed)?,
    })
}

/// Counts remembered choices; the version 6 migration checks that it starts empty.
pub(crate) fn count(connection: &Connection) -> Result<i64, StoreError> {
    connection
        .query_row("SELECT count(*) FROM site_permissions", [], |row| {
            row.get(0)
        })
        .map_err(|error| database_error(StoreOperation::Read, error))
}

pub(crate) fn save(
    connection: &Connection,
    origin: &str,
    kind: PermissionKind,
    allowed: bool,
) -> Result<(), StoreError> {
    validate_origin(origin)?;
    connection
        .execute(
            "INSERT INTO site_permissions(origin, kind, allowed) VALUES (?1, ?2, ?3)
         ON CONFLICT(origin, kind) DO UPDATE SET allowed = excluded.allowed",
            params![origin, kind as i32, allowed],
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    Ok(())
}

/// Removes one remembered choice so the next request asks again; `false` when none was saved.
/// With `expected`, only a choice still saved as that allow/deny value is removed.
pub(crate) fn forget(
    connection: &Connection,
    origin: &str,
    kind: PermissionKind,
    expected: Option<bool>,
) -> Result<bool, StoreError> {
    validate_origin(origin)?;
    let removed = connection
        .execute(
            "DELETE FROM site_permissions
             WHERE origin = ?1 AND kind = ?2 AND (?3 IS NULL OR allowed = ?3)",
            params![origin, kind as i32, expected],
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    Ok(removed == 1)
}

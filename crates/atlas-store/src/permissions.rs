//! Stores explicit site permission choices independently of the tab session revision.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{EncodingField, StoreError, StoreOperation, error::database_error};

pub(crate) const TABLE: &str = "CREATE TABLE site_permissions (
    origin TEXT NOT NULL CHECK(length(origin) BETWEEN 1 AND 2048),
    kind INTEGER NOT NULL CHECK(kind BETWEEN 1 AND 12),
    allowed INTEGER NOT NULL CHECK(allowed IN (0, 1)),
    PRIMARY KEY(origin, kind)
) STRICT";

/// Stable Atlas identifiers, independent of native engine enum values.
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
    match value {
        None => Ok(None),
        Some(0) => Ok(Some(false)),
        Some(1) => Ok(Some(true)),
        _ => Err(StoreError::InvalidEncoding(EncodingField::Permission)),
    }
}

/// Counts remembered choices; the version 6 migration checks that it starts empty.
pub(crate) fn count(connection: &Connection) -> Result<i64, StoreError> {
    connection
        .query_row("SELECT count(*) FROM site_permissions", [], |row| {
            row.get(0)
        })
        .map_err(|error| database_error(StoreOperation::Read, error))
}

// TODO(DW-7): Add remembered-choice removal through P1-29 settings or a P1-12 follow-up.
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

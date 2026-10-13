//! Reads and writes the owner's settings and Quick access/Favorites tiles (schema version 3).

use std::collections::BTreeMap;
use std::fmt;

use rusqlite::{Connection, Row, TransactionBehavior, params};
use syrinx_core::{
    BuiltInPlugin, Destination, DestinationSurface, DestinationTarget, QuickDestinations,
};

use crate::error::{EncodingField, StoreError, StoreOperation, database_error};
use crate::records::finish_transaction;

/// The longest settings key and value the table accepts.
pub const MAX_SETTING_KEY_BYTES: usize = 64;
pub const MAX_SETTING_VALUE_BYTES: usize = 256;

const WEB_TARGET: i64 = 0;
const PLUGIN_TARGET: i64 = 1;

/// Every stored preference; an absent key means the caller's documented default.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct StoredPreferences {
    pub settings: BTreeMap<String, String>,
    pub destinations: QuickDestinations,
}

impl fmt::Debug for StoredPreferences {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Tile labels and addresses are the owner's data; only counts reach diagnostics.
        formatter
            .debug_struct("StoredPreferences")
            .field("settings", &self.settings.len())
            .finish_non_exhaustive()
    }
}

/// A change the writer commits in one transaction.
pub(crate) enum PreferenceWrite {
    Settings(Vec<(String, String)>),
    Destinations {
        surface: DestinationSurface,
        tiles: Vec<Destination>,
    },
}

/// Loads every preference from one read snapshot.
pub(crate) fn load(connection: &mut Connection) -> Result<StoredPreferences, StoreError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| database_error(StoreOperation::BeginRead, error))?;
    let result = read(&transaction);
    finish_transaction(transaction, result)
}

/// Reads and validates preferences inside a caller's transaction.
pub(crate) fn read(connection: &Connection) -> Result<StoredPreferences, StoreError> {
    let settings = read_settings(connection)?;
    let quick_access = read_surface(connection, DestinationSurface::QuickAccess)?;
    let favorites = read_surface(connection, DestinationSurface::Favorites)?;
    let destinations = QuickDestinations::from_lists(quick_access, favorites)
        .map_err(|_| StoreError::InvalidEncoding(EncodingField::Destination))?;
    Ok(StoredPreferences {
        settings,
        destinations,
    })
}

/// Commits one settings or tile change.
pub(crate) fn write(
    connection: &mut Connection,
    change: &PreferenceWrite,
) -> Result<(), StoreError> {
    check_change(change)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| database_error(StoreOperation::BeginWrite, error))?;
    let result = match change {
        PreferenceWrite::Settings(entries) => write_settings(&transaction, entries),
        PreferenceWrite::Destinations { surface, tiles } => {
            write_surface(&transaction, *surface, tiles)
        }
    };
    finish_transaction(transaction, result)
}

/// Refuses a change that the table constraints or the tile model would reject.
fn check_change(change: &PreferenceWrite) -> Result<(), StoreError> {
    match change {
        PreferenceWrite::Settings(entries) => entries
            .iter()
            .try_for_each(|(key, value)| check_setting(key, value)),
        PreferenceWrite::Destinations { surface, tiles } => {
            let (quick, favorites) = match surface {
                DestinationSurface::QuickAccess => (tiles.clone(), Vec::new()),
                DestinationSurface::Favorites => (Vec::new(), tiles.clone()),
            };
            QuickDestinations::from_lists(quick, favorites)
                .map(drop)
                .map_err(|_| StoreError::InvalidEncoding(EncodingField::Destination))
        }
    }
}

fn check_setting(key: &str, value: &str) -> Result<(), StoreError> {
    let key_valid = !key.is_empty() && key.len() <= MAX_SETTING_KEY_BYTES;
    if !key_valid || value.len() > MAX_SETTING_VALUE_BYTES {
        return Err(StoreError::InvalidEncoding(EncodingField::Setting));
    }
    Ok(())
}

fn read_settings(connection: &Connection) -> Result<BTreeMap<String, String>, StoreError> {
    let mut statement = connection
        .prepare_cached("SELECT key, value FROM settings ORDER BY key")
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut settings = BTreeMap::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
    {
        let key = text(row, 0, EncodingField::Setting)?;
        let value = text(row, 1, EncodingField::Setting)?;
        settings.insert(key, value);
    }
    Ok(settings)
}

fn read_surface(
    connection: &Connection,
    surface: DestinationSurface,
) -> Result<Vec<Destination>, StoreError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT position, label, target_kind, target FROM quick_destinations
             WHERE surface = ?1 ORDER BY position",
        )
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut rows = statement
        .query([surface_code(surface)])
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut tiles = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
    {
        // Positions are dense, so a gap means a damaged or hand-edited list.
        let position: i64 = row
            .get(0)
            .map_err(|_| StoreError::InvalidEncoding(EncodingField::Destination))?;
        if usize::try_from(position).ok() != Some(tiles.len()) {
            return Err(StoreError::InvalidEncoding(EncodingField::Destination));
        }
        tiles.push(destination(row)?);
    }
    Ok(tiles)
}

fn destination(row: &Row<'_>) -> Result<Destination, StoreError> {
    let invalid = || StoreError::InvalidEncoding(EncodingField::Destination);
    let label = text(row, 1, EncodingField::Destination)?;
    let kind: i64 = row.get(2).map_err(|_| invalid())?;
    let target = text(row, 3, EncodingField::Destination)?;
    let target = match kind {
        WEB_TARGET => DestinationTarget::Web(target),
        PLUGIN_TARGET => {
            DestinationTarget::Plugin(BuiltInPlugin::from_id(&target).ok_or_else(invalid)?)
        }
        _ => return Err(invalid()),
    };
    // Stored labels are already trimmed; `new` would silently repair one that is not.
    let tile = Destination::new(&label, target).map_err(|_| invalid())?;
    if tile.label != label {
        return Err(invalid());
    }
    Ok(tile)
}

fn write_settings(connection: &Connection, entries: &[(String, String)]) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare_cached("INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    for (key, value) in entries {
        statement
            .execute(params![key, value])
            .map_err(|error| database_error(StoreOperation::Write, error))?;
    }
    Ok(())
}

fn write_surface(
    connection: &Connection,
    surface: DestinationSurface,
    tiles: &[Destination],
) -> Result<(), StoreError> {
    // Replace the whole surface so positions stay dense after moves and removals.
    connection
        .execute(
            "DELETE FROM quick_destinations WHERE surface = ?1",
            [surface_code(surface)],
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    let mut statement = connection
        .prepare_cached("INSERT INTO quick_destinations(surface, position, label, target_kind, target) VALUES (?1, ?2, ?3, ?4, ?5)")
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    for (position, tile) in tiles.iter().enumerate() {
        let position = i64::try_from(position).map_err(|_| StoreError::TooManyRecords)?;
        let (kind, target) = match &tile.target {
            DestinationTarget::Web(address) => (WEB_TARGET, address.as_str()),
            DestinationTarget::Plugin(plugin) => (PLUGIN_TARGET, plugin.id()),
        };
        statement
            .execute(params![
                surface_code(surface),
                position,
                tile.label,
                kind,
                target
            ])
            .map_err(|error| database_error(StoreOperation::Write, error))?;
    }
    Ok(())
}

const fn surface_code(surface: DestinationSurface) -> i64 {
    match surface {
        DestinationSurface::QuickAccess => 0,
        DestinationSurface::Favorites => 1,
    }
}

fn text(row: &Row<'_>, column: usize, field: EncodingField) -> Result<String, StoreError> {
    row.get(column)
        .map_err(|_| StoreError::InvalidEncoding(field))
}

//! Stores each site's zoom level, keyed by host, so it survives new tabs and restarts (#90).

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::deletion::{DeletionCategory, DeletionRequest};
use crate::{EncodingField, StoreError, StoreOperation, error::database_error};

/// Only levels away from 100% are kept; an absent host means 100%. `last_modified` is in
/// Unix seconds, like history visits, so Clear browsing data can remove a time range.
pub(crate) const TABLE: &str = "CREATE TABLE site_zoom (
    host TEXT PRIMARY KEY NOT NULL CHECK(length(host) BETWEEN 1 AND 253),
    factor REAL NOT NULL CHECK(factor BETWEEN 0.25 AND 5.0),
    last_modified INTEGER NOT NULL CHECK(last_modified >= 0)
) STRICT";

/// The smallest and largest factors WebView2 accepts, and Syrinx's own zoom steps span.
pub const MIN_SITE_ZOOM: f64 = 0.25;
pub const MAX_SITE_ZOOM: f64 = 5.0;

/// Whether `factor` is 100% within rounding noise, which is never stored.
pub fn is_default_zoom(factor: f64) -> bool {
    (factor - 1.).abs() < 0.001
}

// Callers supply a URL parser's lowercase host, never an address, path or user name.
pub(crate) fn validate_host(host: &str) -> Result<(), StoreError> {
    let refused = |ch: char| {
        !ch.is_ascii_graphic() || ch.is_ascii_uppercase() || matches!(ch, '/' | '?' | '#' | '@')
    };
    if host.is_empty() || host.len() > 253 || host.chars().any(refused) {
        return Err(invalid());
    }
    Ok(())
}

/// A stored factor must be a finite, non-default level WebView2 can take.
fn validate_factor(factor: f64) -> Result<(), StoreError> {
    if !(MIN_SITE_ZOOM..=MAX_SITE_ZOOM).contains(&factor) || is_default_zoom(factor) {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> StoreError {
    StoreError::InvalidEncoding(EncodingField::SiteZoom)
}

/// Every saved level by host; one damaged row refuses the whole map rather than guess.
pub(crate) fn load(connection: &Connection) -> Result<BTreeMap<String, f64>, StoreError> {
    let read_error = |error| database_error(StoreOperation::Read, error);
    let mut statement = connection
        .prepare("SELECT host, factor, last_modified FROM site_zoom ORDER BY host")
        .map_err(read_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(read_error)?;
    rows.map(|row| {
        let (host, factor, modified_at) = row.map_err(read_error)?;
        validate_host(&host)?;
        validate_factor(factor)?;
        if modified_at < 0 {
            return Err(invalid());
        }
        Ok((host, factor))
    })
    .collect()
}

/// Counts saved levels; the version 7 migration checks that it starts empty.
pub(crate) fn count(connection: &Connection) -> Result<i64, StoreError> {
    connection
        .query_row("SELECT count(*) FROM site_zoom", [], |row| row.get(0))
        .map_err(|error| database_error(StoreOperation::Read, error))
}

/// Saves `host`'s level as changed at `modified_at` Unix seconds, or removes it when
/// `factor` is `None`, so 100% leaves no row.
pub(crate) fn write(
    connection: &Connection,
    host: &str,
    factor: Option<f64>,
    modified_at: i64,
) -> Result<(), StoreError> {
    validate_host(host)?;
    let Some(factor) = factor else {
        connection
            .execute("DELETE FROM site_zoom WHERE host = ?1", params![host])
            .map_err(|error| database_error(StoreOperation::Write, error))?;
        return Ok(());
    };
    validate_factor(factor)?;
    if modified_at < 0 {
        return Err(invalid());
    }
    connection
        .execute(
            "INSERT INTO site_zoom(host, factor, last_modified) VALUES (?1, ?2, ?3)
             ON CONFLICT(host) DO UPDATE SET
                 factor = excluded.factor, last_modified = excluded.last_modified",
            params![host, factor, modified_at],
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    Ok(())
}

/// Removes the levels changed in each `SiteZoom` request's half-open range, as Clear browsing
/// data's "Cookies and site data" asks through D47's journal (#121); requests of other
/// categories are skipped. Returns how many sites were cleared.
pub(crate) fn delete_ranges(
    connection: &Connection,
    requests: &[DeletionRequest],
) -> Result<usize, StoreError> {
    let mut sites = 0;
    for request in requests {
        if request.category() != DeletionCategory::SiteZoom {
            continue;
        }
        let range = request.range();
        sites += clear(connection, range.start(), range.end())?;
    }
    Ok(sites)
}

/// Removes the levels changed in `[start, end)` Unix seconds; returns how many sites were cleared.
fn clear(connection: &Connection, start: i64, end: i64) -> Result<usize, StoreError> {
    connection
        .execute(
            "DELETE FROM site_zoom WHERE last_modified >= ?1 AND last_modified < ?2",
            params![start, end],
        )
        .map_err(|error| database_error(StoreOperation::Write, error))
}

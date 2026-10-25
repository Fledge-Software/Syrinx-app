//! Persists browsing history (P1-20) beside the session, with D47's range deletion participant.

use std::{collections::BTreeSet, fmt, num::NonZeroU64, sync::mpsc};

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::deletion::{DeletionCategory, DeletionRange, DeletionRequest};
use crate::error::{EncodingField, StoreError, StoreOperation, database_error};
use crate::records::finish_transaction;

/// One remembered address with its latest title and visit totals; debug output is redacted.
#[derive(Clone, Eq, PartialEq)]
pub struct HistoryEntry {
    pub id: NonZeroU64,
    pub address: String,
    /// Empty until the page reports a title.
    pub title: String,
    pub visit_count: u64,
    /// Unix seconds of the newest remaining visit.
    pub last_visited: i64,
}

impl fmt::Debug for HistoryEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryEntry")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// One visit with its address's current title; listings return the newest first.
#[derive(Clone, Eq, PartialEq)]
pub struct HistoryVisit {
    pub id: NonZeroU64,
    pub entry_id: NonZeroU64,
    pub visited_at: i64,
    pub address: String,
    pub title: String,
}

impl fmt::Debug for HistoryVisit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryVisit")
            .field("id", &self.id)
            .field("entry_id", &self.entry_id)
            .finish_non_exhaustive()
    }
}

/// A history change performed on the writer thread.
pub(crate) enum HistoryWrite {
    Record {
        address: String,
        title: String,
        visited_at: i64,
    },
    Retitle {
        id: NonZeroU64,
        title: String,
    },
    RemoveVisit(NonZeroU64),
}

/// A newly stored visit and its address's updated totals.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedVisit {
    pub visit: NonZeroU64,
    pub entry: HistoryEntry,
}

/// What a D47 clear changed, so a running search catalogue can follow it.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct HistoryDeletion {
    /// Visits removed, including visits to addresses that keep older ones.
    pub visits: usize,
    /// Addresses left without any visit, now gone from history.
    pub removed: Vec<NonZeroU64>,
    /// Addresses that lost some visits and keep others, with their recomputed totals.
    pub changed: Vec<HistoryEntry>,
}

impl fmt::Debug for HistoryDeletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryDeletion")
            .field("visits", &self.visits)
            .field("removed", &self.removed.len())
            .field("changed", &self.changed.len())
            .finish()
    }
}

/// What a history write produced, by kind of write.
pub(crate) enum HistoryOutcome {
    Recorded(RecordedVisit),
    /// The changed entry, or `None` when it no longer exists.
    Entry(Option<HistoryEntry>),
}

pub(crate) struct HistoryRequest {
    pub(crate) write: HistoryWrite,
    pub(crate) reply: mpsc::SyncSender<Result<HistoryOutcome, StoreError>>,
}

/// Applies one history change in its own write transaction.
pub(crate) fn write(
    connection: &mut Connection,
    change: HistoryWrite,
) -> Result<HistoryOutcome, StoreError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| database_error(StoreOperation::BeginWrite, error))?;
    let result = match change {
        HistoryWrite::Record {
            address,
            title,
            visited_at,
        } => record(&transaction, &address, &title, visited_at).map(HistoryOutcome::Recorded),
        HistoryWrite::Retitle { id, title } => {
            retitle(&transaction, id, &title).map(HistoryOutcome::Entry)
        }
        HistoryWrite::RemoveVisit(visit) => {
            remove_visit(&transaction, visit).map(HistoryOutcome::Entry)
        }
    };
    finish_transaction(transaction, result)
}

fn record(
    connection: &Connection,
    address: &str,
    title: &str,
    visited_at: i64,
) -> Result<RecordedVisit, StoreError> {
    // Refuse values the table would reject, without reporting their contents.
    // i64::MAX is refused too: clear-all's half-open range ends there, so it could not clear it.
    if address.is_empty() || visited_at < 0 || visited_at == i64::MAX {
        return Err(StoreError::InvalidEncoding(EncodingField::History));
    }
    // One row per address; an empty title keeps the title already known for it.
    let columns = connection
        .query_row(
            "INSERT INTO history_urls(address, title, visit_count, last_visited) VALUES (?1, ?2, 1, ?3)
             ON CONFLICT(address) DO UPDATE SET
                 title = CASE WHEN excluded.title = '' THEN title ELSE excluded.title END,
                 visit_count = visit_count + 1,
                 last_visited = max(last_visited, excluded.last_visited)
             RETURNING id, address, title, visit_count, last_visited",
            params![address, title, visited_at],
            entry_columns,
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    let entry = decode_entry(columns)?;
    let visit = connection
        .query_row(
            "INSERT INTO history_visits(url_id, visited_at) VALUES (?1, ?2) RETURNING id",
            params![row_id(entry.id)?, visited_at],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    Ok(RecordedVisit {
        visit: identity(visit)?,
        entry,
    })
}

fn remove_visit(
    connection: &Connection,
    visit: NonZeroU64,
) -> Result<Option<HistoryEntry>, StoreError> {
    // Retract one visit, then drop its address if no visit remains or recompute its totals.
    let url_id: Option<i64> = connection
        .query_row(
            "DELETE FROM history_visits WHERE id = ?1 RETURNING url_id",
            [row_id(visit)?],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    let Some(url_id) = url_id else {
        return Ok(None);
    };
    connection
        .execute(
            "DELETE FROM history_urls WHERE id = ?1 AND NOT EXISTS
                 (SELECT 1 FROM history_visits WHERE url_id = ?1)",
            [url_id],
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    connection
        .query_row(
            "UPDATE history_urls SET
                 visit_count = (SELECT count(*) FROM history_visits WHERE url_id = ?1),
                 last_visited = (SELECT max(visited_at) FROM history_visits WHERE url_id = ?1)
             WHERE id = ?1
             RETURNING id, address, title, visit_count, last_visited",
            [url_id],
            entry_columns,
        )
        .optional()
        .map_err(|error| database_error(StoreOperation::Write, error))?
        .map(decode_entry)
        .transpose()
}

fn retitle(
    connection: &Connection,
    id: NonZeroU64,
    title: &str,
) -> Result<Option<HistoryEntry>, StoreError> {
    // A removed entry stays removed; a late title cannot recreate it.
    connection
        .query_row(
            "UPDATE history_urls SET title = ?2 WHERE id = ?1
             RETURNING id, address, title, visit_count, last_visited",
            params![row_id(id)?, title],
            entry_columns,
        )
        .optional()
        .map_err(|error| database_error(StoreOperation::Write, error))?
        .map(decode_entry)
        .transpose()
}

/// Removes the visits in each `History` request's range inside the caller's transaction and
/// reports which addresses went or changed. The caller records the applied generation.
pub(crate) fn delete(
    connection: &Connection,
    requests: &[DeletionRequest],
) -> Result<HistoryDeletion, StoreError> {
    // Note the touched addresses first, so the caller learns which rows went or changed.
    let affected = affected_entries(connection, requests)?;
    let visits = delete_ranges(connection, requests)?;
    let mut deletion = HistoryDeletion {
        visits,
        ..HistoryDeletion::default()
    };
    for id in affected {
        match entry(connection, id)? {
            Some(entry) => deletion.changed.push(entry),
            None => deletion.removed.push(id),
        }
    }
    Ok(deletion)
}

fn history_ranges(requests: &[DeletionRequest]) -> impl Iterator<Item = DeletionRange> + '_ {
    requests
        .iter()
        .filter(|request| request.category() == DeletionCategory::History)
        .map(|request| request.range())
}

fn affected_entries(
    connection: &Connection,
    requests: &[DeletionRequest],
) -> Result<BTreeSet<NonZeroU64>, StoreError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT DISTINCT url_id FROM history_visits WHERE visited_at >= ?1 AND visited_at < ?2",
        )
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut affected = BTreeSet::new();
    for range in history_ranges(requests) {
        let rows = statement
            .query_map(params![range.start(), range.end()], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| database_error(StoreOperation::Read, error))?;
        for row in rows {
            let id = row.map_err(|error| database_error(StoreOperation::Read, error))?;
            affected.insert(identity(id)?);
        }
    }
    Ok(affected)
}

fn entry(connection: &Connection, id: NonZeroU64) -> Result<Option<HistoryEntry>, StoreError> {
    connection
        .query_row(
            "SELECT id, address, title, visit_count, last_visited FROM history_urls WHERE id = ?1",
            [row_id(id)?],
            entry_columns,
        )
        .optional()
        .map_err(|error| database_error(StoreOperation::Read, error))?
        .map(decode_entry)
        .transpose()
}

/// The newest D47 clear this database has applied (schema version 5 and later).
pub(crate) fn applied_generation(connection: &Connection) -> Result<u64, StoreError> {
    let generation: i64 = connection
        .query_row(
            "SELECT applied_generation FROM history_deletion_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    u64::try_from(generation).map_err(|_| StoreError::InvalidEncoding(EncodingField::History))
}

/// Advances the applied generation; replaying an older request never moves it backwards.
pub(crate) fn record_generation(
    connection: &Connection,
    generation: u64,
) -> Result<(), StoreError> {
    let generation = i64::try_from(generation)
        .map_err(|_| StoreError::InvalidEncoding(EncodingField::History))?;
    let changed = connection
        .execute(
            "UPDATE history_deletion_state SET applied_generation = max(applied_generation, ?1)
             WHERE singleton = 1",
            [generation],
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    if changed != 1 {
        return Err(StoreError::InvalidEncoding(EncodingField::History));
    }
    Ok(())
}

/// Removes visits in each cleared half-open range, then entries left without visits.
/// This is the whole participant for a staged copy, which may predate the generation table.
/// Returns how many visits were removed.
pub(crate) fn delete_ranges(
    connection: &Connection,
    requests: &[DeletionRequest],
) -> Result<usize, StoreError> {
    // A batch of other categories (a zoom clear, #121) leaves every history row untouched.
    let mut ranges = history_ranges(requests).peekable();
    if ranges.peek().is_none() {
        return Ok(0);
    }
    let mut visits = 0;
    for range in ranges {
        visits += connection
            .execute(
                "DELETE FROM history_visits WHERE visited_at >= ?1 AND visited_at < ?2",
                params![range.start(), range.end()],
            )
            .map_err(|error| database_error(StoreOperation::Write, error))?;
    }
    connection
        .execute_batch(
            "DELETE FROM history_urls WHERE NOT EXISTS
                 (SELECT 1 FROM history_visits WHERE url_id = history_urls.id);
             UPDATE history_urls SET
                 visit_count = (SELECT count(*) FROM history_visits WHERE url_id = history_urls.id),
                 last_visited = (SELECT max(visited_at) FROM history_visits WHERE url_id = history_urls.id);",
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    Ok(visits)
}

/// Unix seconds of the newest stored visit, or `None` without history.
pub(crate) fn newest_visit_time(connection: &Connection) -> Result<Option<i64>, StoreError> {
    connection
        .query_row("SELECT max(visited_at) FROM history_visits", [], |row| {
            row.get(0)
        })
        .map_err(|error| database_error(StoreOperation::Read, error))
}

/// The end of the next expiry range `[0, end)`: `cutoff` when at most `limit` visits are
/// older, otherwise the time of the first visit beyond `limit`. A range never ends on the
/// oldest visit's own second, so each expiry removes something.
pub(crate) fn expiry_end(
    connection: &Connection,
    cutoff: i64,
    limit: usize,
) -> Result<Option<i64>, StoreError> {
    let limit = i64::try_from(limit).map_err(|_| StoreError::TooManyRecords)?;
    let oldest: Option<i64> = connection
        .query_row(
            "SELECT min(visited_at) FROM history_visits WHERE visited_at < ?1",
            [cutoff],
            |row| row.get(0),
        )
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let Some(oldest) = oldest else {
        return Ok(None);
    };
    let beyond: Option<i64> = connection
        .query_row(
            "SELECT visited_at FROM history_visits WHERE visited_at < ?1
             ORDER BY visited_at LIMIT 1 OFFSET ?2",
            params![cutoff, limit],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    // `oldest < cutoff`, so `oldest + 1` cannot overflow and never passes `cutoff`.
    Ok(Some(beyond.map_or(cutoff, |time| time.max(oldest + 1))))
}

/// Every remembered address in identity order, for rebuilding the search catalogue.
pub(crate) fn entries(connection: &Connection) -> Result<Vec<HistoryEntry>, StoreError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT id, address, title, visit_count, last_visited FROM history_urls ORDER BY id",
        )
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let rows = statement
        .query_map([], entry_columns)
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    rows.map(|row| {
        row.map_err(|error| database_error(StoreOperation::Read, error))
            .and_then(decode_entry)
    })
    .collect()
}

/// At most `limit` visits, newest recorded first.
pub(crate) fn recent(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<HistoryVisit>, StoreError> {
    let limit = i64::try_from(limit).map_err(|_| StoreError::TooManyRecords)?;
    let mut statement = connection
        .prepare_cached(
            "SELECT v.id, v.url_id, v.visited_at, u.address, u.title
             FROM history_visits AS v JOIN history_urls AS u ON u.id = v.url_id
             ORDER BY v.id DESC LIMIT ?1",
        )
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let rows = statement
        .query_map([limit], visit_columns)
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    rows.map(|row| {
        row.map_err(|error| database_error(StoreOperation::Read, error))
            .and_then(decode_visit)
    })
    .collect()
}

type EntryColumns = (i64, String, String, i64, i64);
type VisitColumns = (i64, i64, i64, String, String);

fn entry_columns(row: &Row<'_>) -> rusqlite::Result<EntryColumns> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn visit_columns(row: &Row<'_>) -> rusqlite::Result<VisitColumns> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn decode_entry(columns: EntryColumns) -> Result<HistoryEntry, StoreError> {
    let (id, address, title, visit_count, last_visited) = columns;
    Ok(HistoryEntry {
        id: identity(id)?,
        address,
        title,
        visit_count: u64::try_from(visit_count)
            .map_err(|_| StoreError::InvalidEncoding(EncodingField::History))?,
        last_visited,
    })
}

fn decode_visit(columns: VisitColumns) -> Result<HistoryVisit, StoreError> {
    let (id, entry_id, visited_at, address, title) = columns;
    Ok(HistoryVisit {
        id: identity(id)?,
        entry_id: identity(entry_id)?,
        visited_at,
        address,
        title,
    })
}

fn identity(value: i64) -> Result<NonZeroU64, StoreError> {
    u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or(StoreError::InvalidEncoding(EncodingField::History))
}

fn row_id(id: NonZeroU64) -> Result<i64, StoreError> {
    i64::try_from(id.get()).map_err(|_| StoreError::InvalidEncoding(EncodingField::History))
}

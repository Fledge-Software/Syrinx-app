//! Applies D47's deletion journal to a database: the live one after a clear or at start-up,
//! and a staged recovery copy before it can become live.

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::deletion::{DeletionLog, DeletionRequest};
use crate::error::{database_error, with_cleanup};
use crate::history::HistoryDeletion;
use crate::records::finish_transaction;
use crate::{DirectoryLease, StoreError, StoreOperation, history, schema, site_zoom};

/// What one batch of journal requests removed from the live database, by category. It holds
/// counts and row identities only, never an address or a host.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AppliedDeletions {
    pub history: HistoryDeletion,
    /// Sites whose saved zoom level was removed (#121).
    pub site_zooms: usize,
}

/// Applies `requests` of every category to the live database in one transaction and records
/// the newest generation among them, so a restore of this copy replays only later clears.
pub(crate) fn apply_live(
    connection: &mut Connection,
    requests: &[DeletionRequest],
) -> Result<AppliedDeletions, StoreError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| database_error(StoreOperation::BeginWrite, error))?;
    let result = apply_requests(&transaction, requests);
    finish_transaction(transaction, result)
}

fn apply_requests(
    connection: &Connection,
    requests: &[DeletionRequest],
) -> Result<AppliedDeletions, StoreError> {
    // Each participant skips the categories it does not own.
    let history = history::delete(connection, requests)?;
    let site_zooms = site_zoom::delete_ranges(connection, requests)?;
    if let Some(generation) = newest_generation(requests) {
        history::record_generation(connection, generation)?;
    }
    Ok(AppliedDeletions {
        history,
        site_zooms,
    })
}

fn newest_generation(requests: &[DeletionRequest]) -> Option<u64> {
    requests.iter().map(|request| request.generation()).max()
}

/// Clears every journal request newer than `recorded_generation`, of each category the copy's
/// schema holds, from a guarded staged copy.
///
/// `recorded_generation` comes from the backup's completion record and must match the copy's
/// own state. A copy older than schema 5 cannot record its generation; the live startup
/// reconcile then replays the journal again, which deletes nothing twice.
pub fn replay_deletions(
    lease: &dyn DirectoryLease,
    log: &DeletionLog,
    recorded_generation: u64,
) -> Result<(), StoreError> {
    lease.revalidate()?;
    let mut connection = Connection::open_with_flags_and_vfs(
        lease.database_path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        "win32-longpath",
    )
    .map_err(|error| database_error(StoreOperation::Open, error))?;
    let result = replay_open(&mut connection, log, recorded_generation).and_then(|()| {
        lease.revalidate()?;
        Ok(())
    });
    let closed = connection.close().map_err(|(connection, error)| {
        let error = database_error(StoreOperation::Close, error);
        drop(connection);
        error
    });
    match result {
        Ok(()) => closed,
        Err(error) => Err(with_cleanup(error, closed)),
    }
}

fn replay_open(
    connection: &mut Connection,
    log: &DeletionLog,
    recorded_generation: u64,
) -> Result<(), StoreError> {
    schema::configure_connection(connection)?;
    let generation = schema::staged_generation(connection)?;

    // The copy's own state must agree with its completion record before anything is trusted.
    let applied = if generation.has_deletion_state() {
        history::applied_generation(connection)?
    } else {
        0
    };
    if applied != recorded_generation {
        return Err(StoreError::DeletionGenerationMismatch);
    }
    let requests = log.unapplied(applied)?;
    if requests.is_empty() || !generation.has_history() {
        return Ok(());
    }

    // A rollback journal, not WAL, keeps the staging bundle to its single database file.
    let mode: String = connection
        .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
        .map_err(|error| database_error(StoreOperation::Configure, error))?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(StoreError::SchemaMismatch);
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| database_error(StoreOperation::BeginWrite, error))?;
    let result = replay_requests(&transaction, generation, requests, log.last_generation());
    finish_transaction(transaction, result)
}

/// Each participant the copy's schema holds takes its own category; a copy that records its
/// generation then notes the journal's newest, so nothing replays twice once it is live.
fn replay_requests(
    connection: &Connection,
    generation: schema::SchemaGeneration,
    requests: &[DeletionRequest],
    last_generation: u64,
) -> Result<(), StoreError> {
    history::delete_ranges(connection, requests)?;
    if generation.has_site_zoom() {
        site_zoom::delete_ranges(connection, requests)?;
    }
    if !generation.has_deletion_state() {
        return Ok(());
    }
    history::record_generation(connection, last_generation)
}

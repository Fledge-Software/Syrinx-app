//! Applies D47's deletion journal to a staged recovery copy before it can become live.

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::deletion::DeletionLog;
use crate::error::{database_error, with_cleanup};
use crate::records::finish_transaction;
use crate::{DirectoryLease, StoreError, StoreOperation, history, schema};

/// Clears every journal request newer than `recorded_generation` from a guarded staged copy.
///
/// `recorded_generation` comes from the backup's completion record and must match the copy's
/// own state. A copy older than schema 5 cannot record its generation; the live startup
/// reconcile then replays the journal again, which deletes nothing twice.
pub fn replay_history_deletions(
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
    let result = history::delete_ranges(&transaction, requests).and_then(|_visits| {
        if !generation.has_deletion_state() {
            return Ok(());
        }
        history::record_generation(&transaction, log.last_generation())
    });
    finish_transaction(transaction, result)
}

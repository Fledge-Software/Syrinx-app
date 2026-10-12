//! Runs D47 history clears through the journal, the store and the running catalogue (P1-27).

use atlas_platform::{DeletionJournal, JournalError};
use atlas_search::{Catalogue, Target};
use atlas_store::{
    HistoryDeletion, Store,
    deletion::{DeletionCategory, DeletionError, DeletionRange},
};

use super::{ClearedHistory, SessionError, history};

/// Visits are kept for 90 days (DW-4, the owner's decision of 2026-10-12).
pub(super) const RETENTION_SECS: i64 = 90 * DAY_SECS;
/// One expiry removes at most this many visits, so start-up never waits on a backlog.
pub(super) const EXPIRY_BATCH: usize = 2_000;
/// Expiry runs at most once a day once any backlog is gone.
pub(super) const DAY_SECS: i64 = 24 * 60 * 60;

/// Brings the opened database level with the journal before anything reads history.
///
/// A restored or older database replays later clears; an interrupted clear is finished.
/// A database ahead of its journal means records were lost, which is refused.
pub(super) fn reconcile(store: &Store, journal: &mut DeletionJournal) -> Result<(), SessionError> {
    let applied = store
        .history_deletion_generation()
        .map_err(SessionError::Store)?;
    let missing = journal.log().unapplied(applied).map_err(journal_error)?;
    if !missing.is_empty() {
        store
            .apply_history_deletions(missing)
            .map_err(SessionError::Store)?;
    }
    complete_pending(journal)
}

/// Clears all history at start-up, after the owner confirmed an unusable journal's repair.
pub(super) fn clear_all(store: &Store, journal: &mut DeletionJournal) -> Result<(), SessionError> {
    clear_before_catalogue(store, journal, DeletionRange::all_time())
}

/// Expires one bounded batch of visits older than the retention, before the catalogue
/// loads. Returns the time the next expiry is due.
pub(super) fn expire_at_startup(
    store: &Store,
    journal: &mut DeletionJournal,
    now: i64,
    limit: usize,
) -> Result<i64, SessionError> {
    let Some(range) = expiry_range(store, now, limit)? else {
        return Ok(next_expiry(now, None));
    };
    clear_before_catalogue(store, journal, range)?;
    Ok(next_expiry(now, Some(range)))
}

/// Expires one bounded batch while the session runs, with the catalogue following.
/// Returns the time the next expiry is due.
pub(super) fn expire_live(
    store: &Store,
    journal: &mut DeletionJournal,
    catalogue: &mut Catalogue,
    recorder: &mut history::HistoryRecorder,
    now: i64,
    limit: usize,
) -> Result<i64, SessionError> {
    let Some(range) = expiry_range(store, now, limit)? else {
        return Ok(next_expiry(now, None));
    };
    clear(
        store,
        journal,
        catalogue,
        recorder,
        range.start(),
        range.end(),
    )?;
    Ok(next_expiry(now, Some(range)))
}

/// The journaled range the next expiry clears, or none when nothing has expired.
fn expiry_range(
    store: &Store,
    now: i64,
    limit: usize,
) -> Result<Option<DeletionRange>, SessionError> {
    let cutoff = now.saturating_sub(RETENTION_SECS);
    let Some(end) = store
        .history_expiry_end(cutoff, limit)
        .map_err(SessionError::Store)?
    else {
        return Ok(None);
    };
    DeletionRange::new(0, end).map(Some).map_err(journal_error)
}

/// A batch that stopped short of the cutoff leaves a backlog for the next idle check;
/// otherwise the next expiry waits a day.
fn next_expiry(now: i64, expired: Option<DeletionRange>) -> i64 {
    let cutoff = now.saturating_sub(RETENTION_SECS);
    match expired {
        Some(range) if range.end() < cutoff => now,
        _ => now.saturating_add(DAY_SECS),
    }
}

/// Runs one clear before the catalogue exists, so only the store and the journal change.
fn clear_before_catalogue(
    store: &Store,
    journal: &mut DeletionJournal,
    range: DeletionRange,
) -> Result<(), SessionError> {
    let request = journal
        .log()
        .request(DeletionCategory::History, range)
        .map_err(journal_error)?;
    journal.append(request).map_err(SessionError::Deletion)?;
    let pending = journal
        .log()
        .pending()
        .ok_or(journal_error(DeletionError::NoPendingClear))?;
    store
        .apply_history_deletions(&[pending])
        .map_err(SessionError::Store)?;
    complete_pending(journal)
}

/// Clears `[start, end)` Unix seconds and reports how much went away.
///
/// The request is durable before anything is deleted and completion is written after both
/// the database and the catalogue follow, so a crash in between finishes at the next start.
pub(super) fn clear(
    store: &Store,
    journal: &mut DeletionJournal,
    catalogue: &mut Catalogue,
    recorder: &mut history::HistoryRecorder,
    start: i64,
    end: i64,
) -> Result<ClearedHistory, SessionError> {
    let range = DeletionRange::new(start, end).map_err(journal_error)?;
    // An earlier clear that stopped part-way is finished before a new one may start.
    let earlier = finish_pending(store, journal, catalogue, recorder)?;
    let request = journal
        .log()
        .request(DeletionCategory::History, range)
        .map_err(journal_error)?;
    journal.append(request).map_err(SessionError::Deletion)?;
    let cleared = finish_pending(store, journal, catalogue, recorder)?;
    Ok(ClearedHistory {
        visits: earlier.visits + cleared.visits,
        addresses: earlier.addresses + cleared.addresses,
    })
}

fn finish_pending(
    store: &Store,
    journal: &mut DeletionJournal,
    catalogue: &mut Catalogue,
    recorder: &mut history::HistoryRecorder,
) -> Result<ClearedHistory, SessionError> {
    let Some(pending) = journal.log().pending() else {
        return Ok(ClearedHistory::default());
    };
    let deletion = store
        .apply_history_deletions(&[pending])
        .map_err(SessionError::Store)?;
    follow(catalogue, recorder, &deletion)?;
    complete_pending(journal)?;
    Ok(ClearedHistory {
        visits: deletion.visits,
        addresses: deletion.removed.len(),
    })
}

/// Removes or recomputes every catalogue row the clear touched, and the recorder's cursors.
fn follow(
    catalogue: &mut Catalogue,
    recorder: &mut history::HistoryRecorder,
    deletion: &HistoryDeletion,
) -> Result<(), SessionError> {
    for id in &deletion.removed {
        catalogue
            .remove(Target::History(*id))
            .map(drop)
            .map_err(SessionError::Search)?;
    }
    for entry in &deletion.changed {
        catalogue
            .upsert(history::history_entry(entry))
            .map_err(SessionError::Search)?;
    }
    recorder.history_cleared(&deletion.removed);
    Ok(())
}

fn complete_pending(journal: &mut DeletionJournal) -> Result<(), SessionError> {
    if journal.log().pending().is_none() {
        return Ok(());
    }
    let completion = journal.log().completion().map_err(journal_error)?;
    journal.append(completion).map_err(SessionError::Deletion)
}

fn journal_error(error: DeletionError) -> SessionError {
    SessionError::Deletion(JournalError::Deletion(error))
}

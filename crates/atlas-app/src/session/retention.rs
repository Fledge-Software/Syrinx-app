//! Expires browsing history older than 90 days through D47's journaled clears (DW-4).

use atlas_platform::DeletionJournal;
use atlas_search::Catalogue;
use atlas_store::{Store, deletion::DeletionRange};

use super::{SessionError, deletions, history};

/// Visits are kept for 90 days (DW-4, the owner's decision of 2026-10-12).
pub(super) const RETENTION_SECS: i64 = 90 * DAY_SECS;
/// One expiry removes at most this many visits, so start-up never waits on a backlog.
pub(super) const EXPIRY_BATCH: usize = 2_000;
/// Expiry runs at most once a day once any backlog is gone.
pub(super) const DAY_SECS: i64 = 24 * 60 * 60;
/// The settings key holding when the next expiry is due, in Unix seconds.
pub(super) const EXPIRY_DUE_KEY: &str = "history.expiry_due";

/// Expires one bounded batch before the catalogue loads, unless an expiry ran within the
/// last day. Returns the time the next expiry is due.
pub(super) fn expire_at_startup(
    store: &Store,
    journal: &mut DeletionJournal,
    now: i64,
    saved_due: Option<&str>,
    limit: usize,
) -> Result<i64, SessionError> {
    if let Some(due) = waiting_until(now, saved_due) {
        return Ok(due);
    }
    let batch = expiry_batch(store, now, limit)?;
    if let Some((range, _)) = batch {
        deletions::clear_before_catalogue(store, journal, range)?;
    }
    save_due(store, next_due(now, batch))
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
    let batch = expiry_batch(store, now, limit)?;
    if let Some((range, _)) = batch {
        deletions::clear(
            store,
            journal,
            catalogue,
            recorder,
            range.start(),
            range.end(),
        )?;
    }
    save_due(store, next_due(now, batch))
}

/// The saved due time, while it is still ahead and no more than a day away. A missing or
/// unreadable value, or one a clock moved back made distant, means check now.
fn waiting_until(now: i64, saved_due: Option<&str>) -> Option<i64> {
    let due = match saved_due.map(str::parse::<i64>) {
        Some(Ok(due)) => due,
        _ => return None,
    };
    (now < due && due - now <= DAY_SECS).then_some(due)
}

/// The journaled range the next expiry clears and whether older visits remain after it.
///
/// The cutoff never passes the newest stored visit's own 90 days, so a clock that jumps
/// ahead cannot expire the most recent 90 days of real browsing.
fn expiry_batch(
    store: &Store,
    now: i64,
    limit: usize,
) -> Result<Option<(DeletionRange, bool)>, SessionError> {
    let Some(newest) = store.newest_visit_time().map_err(SessionError::Store)? else {
        return Ok(None);
    };
    let cutoff = now.min(newest).saturating_sub(RETENTION_SECS);
    let Some(end) = store
        .history_expiry_end(cutoff, limit)
        .map_err(SessionError::Store)?
    else {
        return Ok(None);
    };
    let range = DeletionRange::new(0, end).map_err(deletions::journal_error)?;
    Ok(Some((range, end < cutoff)))
}

/// A batch that stopped short of the cutoff leaves a backlog for the next idle check;
/// otherwise the next expiry waits a day.
fn next_due(now: i64, batch: Option<(DeletionRange, bool)>) -> i64 {
    match batch {
        Some((_, true)) => now,
        _ => now.saturating_add(DAY_SECS),
    }
}

/// Saves the due time so a restart within the day journals nothing new.
fn save_due(store: &Store, due: i64) -> Result<i64, SessionError> {
    store
        .save_settings(vec![(EXPIRY_DUE_KEY.to_owned(), due.to_string())])
        .map_err(SessionError::Store)?;
    Ok(due)
}

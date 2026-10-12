//! Expires browsing history older than 90 days through D47's journaled clears (DW-4).

use std::collections::BTreeMap;

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
/// The furthest one expiry may move the cutoff past the last one, so a clock that jumps
/// ahead needs months of runs, not one, to reach real history.
pub(super) const MAX_CUTOFF_STEP: i64 = 2 * DAY_SECS;
/// The settings key holding when the next expiry is due, in Unix seconds.
const EXPIRY_DUE_KEY: &str = "history.expiry_due";
/// The settings key holding the last applied cutoff, in Unix seconds.
const EXPIRY_CUTOFF_KEY: &str = "history.expiry_cutoff";

/// When retention runs next and how far it last reached; saved in the settings table.
#[derive(Debug)]
pub(super) struct Schedule {
    /// Unix seconds when the next expiry is due; zero means now.
    pub(super) due: i64,
    /// The last applied cutoff, which the next may pass by at most `MAX_CUTOFF_STEP`.
    pub(super) cutoff: Option<i64>,
}

impl Schedule {
    /// Takes the saved schedule out of the settings, so the window never receives it.
    /// A missing or unreadable value means "not known": due now, with no earlier cutoff.
    pub(super) fn take_saved(settings: &mut BTreeMap<String, String>) -> Self {
        Self {
            due: saved_seconds(settings.remove(EXPIRY_DUE_KEY)).unwrap_or(0),
            cutoff: saved_seconds(settings.remove(EXPIRY_CUTOFF_KEY)),
        }
    }

    /// Whether an expiry ran within the last day. A due time more than a day ahead was
    /// left by a clock that moved back, so it does not count.
    pub(super) fn waiting(&self, now: i64) -> bool {
        now < self.due && self.due - now <= DAY_SECS
    }
}

/// Expires one bounded batch before the catalogue loads, unless one ran within the day.
pub(super) fn expire_at_startup(
    store: &Store,
    journal: &mut DeletionJournal,
    now: i64,
    schedule: &mut Schedule,
    limit: usize,
) -> Result<(), SessionError> {
    if schedule.waiting(now) {
        return Ok(());
    }
    let cutoff = expiry_cutoff(store, now, schedule.cutoff)?;
    let batch = expiry_batch(store, cutoff, limit)?;
    if let Some((range, _)) = batch {
        deletions::clear_before_catalogue(store, journal, range)?;
    }
    save(store, schedule, now, cutoff, batch)
}

/// Expires one bounded batch while the session runs, with the catalogue following.
pub(super) fn expire_live(
    store: &Store,
    journal: &mut DeletionJournal,
    catalogue: &mut Catalogue,
    recorder: &mut history::HistoryRecorder,
    now: i64,
    schedule: &mut Schedule,
    limit: usize,
) -> Result<(), SessionError> {
    let cutoff = expiry_cutoff(store, now, schedule.cutoff)?;
    let batch = expiry_batch(store, cutoff, limit)?;
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
    save(store, schedule, now, cutoff, batch)
}

fn saved_seconds(value: Option<String>) -> Option<i64> {
    match value.as_deref().map(str::parse::<i64>) {
        Some(Ok(seconds)) => Some(seconds),
        _ => None,
    }
}

/// Visits before this time expire; `None` when there is no history.
///
/// It is 90 days before the earlier of the clock and the newest stored visit, and moves at
/// most `MAX_CUTOFF_STEP` past the last applied cutoff. A wrong clock, or a visit stamped by
/// one, therefore advances it by days per run instead of years. A saved cutoff ahead of the
/// new one (a clock moved back) is ignored.
fn expiry_cutoff(store: &Store, now: i64, last: Option<i64>) -> Result<Option<i64>, SessionError> {
    let Some(newest) = store.newest_visit_time().map_err(SessionError::Store)? else {
        return Ok(None);
    };
    let candidate = now.min(newest).saturating_sub(RETENTION_SECS);
    Ok(Some(match last {
        Some(last) if last <= candidate => candidate.min(last.saturating_add(MAX_CUTOFF_STEP)),
        _ => candidate,
    }))
}

/// The journaled range the next expiry clears and whether older visits remain after it.
fn expiry_batch(
    store: &Store,
    cutoff: Option<i64>,
    limit: usize,
) -> Result<Option<(DeletionRange, bool)>, SessionError> {
    let Some(cutoff) = cutoff else {
        return Ok(None);
    };
    let Some(end) = store
        .history_expiry_end(cutoff, limit)
        .map_err(SessionError::Store)?
    else {
        return Ok(None);
    };
    let range = DeletionRange::new(0, end).map_err(deletions::journal_error)?;
    Ok(Some((range, end < cutoff)))
}

/// Records the run: a batch that stopped short of the cutoff leaves a backlog for the next
/// idle check, otherwise the next expiry waits a day. Both values are saved so a restart
/// within the day journals nothing and the cutoff keeps its step limit.
fn save(
    store: &Store,
    schedule: &mut Schedule,
    now: i64,
    cutoff: Option<i64>,
    batch: Option<(DeletionRange, bool)>,
) -> Result<(), SessionError> {
    schedule.due = match batch {
        Some((_, true)) => now,
        _ => now.saturating_add(DAY_SECS),
    };
    if cutoff.is_some() {
        schedule.cutoff = cutoff;
    }
    let mut entries = vec![(EXPIRY_DUE_KEY.to_owned(), schedule.due.to_string())];
    if let Some(cutoff) = schedule.cutoff {
        entries.push((EXPIRY_CUTOFF_KEY.to_owned(), cutoff.to_string()));
    }
    store.save_settings(entries).map_err(SessionError::Store)
}

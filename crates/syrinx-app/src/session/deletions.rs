//! Runs D47 clears of history and site zoom levels through the journal, the store and the
//! running catalogue (P1-27, #121).

use futures::channel::mpsc::UnboundedSender;
use syrinx_platform::{DeletionJournal, JournalError};
use syrinx_search::{Catalogue, Target};
use syrinx_store::{
    AppliedDeletions, HistoryDeletion, Store,
    deletion::{DeletionCategory, DeletionError, DeletionRange},
};

use super::{
    ClearedHistory, ClearedSiteZooms, SessionError, SessionEvent, emit, history,
    page_text::PageText, preferences,
};

/// Everything besides the database that a clear must reach before the journal completes it:
/// the running catalogue, the history recorder's cursors and the index's page text.
pub(super) struct Followers<'a> {
    catalogue: &'a mut Catalogue,
    recorder: &'a mut history::HistoryRecorder,
    page_text: &'a mut PageText,
}

impl<'a> Followers<'a> {
    pub(super) fn new(
        catalogue: &'a mut Catalogue,
        recorder: &'a mut history::HistoryRecorder,
        page_text: &'a mut PageText,
    ) -> Self {
        Self {
            catalogue,
            recorder,
            page_text,
        }
    }
}

/// What finishing journal requests removed, by category, in the counts the dialog shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Finished {
    pub(super) history: ClearedHistory,
    site_zooms: usize,
    /// A `SiteZoom` request was applied, so the window's copy of the levels is stale even when
    /// a retried clear found nothing left to remove (#121).
    zoom_applied: bool,
}

impl Finished {
    fn from_applied(applied: &AppliedDeletions, category: DeletionCategory) -> Self {
        Self {
            history: ClearedHistory {
                visits: applied.history.visits,
                addresses: applied.history.removed.len(),
            },
            site_zooms: applied.site_zooms,
            zoom_applied: category == DeletionCategory::SiteZoom,
        }
    }

    fn plus(self, other: Self) -> Self {
        Self {
            history: ClearedHistory {
                visits: self.history.visits + other.history.visits,
                addresses: self.history.addresses + other.history.addresses,
            },
            site_zooms: self.site_zooms + other.site_zooms,
            zoom_applied: self.zoom_applied || other.zoom_applied,
        }
    }
}

/// Brings the opened database level with the journal before anything reads history or zoom.
///
/// A restored or older database replays later clears; an interrupted clear is finished.
/// A database ahead of its journal means records were lost, which is refused. A repaired
/// journal arrives with a requested all-time clear, which this applies and completes. The
/// index keeps its own generation, so it catches up apart from the database.
pub(super) fn reconcile(
    store: &Store,
    journal: &mut DeletionJournal,
    page_text: &mut PageText,
) -> Result<(), SessionError> {
    let applied = store.deletion_generation().map_err(SessionError::Store)?;
    let missing = journal.log().unapplied(applied).map_err(journal_error)?;
    if !missing.is_empty() {
        store
            .apply_deletions(missing)
            .map_err(SessionError::Store)?;
    }
    page_text.follow(journal.log());
    complete_pending(journal)
}

/// Runs one history clear before the catalogue exists, so only the store, the index and the
/// journal change.
pub(super) fn clear_before_catalogue(
    store: &Store,
    journal: &mut DeletionJournal,
    page_text: &mut PageText,
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
        .apply_deletions(&[pending])
        .map_err(SessionError::Store)?;
    page_text.follow(journal.log());
    complete_pending(journal)
}

/// Clears history visited in `[start, end)` Unix seconds and reports how much went away, with
/// whatever an interrupted earlier clear of either category removed on the way.
///
/// The request is durable before anything is deleted and completion is written after the
/// database and its followers act, so a crash in between finishes at the next start.
pub(super) fn clear(
    store: &Store,
    journal: &mut DeletionJournal,
    followers: Followers<'_>,
    start: i64,
    end: i64,
) -> Result<Finished, SessionError> {
    journaled_clear(
        store,
        journal,
        followers,
        DeletionCategory::History,
        start,
        end,
    )
}

/// Clears the zoom levels changed in `[start, end)` Unix seconds through the same journal, so
/// a restored backup cannot bring them back (#121), and reads what is left for the window.
pub(super) fn clear_site_zooms(
    store: &Store,
    journal: &mut DeletionJournal,
    followers: Followers<'_>,
    start: i64,
    end: i64,
) -> Result<ClearedSiteZooms, SessionError> {
    let finished = journaled_clear(
        store,
        journal,
        followers,
        DeletionCategory::SiteZoom,
        start,
        end,
    )?;
    let remaining = preferences::restore_site_zooms(store)?;
    Ok(ClearedSiteZooms {
        sites: finished.site_zooms,
        remaining,
    })
}

/// Finishes any interrupted clear, then journals, applies and completes one of `category`.
fn journaled_clear(
    store: &Store,
    journal: &mut DeletionJournal,
    mut followers: Followers<'_>,
    category: DeletionCategory,
    start: i64,
    end: i64,
) -> Result<Finished, SessionError> {
    let range = DeletionRange::new(start, end).map_err(journal_error)?;
    // An earlier clear that stopped part-way is finished before a new one may start.
    let earlier = finish_pending(store, journal, &mut followers)?;
    let request = journal
        .log()
        .request(category, range)
        .map_err(journal_error)?;
    journal.append(request).map_err(SessionError::Deletion)?;
    let cleared = finish_pending(store, journal, &mut followers)?;
    Ok(earlier.plus(cleared))
}

pub(super) fn finish_pending(
    store: &Store,
    journal: &mut DeletionJournal,
    followers: &mut Followers<'_>,
) -> Result<Finished, SessionError> {
    let Some(pending) = journal.log().pending() else {
        return Ok(Finished::default());
    };
    let applied = store
        .apply_deletions(&[pending])
        .map_err(SessionError::Store)?;
    follow(followers.catalogue, followers.recorder, &applied.history)?;
    followers.page_text.follow(journal.log());
    complete_pending(journal)?;
    Ok(Finished::from_applied(&applied, pending.category()))
}

/// Hands the window the levels left when a clear it did not ask for removed zoom levels: one a
/// busy database interrupted, which the idle loop or a later history clear finished (#121). A
/// clear the window asked for reports its levels in `SiteZoomsCleared` instead.
pub(super) fn report_replaced_site_zooms(
    store: &Store,
    finished: &Finished,
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    if !finished.zoom_applied {
        return Ok(());
    }
    let levels = preferences::restore_site_zooms(store)?;
    emit(events, SessionEvent::SiteZoomsReplaced(levels))
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

pub(super) fn journal_error(error: DeletionError) -> SessionError {
    SessionError::Deletion(JournalError::Deletion(error))
}

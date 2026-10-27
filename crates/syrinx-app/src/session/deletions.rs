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
/// the running catalogue, the history recorder's cursors, the index's page text and the window.
pub(super) struct Followers<'a> {
    catalogue: &'a mut Catalogue,
    recorder: &'a mut history::HistoryRecorder,
    page_text: &'a mut PageText,
    events: &'a UnboundedSender<SessionEvent>,
}

impl<'a> Followers<'a> {
    pub(super) fn new(
        catalogue: &'a mut Catalogue,
        recorder: &'a mut history::HistoryRecorder,
        page_text: &'a mut PageText,
        events: &'a UnboundedSender<SessionEvent>,
    ) -> Self {
        Self {
            catalogue,
            recorder,
            page_text,
            events,
        }
    }
}

/// What finishing journal requests removed, by category, in the counts the dialog shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Finished {
    pub(super) history: ClearedHistory,
    site_zooms: usize,
}

impl Finished {
    fn from_applied(applied: &AppliedDeletions) -> Self {
        Self {
            history: ClearedHistory {
                visits: applied.history.visits,
                addresses: applied.history.removed.len(),
            },
            site_zooms: applied.site_zooms,
        }
    }

    fn plus(self, other: Self) -> Self {
        Self {
            history: ClearedHistory {
                visits: self.history.visits + other.history.visits,
                addresses: self.history.addresses + other.history.addresses,
            },
            site_zooms: self.site_zooms + other.site_zooms,
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
    let earlier = finish_pending(store, journal, &mut followers, Origin::Interrupted)?;
    let cleared = journal_and_finish(store, journal, &mut followers, category, range);
    // The earlier clear's failed report waits until the new clear has run, so a closed window
    // cannot cancel the owner's deletion.
    match cleared {
        Ok(cleared) => Ok(earlier.settle()?.plus(cleared)),
        Err(error) => {
            log_held(earlier.report);
            Err(error)
        }
    }
}

/// Journals a new request of `category`, then applies and completes it.
fn journal_and_finish(
    store: &Store,
    journal: &mut DeletionJournal,
    followers: &mut Followers<'_>,
    category: DeletionCategory,
    range: DeletionRange,
) -> Result<Finished, SessionError> {
    let request = journal
        .log()
        .request(category, range)
        .map_err(journal_error)?;
    journal.append(request).map_err(SessionError::Deletion)?;
    finish_pending(store, journal, followers, Origin::Requested)?.settle()
}

/// Finishes a clear that an earlier run left pending, which the window did not just ask for.
/// If it cleared zoom levels the window hears the levels left as soon as they are gone, even
/// when a later step fails (#121, #159).
pub(super) fn finish_interrupted(
    store: &Store,
    journal: &mut DeletionJournal,
    followers: &mut Followers<'_>,
) -> Result<Finished, SessionError> {
    finish_pending(store, journal, followers, Origin::Interrupted)?.settle()
}

/// Whether the window asked for the pending clear it is about to be told about.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Origin {
    /// An earlier run left it pending, so the window has not heard its zoom levels.
    Interrupted,
    /// The window asked for it and hears its levels in `SiteZoomsCleared`.
    Requested,
}

/// What finishing a pending request removed, and whether telling the window about it failed.
struct Outcome {
    finished: Finished,
    /// A failed send of the zoom levels, held back so it cannot stop the clears after it.
    report: Result<(), SessionError>,
}

impl Outcome {
    /// The counts, or the failed report if there is one.
    fn settle(self) -> Result<Finished, SessionError> {
        self.report?;
        Ok(self.finished)
    }
}

/// Applies the pending request and completes it.
fn finish_pending(
    store: &Store,
    journal: &mut DeletionJournal,
    followers: &mut Followers<'_>,
    origin: Origin,
) -> Result<Outcome, SessionError> {
    let Some(pending) = journal.log().pending() else {
        return Ok(Outcome {
            finished: Finished::default(),
            report: Ok(()),
        });
    };
    let applied = store
        .apply_deletions(&[pending])
        .map_err(SessionError::Store)?;
    // Sent before the steps that can still fail, so a failure cannot leave the window stale.
    // Its own failure waits until the clear is complete, so it cannot strand the request.
    let report =
        if origin == Origin::Interrupted && pending.category() == DeletionCategory::SiteZoom {
            report_replaced_site_zooms(store, followers.events)
        } else {
            Ok(())
        };
    if let Err(error) = follow_and_complete(journal, followers, &applied) {
        log_held(report);
        return Err(error);
    }
    Ok(Outcome {
        finished: Finished::from_applied(&applied),
        report,
    })
}

/// Brings the followers level with an applied request, then completes it in the journal.
fn follow_and_complete(
    journal: &mut DeletionJournal,
    followers: &mut Followers<'_>,
    applied: &AppliedDeletions,
) -> Result<(), SessionError> {
    follow(followers.catalogue, followers.recorder, &applied.history)?;
    followers.page_text.follow(journal.log());
    complete_pending(journal)
}

/// Hands the window the zoom levels the database now holds.
fn report_replaced_site_zooms(
    store: &Store,
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    let levels = preferences::restore_site_zooms(store)?;
    emit(events, SessionEvent::SiteZoomsReplaced(levels))
}

/// Writes a held report failure to the diagnostic stream when a later error is returned
/// instead, since only one error can be returned and neither may vanish.
fn log_held(report: Result<(), SessionError>) {
    if let Err(error) = report {
        eprintln!("{}", held_line(&error));
    }
}

/// The diagnostic line for a report that failed and was overtaken by a later failure.
fn held_line(error: &SessionError) -> String {
    format!("The window was not sent its zoom levels, and a later step failed too: {error}")
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

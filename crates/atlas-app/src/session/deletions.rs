//! Runs D47 history clears through the journal, the store and the running catalogue (P1-27).

use atlas_platform::{DeletionJournal, JournalError};
use atlas_search::{Catalogue, Target};
use atlas_store::{
    HistoryDeletion, Store,
    deletion::{DeletionCategory, DeletionRange},
};

use super::{SessionError, history};

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

/// Clears `[start, end)` Unix seconds and returns how many remembered addresses went away.
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
) -> Result<usize, SessionError> {
    let range = DeletionRange::new(start, end).map_err(journal_error)?;
    // An earlier clear that stopped part-way is finished before a new one may start.
    finish_pending(store, journal, catalogue, recorder)?;
    let request = journal
        .log()
        .request(DeletionCategory::History, range)
        .map_err(journal_error)?;
    journal.append(request).map_err(SessionError::Deletion)?;
    finish_pending(store, journal, catalogue, recorder)
}

fn finish_pending(
    store: &Store,
    journal: &mut DeletionJournal,
    catalogue: &mut Catalogue,
    recorder: &mut history::HistoryRecorder,
) -> Result<usize, SessionError> {
    let Some(pending) = journal.log().pending() else {
        return Ok(0);
    };
    let deletion = store
        .apply_history_deletions(&[pending])
        .map_err(SessionError::Store)?;
    follow(catalogue, recorder, &deletion)?;
    complete_pending(journal)?;
    Ok(deletion.removed.len())
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

fn journal_error(error: atlas_store::deletion::DeletionError) -> SessionError {
    SessionError::Deletion(JournalError::Deletion(error))
}

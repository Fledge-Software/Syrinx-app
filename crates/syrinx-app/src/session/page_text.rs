//! Keeps page text in the full-text index level with D47's clears and D40's seven days (#53).

use syrinx_platform::{CacheFolder, WindowsDirectoryLease};
use syrinx_search::fulltext::{ClearRequest, ClearScope, FullText, FullTextError};
use syrinx_store::deletion::{DeletionCategory, DeletionLog, DeletionRequest};

use super::{SessionError, deletions, retention};

/// The index's folder beside the database. Backups copy only the database, so the index stays
/// outside D13's snapshots as D40 requires.
pub(super) const INDEX_FOLDER: &str = "fulltext";

/// The session's full-text index and when its page text next expires.
///
/// The index records which journal generation it has applied, so it can always catch up from
/// the journal. That is why a failing index is closed for the rest of the session rather than
/// holding up a clear: nothing can read text it failed to remove, and the next start replays
/// what it missed.
pub(super) struct PageText {
    /// `None` once opening or maintenance failed.
    index: Option<FullText>,
    /// Held while the index is open, so its folder cannot be moved or replaced meanwhile.
    folder: Option<CacheFolder>,
    /// Unix seconds when D40's expiry next runs; zero, so start-up always runs it.
    expiry_due: i64,
    /// A failure the owner has not been shown yet.
    problem: Option<SessionError>,
}

impl PageText {
    /// Opens the index in its guarded folder; a failure leaves the session running without it.
    pub(super) fn open(lease: &WindowsDirectoryLease) -> Self {
        let (index, folder, problem) = match open_index(lease) {
            Ok((folder, index)) => (Some(index), Some(folder), None),
            Err(error) => (None, None, Some(error)),
        };
        Self {
            index,
            folder,
            expiry_due: 0,
            problem,
        }
    }

    /// Applies every journal clear the index has not, before the journal marks one complete.
    ///
    /// An index ahead of the journal cannot tell what the recreated journal cleared, so its
    /// page text goes and it counts again from the journal's start.
    pub(super) fn follow(&mut self, log: &DeletionLog) {
        let Some(index) = self.index.as_mut() else {
            return;
        };
        let followed = follow_journal(index, log);
        self.settle(followed);
    }

    /// Removes page text past D40's seven days at start-up and then once a day.
    pub(super) fn expire_if_due(&mut self, now: i64) {
        if retention::waiting(self.expiry_due, now) {
            return;
        }
        let Some(index) = self.index.as_mut() else {
            return;
        };
        let expired = index.expire(now).map(drop).or_else(cleanup_later);
        self.expiry_due = now.saturating_add(retention::DAY_SECS);
        self.settle(expired);
    }

    /// Retries removing deleted text from disk after Windows refused it.
    pub(super) fn retry_cleanup(&mut self) {
        let Some(index) = self.index.as_mut() else {
            return;
        };
        if !index.cleanup_pending() {
            return;
        }
        let retried = index.retry_cleanup().or_else(cleanup_later);
        self.settle(retried);
    }

    /// The failure to show the owner once, if any.
    pub(super) fn take_problem(&mut self) -> Result<(), SessionError> {
        self.problem.take().map_or(Ok(()), Err)
    }

    /// The open index, for tests that read what it holds.
    #[cfg(test)]
    pub(super) fn index(&self) -> Option<&FullText> {
        self.index.as_ref()
    }

    /// Closes the index after a failure and keeps the failure to report.
    fn settle(&mut self, result: Result<(), SessionError>) {
        let Err(error) = result else {
            return;
        };
        self.index = None;
        self.folder = None;
        self.problem = Some(error);
    }
}

/// Guards the index's folder beside the database, then opens the index inside it.
fn open_index(lease: &WindowsDirectoryLease) -> Result<(CacheFolder, FullText), SessionError> {
    let folder = lease
        .cache_folder(INDEX_FOLDER)
        .map_err(SessionError::IndexFolder)?;
    let index = FullText::open(folder.path()).map_err(SessionError::FullText)?;
    Ok((folder, index))
}

/// Brings the index level with the journal, restarting its count if it is ahead.
fn follow_journal(index: &mut FullText, log: &DeletionLog) -> Result<(), SessionError> {
    if index.cleared_generation() > log.last_generation() {
        index.restart_clears().or_else(cleanup_later)?;
    }
    let unapplied = log
        .unapplied(index.cleared_generation())
        .map_err(deletions::journal_error)?;
    let clears: Vec<_> = unapplied.iter().copied().map(clear_request).collect();
    index.apply_clears(&clears).map(drop).or_else(cleanup_later)
}

/// What one journal record means for the index: a history clear removes page text in its
/// range; any other category only advances the generation.
fn clear_request(request: DeletionRequest) -> ClearRequest {
    let scope = match request.category() {
        DeletionCategory::History => ClearScope::Pages {
            start: request.range().start(),
            end: request.range().end(),
        },
        DeletionCategory::SiteZoom => ClearScope::Elsewhere,
    };
    ClearRequest {
        generation: request.generation(),
        scope,
    }
}

/// A durable change whose deleted text Windows would not yet let go of is not a failure: the
/// next commit, or the idle retry, removes it.
fn cleanup_later(error: FullTextError) -> Result<(), SessionError> {
    match error {
        FullTextError::CleanupPending => Ok(()),
        error => Err(SessionError::FullText(error)),
    }
}

//! Keeps page text in the full-text index level with D47's clears and D40's seven days (#53).

use std::{fmt, fs};

use syrinx_platform::{CacheFolder, WindowsDirectoryLease};
use syrinx_search::fulltext::{
    Change, ClearRequest, ClearScope, FullText, FullTextDocument, FullTextError, Namespace,
};
use syrinx_store::deletion::{DeletionCategory, DeletionLog, DeletionRequest};
use syrinx_store::{DirectoryLease, LocationError};

use super::{SessionError, deletions, retention, stop};

/// Whether page text is captured: `on`, or anything else for off (#54).
pub const CAPTURE_KEY: &str = "page_text.capture";
/// The one stored value that switches capture on.
pub const CAPTURE_ON: &str = "on";

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
    /// The failures the owner has not been shown yet, each named with what the index was
    /// doing; a later one is kept after an earlier one, never in its place.
    problem: Option<SessionError>,
}

impl PageText {
    /// Opens the index in its guarded folder; a failure leaves the session running without it.
    pub(super) fn open(lease: &WindowsDirectoryLease) -> Self {
        let (index, folder, problem) = match open_index(lease, Emptying::WhenUnreadable) {
            Ok((folder, index)) => (Some(index), Some(folder), None),
            Err(failure) => (
                None,
                None,
                Some(stop::during(OPENING)(failure.into_error())),
            ),
        };
        Self {
            index,
            folder,
            expiry_due: 0,
            problem,
        }
    }

    /// Removes all page text and restarts the index's count before a confirmed repair rebuilds
    /// the journal, whose new generations cannot say what the index cleared.
    ///
    /// The restart keeps plugin feeds. An index that is closed or fails to restart is emptied
    /// instead, feeds and all, because nothing in it can be read to remove page text alone;
    /// failing to empty it refuses the repair, so the next start offers it again.
    pub(super) fn restart_before_repair(
        &mut self,
        lease: &WindowsDirectoryLease,
    ) -> Result<(), SessionError> {
        if self.restart_open_index() {
            return Ok(());
        }
        self.start_afresh(lease)
    }

    /// Applies every journal clear the index has not, before the journal marks one complete.
    ///
    /// An index ahead of the journal cannot tell what the journal's generations cleared, so
    /// its page text goes and it counts again from the journal's start.
    pub(super) fn follow(&mut self, log: &DeletionLog) {
        let Some(index) = self.index.as_mut() else {
            return;
        };
        let followed = follow_journal(index, log);
        self.settle(followed, "applying a clear to the search index");
    }

    /// Keeps one page's text for D40's seven days, replacing an earlier capture of its address.
    pub(super) fn store(&mut self, page: CapturedPage, now: i64) {
        let Some(index) = self.index.as_mut() else {
            return;
        };
        let stored = store_page(index, page, now);
        self.settle(stored, "keeping a page's text");
    }

    /// Erases all page text at once, as switching capture off does (owner, 2026-10-27).
    /// An index closed by an earlier failure cannot be erased now, so that is reported too;
    /// the next start, or the next save of the setting, tries again.
    pub(super) fn erase(&mut self) {
        let Some(index) = self.index.as_mut() else {
            let closed = SessionError::FullText(FullTextError::NeedsReopen);
            self.report(stop::during(ERASING)(closed));
            return;
        };
        let erased = index.reset(&Namespace::page()).or_else(cleanup_later);
        self.settle(erased, ERASING);
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
        self.settle(expired, "expiring old page text");
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
        self.settle(retried, "removing deleted text from the search index");
    }

    /// The failures to show the owner once, if any.
    pub(super) fn take_problem(&mut self) -> Result<(), SessionError> {
        self.problem.take().map_or(Ok(()), Err)
    }

    /// The open index, for tests that read what it holds.
    #[cfg(test)]
    pub(super) fn index(&self) -> Option<&FullText> {
        self.index.as_ref()
    }

    /// Closes the index as a failed operation would, for tests of what follows.
    #[cfg(test)]
    pub(super) fn close_for_test(&mut self) {
        self.index = None;
        self.folder = None;
    }

    /// The open index, for tests that stage work in it.
    #[cfg(test)]
    pub(super) fn index_mut(&mut self) -> Option<&mut FullText> {
        self.index.as_mut()
    }

    /// Restarts the open index's count; false when it is closed or the restart failed.
    fn restart_open_index(&mut self) -> bool {
        let Some(index) = self.index.as_mut() else {
            return false;
        };
        let restarted = index.restart_clears().or_else(cleanup_later);
        let succeeded = restarted.is_ok();
        self.settle(
            restarted,
            "restarting the search index for a history repair",
        );
        succeeded
    }

    /// Empties the index's folder and opens a fresh index in it. The repair is refused only
    /// when old page text could be left behind.
    fn start_afresh(&mut self, lease: &WindowsDirectoryLease) -> Result<(), SessionError> {
        // Release the old index's files and folder before deleting them.
        self.index = None;
        self.folder = None;
        let failure = match open_index(lease, Emptying::First) {
            Ok((folder, index)) => {
                self.index = Some(index);
                self.folder = Some(folder);
                return Ok(());
            }
            Err(failure) => failure,
        };
        match failure {
            // The folder is empty, so no page text is left: a fresh index that will not open
            // is only reported.
            OpenFailure::Opening(error) => {
                self.report(stop::during(OPENING)(SessionError::FullText(error)));
                Ok(())
            }
            // Syrinx never opens an index through a file or link, so none can strand page text.
            OpenFailure::Folder(error) if wrong_entry_where_the_folder_belongs(lease, &error) => {
                self.report(stop::during(REPAIRING)(SessionError::IndexFolder(error)));
                Ok(())
            }
            OpenFailure::Folder(error) => {
                Err(self.after_problem(stop::during(REPAIRING)(SessionError::IndexFolder(error))))
            }
            OpenFailure::Emptying(error) => {
                Err(self.after_problem(SessionError::IndexNotEmptied(error)))
            }
        }
    }

    /// Closes the index after a failure and keeps the failure, named with `operation`.
    fn settle(&mut self, result: Result<(), SessionError>, operation: &'static str) {
        let Err(error) = result else {
            return;
        };
        self.index = None;
        self.folder = None;
        self.report(stop::during(operation)(error));
    }

    /// Keeps `error` to show the owner, after any failure not yet shown.
    fn report(&mut self, error: SessionError) {
        let error = self.after_problem(error);
        self.problem = Some(error);
    }

    /// `error`, carrying any failure not yet shown as its earlier cause.
    fn after_problem(&mut self, error: SessionError) -> SessionError {
        match self.problem.take() {
            None => error,
            Some(earlier) => SessionError::AfterEarlier {
                earlier: Box::new(earlier),
                error: Box::new(error),
            },
        }
    }
}

/// One page's captured text, as the window hands it over (#54). Its address, without a
/// fragment, keys the document, so a later capture of the same page replaces it.
pub struct CapturedPage {
    pub address: String,
    pub title: String,
    pub text: String,
}

impl fmt::Debug for CapturedPage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Addresses, titles and page text are the owner's browsing; they never reach a log.
        formatter.write_str("CapturedPage { contents: [redacted] }")
    }
}

/// Stages and commits one page. Captures are seconds apart, so `now` orders them as
/// revisions; a second capture within the same second is the same page and changes nothing.
fn store_page(index: &mut FullText, page: CapturedPage, now: i64) -> Result<(), SessionError> {
    let revision = u64::try_from(now).map_err(|_| SessionError::ClockUnavailable)?;
    let document = FullTextDocument {
        namespace: Namespace::page(),
        id: page.address,
        revision,
        captured_at: now,
        title: page.title,
        body: page.text,
    };
    match index.stage(&[Change::Upsert(document)], now) {
        Ok(_) => {}
        // An address too long to key the index is left unread; the index itself is sound.
        Err(FullTextError::InvalidDocument) => return Ok(()),
        Err(error) => return Err(SessionError::FullText(error)),
    }
    index.commit().or_else(cleanup_later)
}

/// Whether a saved capture value means off after having been set: then any page text left
/// by an erase that failed or was interrupted goes at start-up or with the save.
pub(super) fn capture_switched_off(value: Option<&str>) -> bool {
    value.is_some_and(|value| value != CAPTURE_ON)
}

/// What a failed erase is reported as.
const ERASING: &str = "erasing page text after capture was switched off";

/// What a failure to open the index is reported as.
const OPENING: &str = "opening the search index";

/// What a guard failure that refuses a repair is reported as.
const REPAIRING: &str = "emptying the search index before a history repair";

/// When the index's folder is emptied before the index opens.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Emptying {
    /// Only when the index is damaged or from another version.
    WhenUnreadable,
    /// Always, so the fresh index holds nothing from before.
    First,
}

/// Which step of opening the index failed, since a repair treats each differently.
enum OpenFailure {
    /// The storage guard refused the index's folder.
    Folder(LocationError),
    /// A file inside the folder could not be deleted.
    Emptying(LocationError),
    /// Tantivy could not open an index in the folder.
    Opening(FullTextError),
}

impl OpenFailure {
    fn into_error(self) -> SessionError {
        match self {
            Self::Folder(error) | Self::Emptying(error) => SessionError::IndexFolder(error),
            Self::Opening(error) => SessionError::FullText(error),
        }
    }
}

/// Guards the index's folder beside the database, empties it when `emptying` says so, then
/// opens the index inside it.
///
/// The index is a cache, so one that is damaged or from another version starts afresh:
/// left alone, its unreadable page text would never expire (D40).
fn open_index(
    lease: &WindowsDirectoryLease,
    emptying: Emptying,
) -> Result<(CacheFolder, FullText), OpenFailure> {
    let folder = lease
        .cache_folder(INDEX_FOLDER)
        .map_err(OpenFailure::Folder)?;
    if emptying == Emptying::First {
        folder.empty().map_err(OpenFailure::Emptying)?;
    }
    let opened = match FullText::open(folder.path()) {
        Err(FullTextError::Damaged | FullTextError::IncompatibleIndex) => {
            folder.empty().map_err(OpenFailure::Emptying)?;
            FullText::open(folder.path())
        }
        opened => opened,
    };
    let index = opened.map_err(OpenFailure::Opening)?;
    Ok((folder, index))
}

/// Whether the guard refused the index's folder because a file or a link stands where it
/// belongs, rather than for a fault in the data folder, the database or a sync root.
fn wrong_entry_where_the_folder_belongs(
    lease: &WindowsDirectoryLease,
    error: &LocationError,
) -> bool {
    if !matches!(
        error,
        LocationError::UnexpectedEntryKind | LocationError::ReparsePoint
    ) {
        return false;
    }
    let Some(data_folder) = lease.database_path().parent() else {
        return false;
    };
    // The entry's own metadata, never a link's target: a symbolic link or junction is not a
    // folder here, while a real folder with another reparse tag (a cloud placeholder, say)
    // still is, so its refusal is not waved through. An entry that cannot be read is not
    // known to be wrong, so the guard's error refuses the repair instead.
    let Ok(entry) = fs::symlink_metadata(data_folder.join(INDEX_FOLDER)) else {
        return false;
    };
    !entry.is_dir()
}

/// Brings the index level with the journal, first restarting its count when the journal is
/// behind it.
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

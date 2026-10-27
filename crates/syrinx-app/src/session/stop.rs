//! Names what the session worker was doing when it stopped, decides which failures it survives,
//! and keeps each kind in its own size-capped log beside the database so neither is lost (#9).

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use futures::channel::mpsc::UnboundedSender;
use syrinx_store::{EncodingField, StoreError, StoreOperation};

use super::{SessionCommand, SessionError, SessionEvent, emit_recoverable};

/// Beside `syrinx.db`. D13's synced-folder guard covers it only while its lease is held, so
/// write it before the store closes; backups copy only the database.
pub(super) const LOG_NAME: &str = "worker-stops.log";
/// The previous log, replaced each time the current one reaches `LOG_LIMIT`.
pub(super) const OLD_LOG_NAME: &str = "worker-stops.log.old";
/// Survived failures, kept apart so a flood of them can never push a stop out of its log.
pub(super) const SURVIVED_LOG_NAME: &str = "worker-survived.log";
/// The previous survived-failure log, rotated like the stop log.
pub(super) const OLD_SURVIVED_LOG_NAME: &str = "worker-survived.log.old";
/// About 300 lines; with the previous file kept, at most twice this on disk, per log.
pub(super) const LOG_LIMIT: u64 = 32 * 1024;

/// Names shared with the loop, which answers these two commands before `handle`.
pub(super) const CHANGING_USAGE: &str = "changing usage counting";
pub(super) const BUILDING_USAGE_REPORT: &str = "building a usage report";

impl SessionCommand {
    /// A fixed name for this command, safe to show and log: it never includes the contents.
    pub(crate) fn operation(&self) -> &'static str {
        match self {
            Self::CheckPermission { .. } => "checking a site permission",
            Self::RememberPermission { .. } => "remembering a site permission",
            Self::RememberedPermissions { .. } => "listing site permissions",
            Self::ForgetPermission { .. } => "forgetting a site permission",
            Self::ConfirmRecovery { .. } => "confirming a backup recovery",
            Self::ConfirmHistoryRepair { .. } => "confirming a history repair",
            Self::HomeEntry { .. } => "opening from Home",
            Self::NewTab { .. } => "opening a new tab",
            Self::AdoptPopup { .. } => "adopting a popup",
            Self::NavigateActive { .. } => "navigating the active tab",
            Self::CloseTab(_) => "closing a tab",
            Self::ObserveClosed(_) => "recording a page that closed itself",
            Self::CloseActive => "closing the active tab",
            Self::CycleActive { .. } => "moving to the next tab",
            Self::FocusTab(_) => "switching tabs",
            #[cfg(test)]
            Self::Duplicate(_) => "duplicating a tab",
            #[cfg(test)]
            Self::CloseOthers(_) => "closing other tabs",
            #[cfg(test)]
            Self::CloseBelow(_) => "closing tabs below",
            Self::SaveTab(_) => "saving a page",
            Self::OpenSaved(_) => "opening a saved page",
            Self::SaveActive => "saving the active page",
            Self::Unsave(_) => "removing a saved page",
            Self::Folder(_) => "changing saved folders",
            Self::UpdateMetadata { .. } => "updating a tab's details",
            Self::ObserveMetadata { .. } => "recording a page's title or address",
            Self::ReopenClosed => "reopening a closed tab",
            Self::ImportBookmarks { .. } => "importing bookmarks",
            Self::SaveSettings(_) => "saving settings",
            Self::SiteZoom { .. } => "saving a site zoom level",
            Self::ClearSiteZooms { .. } => "clearing site zoom levels",
            Self::EditDestinations { .. } => "editing tiles",
            Self::Query { .. } => "searching",
            Self::ObserveNavigation { .. } => "recording a finished navigation",
            Self::OpenHistory(_) => "opening a history entry",
            Self::RecentHistory { .. } => "listing history",
            Self::ClearHistory { .. } => "clearing history",
            Self::GrowSyntheticCatalogue { .. } => "growing the synthetic catalogue",
            Self::SnapshotBeforeUpdate => "taking the pre-update snapshot",
            Self::Usage(_) => CHANGING_USAGE,
            Self::UsageReport { .. } => BUILDING_USAGE_REPORT,
            Self::Shutdown => "shutting down",
        }
    }
}

/// Names in one place what the loop and `handle` call before a command runs.
pub(super) const SAVING_SETTLED_TITLES: &str = "saving settled page titles";

/// Tags an error that ends the worker's loop with the operation that raised it.
/// An error already tagged keeps its inner, more specific name (#77).
pub(super) fn during(operation: &'static str) -> impl FnOnce(SessionError) -> SessionError {
    move |error| match error {
        SessionError::During { .. } => error,
        error => SessionError::During {
            operation,
            error: Box::new(error),
        },
    }
}

/// Whether the worker can go on after `error`: a busy database, one refused setting, an index
/// refusal, a refused command or a window that stopped listening. Any sign that storage is
/// lost or damaged stops it (#9).
pub(super) fn survivable(error: &SessionError) -> bool {
    match error {
        SessionError::During { error, .. } => survivable(error),
        SessionError::Store(error) => store_survivable(error),
        SessionError::Search(_)
        | SessionError::FullText(_)
        | SessionError::IndexFolder(_)
        | SessionError::EventReceiverClosed
        | SessionError::Model(_)
        | SessionError::Destination(_)
        | SessionError::Deletion(_)
        | SessionError::Usage(_)
        | SessionError::NoActiveTab
        | SessionError::HistoryUnavailable
        | SessionError::ClockUnavailable => true,
        _ => false,
    }
}

/// Whether the window can no longer hear the worker, so a report has nowhere to go.
pub(super) fn window_lost(error: &SessionError) -> bool {
    match error {
        SessionError::During { error, .. } => window_lost(error),
        error => *error == SessionError::EventReceiverClosed,
    }
}

/// A store failure the next write may pass. Anything else means storage is gone, damaged,
/// changed behind the worker or left inside an unfinished transaction. Only a setting is a
/// refusal of the input alone: other encoding errors also come from reading damaged rows.
fn store_survivable(error: &StoreError) -> bool {
    match error {
        StoreError::Database {
            operation: StoreOperation::Rollback,
            ..
        } => false,
        StoreError::Database { code, .. } => code.is_some_and(sqlite_code_survivable),
        StoreError::CleanupFailed { primary, cleanup } => {
            store_survivable(primary) && store_survivable(cleanup)
        }
        StoreError::InvalidEncoding(EncodingField::Setting)
        | StoreError::TooManyRecords
        | StoreError::BackupUnavailable
        | StoreError::BackupVerificationFailed
        | StoreError::InvalidBackupPath => true,
        _ => false,
    }
}

/// SQLITE_IOERR_DATA and SQLITE_IOERR_CORRUPTFS: the file's contents or file system are damaged.
const DAMAGED_IO_CODES: [i32; 2] = [8202, 8458];

/// SQLite's primary result codes for a busy, locked or full database, or one refused row:
/// BUSY, LOCKED, NOMEM, INTERRUPT, IOERR, FULL, PROTOCOL, SCHEMA, TOOBIG, CONSTRAINT, MISMATCH.
fn sqlite_code_survivable(code: i32) -> bool {
    if DAMAGED_IO_CODES.contains(&code) {
        return false;
    }
    matches!(
        code & 0xff,
        5 | 6 | 7 | 9 | 10 | 13 | 15 | 17 | 18 | 19 | 20
    )
}

/// Whether a refused tab save may pass on a later try: the database was busy, locked, out
/// of memory or space, or interrupted. A refused row (a constraint, a type mismatch, a value
/// too big, too many records) fails the same way every time.
pub(super) fn transient_save_failure(error: &StoreError) -> bool {
    let StoreError::Database {
        code: Some(code), ..
    } = error
    else {
        return false;
    };
    !DAMAGED_IO_CODES.contains(code) && matches!(code & 0xff, 5 | 6 | 7 | 9 | 10 | 13 | 15 | 17)
}

/// Keeps a step's survivable failure in `refused`, to report once the command's state is
/// published; a failure that must stop the loop is returned at once.
pub(super) fn keep_refused(
    refused: &mut Vec<SessionError>,
    result: Result<(), SessionError>,
) -> Result<(), SessionError> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if survivable(&error) => {
            refused.push(error);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// What the loop is doing and where its survived failures go; decides whether it goes on.
pub(super) struct Survival {
    /// What the loop is doing, named in each failure it survives or stops on.
    pub(super) operation: &'static str,
    log: SurvivedLog,
    /// Cleared once the window stops taking events, so that is logged only once.
    window_listening: bool,
}

impl Survival {
    pub(super) fn new() -> Self {
        Self {
            operation: "starting",
            log: SurvivedLog::default(),
            window_listening: true,
        }
    }

    /// Keeps survived failures in the folder beside the database from now on.
    pub(super) fn keep_in(&mut self, directory: PathBuf) {
        self.log.directory = Some(directory);
    }

    /// Writes the count of a repeated failure still waiting, before the loop ends.
    pub(super) fn finish(&mut self) {
        self.log.flush();
    }

    /// Passes a step's success through, or lets the loop survive its failure if it can.
    pub(super) fn result(
        &mut self,
        result: Result<(), SessionError>,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.survive(error, events),
        }
    }

    /// Lets the loop go on after a failure that leaves storage usable: it is logged beside
    /// the database and shown in the window. Any other failure is returned, named with the
    /// operation, to stop the loop.
    pub(super) fn survive(
        &mut self,
        error: SessionError,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let error = self.tagged(error);
        self.survive_shown(error.clone())?;
        // A window that stopped listening has nowhere to show it; saving still goes on.
        if window_lost(&error) {
            return Ok(());
        }
        match emit_recoverable(events, error) {
            Ok(()) => Ok(()),
            Err(lost) => self.survive_shown(lost),
        }
    }

    /// As `survive`, for a failure the window has already been told about: only logged.
    pub(super) fn survive_shown(&mut self, error: SessionError) -> Result<(), SessionError> {
        let error = self.tagged(error);
        if !survivable(&error) {
            return Err(error);
        }
        self.keep(&error);
        Ok(())
    }

    /// Reports each failure a command survived, after its state has been published.
    pub(super) fn report_refused(
        &mut self,
        refused: Vec<SessionError>,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        for error in refused {
            self.survive(error, events)?;
        }
        Ok(())
    }

    /// Logs a survived failure; a window that stopped listening is logged the first time only.
    fn keep(&mut self, error: &SessionError) {
        if window_lost(error) {
            if !self.window_listening {
                return;
            }
            self.window_listening = false;
        }
        self.log.record(survived_line(error));
    }

    /// Names the operation the loop is running, unless the failure already names one.
    pub(super) fn tagged(&self, error: SessionError) -> SessionError {
        match error {
            SessionError::During { .. } => error,
            error => during(self.operation)(error),
        }
    }
}

/// Writes survived failures to their own log, collapsing a run of identical lines into one
/// line and a count, so a busy database cannot fill the disk or hide anything else.
#[derive(Default)]
struct SurvivedLog {
    directory: Option<PathBuf>,
    last: Option<String>,
    repeats: u64,
}

impl SurvivedLog {
    fn record(&mut self, line: String) {
        if self.last.as_deref() == Some(line.as_str()) {
            self.repeats += 1;
            return;
        }
        self.flush();
        self.write(&line);
        self.last = Some(line);
    }

    /// Writes how many more times the last line repeated, once something else happens.
    fn flush(&mut self) {
        if self.repeats == 0 {
            return;
        }
        let repeats = std::mem::take(&mut self.repeats);
        self.write(&format!("The line above repeated {repeats} more times"));
    }

    /// One line to stderr and, once the folder is known, to the survived-failure log.
    fn write(&self, line: &str) {
        eprintln!("{line}");
        let Some(directory) = &self.directory else {
            return;
        };
        if let Err(problem) = append_survived(directory, line) {
            report_log_problem("write", &problem);
        }
    }
}

/// The diagnostic line for a stopped worker: the error text and operation name, which may carry
/// numbers (revisions, SQLite codes) but never an address or page content.
pub(super) fn stop_line(error: &SessionError) -> String {
    format!("The session worker stopped: {error}")
}

/// The diagnostic line for a failure the worker skipped and kept running after.
pub(super) fn survived_line(error: &SessionError) -> String {
    format!("The session worker skipped an operation and kept running: {error}")
}

/// Writes the stop reason to stderr, the app's diagnostic stream; a launch from Windows
/// has no console, so `keep_stop` also writes it to the stop log.
pub(super) fn log_stop(error: &SessionError) {
    eprintln!("{}", stop_line(error));
}

/// Adds the stop line to the log in `directory`; a failed write is reported, never fatal.
pub(super) fn keep_stop(directory: &Path, error: &SessionError) {
    if let Err(problem) = append_stop(directory, error) {
        report_log_problem("write", &problem);
    }
}

/// Appends one stop line, first moving a full log aside so the pair stays small.
/// A failed move is reported and the line still appended: one overshoot beats losing it.
pub(super) fn append_stop(directory: &Path, error: &SessionError) -> io::Result<()> {
    append_line(directory, LOG_NAME, OLD_LOG_NAME, &stop_line(error))
}

/// Appends one line to the survived-failure log, rotated like the stop log.
pub(super) fn append_survived(directory: &Path, line: &str) -> io::Result<()> {
    append_line(directory, SURVIVED_LOG_NAME, OLD_SURVIVED_LOG_NAME, line)
}

/// Appends one line to the log `name` in `directory`, moving a full log to `old_name` first.
fn append_line(directory: &Path, name: &str, old_name: &str, line: &str) -> io::Result<()> {
    let path = directory.join(name);
    if let Err(problem) = rotate_if_full(&path, &directory.join(old_name)) {
        report_log_problem("move aside", &problem);
    }
    let mut log = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(log, "{line}")
}

/// One stderr line naming the failed step, the error kind and the Windows code; no path.
fn report_log_problem(step: &str, problem: &io::Error) {
    eprintln!(
        "Cannot {step} the session worker's log ({:?}, code {:?})",
        problem.kind(),
        problem.raw_os_error()
    );
}

/// Replaces the previous log with the current one once it reaches `LOG_LIMIT`.
fn rotate_if_full(path: &Path, old: &Path) -> io::Result<()> {
    let length = match fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if length < LOG_LIMIT {
        return Ok(());
    }
    fs::rename(path, old)
}

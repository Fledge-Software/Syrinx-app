//! Names what the session worker was doing when it stopped, decides which failures it survives,
//! and keeps both in a size-capped log beside the database so neither is lost (#9).

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

use syrinx_store::{StoreError, StoreOperation};

use super::{SessionCommand, SessionError};

/// Beside `syrinx.db`. D13's synced-folder guard covers it only while its lease is held, so
/// write it before the store closes; backups copy only the database.
pub(super) const LOG_NAME: &str = "worker-stops.log";
/// The previous log, replaced each time the current one reaches `LOG_LIMIT`.
pub(super) const OLD_LOG_NAME: &str = "worker-stops.log.old";
/// About 300 stop lines; with the previous file kept, at most twice this on disk.
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
            Self::OpenSaved(_) => "opening a saved page",
            Self::SaveActive => "saving the active page",
            Self::Unsave(_) => "removing a saved page",
            Self::Folder(_) => "changing saved folders",
            Self::UpdateMetadata { .. } => "updating a tab's details",
            Self::ObserveMetadata { .. } => "recording a page's title or address",
            Self::ReopenClosed => "reopening a closed tab",
            Self::ImportBookmarks { .. } => "importing bookmarks",
            Self::SaveSettings(_) => "saving settings",
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

/// Tags an error that ends the worker's loop with the operation that raised it.
pub(super) fn during(operation: &'static str) -> impl FnOnce(SessionError) -> SessionError {
    move |error| SessionError::During {
        operation,
        error: Box::new(error),
    }
}

/// Whether the worker can go on after `error`: one refused write, a busy database, an index
/// refusal or a window that stopped listening. Lost or untrustworthy storage stops it (#9).
pub(super) fn survivable(error: &SessionError) -> bool {
    match error {
        SessionError::During { error, .. } => survivable(error),
        SessionError::Store(error) => store_survivable(error),
        // One refused row, command or clock read, or a window that stopped listening.
        SessionError::Search(_)
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

/// A store failure the next write may pass: anything else means storage is gone, damaged,
/// changed behind the worker or left inside an unfinished transaction.
fn store_survivable(error: &StoreError) -> bool {
    match error {
        StoreError::Database {
            operation: StoreOperation::Rollback,
            ..
        } => false,
        StoreError::Database { code, .. } => code.is_none_or(sqlite_code_survivable),
        StoreError::CleanupFailed { primary, cleanup } => {
            store_survivable(primary) && store_survivable(cleanup)
        }
        StoreError::InvalidEncoding(_)
        | StoreError::TooManyRecords
        | StoreError::BackupUnavailable
        | StoreError::BackupVerificationFailed
        | StoreError::InvalidBackupPath => true,
        _ => false,
    }
}

/// SQLite's primary result codes for a busy, locked or full database, or one refused row:
/// BUSY, LOCKED, NOMEM, INTERRUPT, IOERR, FULL, PROTOCOL, SCHEMA, TOOBIG, CONSTRAINT, MISMATCH.
fn sqlite_code_survivable(code: i32) -> bool {
    matches!(
        code & 0xff,
        5 | 6 | 7 | 9 | 10 | 13 | 15 | 17 | 18 | 19 | 20
    )
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

/// Writes a survived failure to stderr and, when `directory` is known, to the stop log.
pub(super) fn keep_survived(directory: Option<&Path>, error: &SessionError) {
    let line = survived_line(error);
    eprintln!("{line}");
    let Some(directory) = directory else {
        return;
    };
    if let Err(problem) = append_line(directory, &line) {
        report_log_problem("write", &problem);
    }
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
    append_line(directory, &stop_line(error))
}

/// Appends one line to the log in `directory`, moving a full log aside first.
fn append_line(directory: &Path, line: &str) -> io::Result<()> {
    let path = directory.join(LOG_NAME);
    if let Err(problem) = rotate_if_full(&path, &directory.join(OLD_LOG_NAME)) {
        report_log_problem("move aside", &problem);
    }
    let mut log = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(log, "{line}")
}

/// One stderr line naming the failed step, the error kind and the Windows code; no path.
fn report_log_problem(step: &str, problem: &io::Error) {
    eprintln!(
        "Cannot {step} the session worker's stop log ({:?}, code {:?})",
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

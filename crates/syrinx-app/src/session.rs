//! Keeps model, persistence and catalogue work off the browser's event thread.

mod deletions;
mod folders;
mod history;
mod history_repair;
mod preferences;
mod recovery;
mod retention;
mod stop;
mod synthetic;
mod usage;

pub use folders::FolderCommand;
pub use history_repair::{HistoryRepairPrompt, HistoryRepairReason};
pub use recovery::RecoveryPrompt;
pub use synthetic::SyntheticCatalogue;
pub use usage::{STATE_FOLDER, UsageControl, UsageError, UsageStatus, usage_file};

use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    num::NonZeroU64,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use history::{HistoryRecorder, Observation};
use syrinx_core::usage::{ErrorCategory, Feature, ReportPeriod, Timing, UsageDate, UsageRecorder};
use syrinx_core::{
    Activation, BookmarkImport, DestinationEdit, DestinationError, DestinationSurface,
    ImportCommit, PageOrigin, QuickDestinations, SavedPageId, SavedPageSnapshot, SnapshotError,
    TabError, TabId, TabKind, TabMetadata, TabModel, TabModelSnapshot, TabSnapshot,
    commit_bookmark_import,
};
use syrinx_platform::{DeletionJournal, JournalError, StartupStorage, WindowsDirectoryLease};
use syrinx_search::{
    Catalogue, Entry, Scope, SearchError, SearchPolicy, SearchResults, Target, Usage,
};
use syrinx_store::{BackupOutcome, HistoryVisit, LocationError, ShutdownState, Store, StoreError};
use usage::UsageCollector;

const IDLE_DELAY: Duration = Duration::from_secs(5);
const BACKUP_CHECK_INTERVAL: Duration = Duration::from_secs(60);

pub enum HomeEntryAction {
    NewTab,
    ReopenClosed,
    OpenSaved(SavedPageId),
}

pub enum SessionCommand {
    CheckPermission {
        id: u64,
        request: u64,
        origin: String,
        kind: syrinx_store::PermissionKind,
    },
    RememberPermission {
        id: u64,
        request: u64,
        origin: String,
        kind: syrinx_store::PermissionKind,
        allowed: bool,
    },
    /// Reads every remembered site permission for the settings list (DW-7).
    RememberedPermissions {
        request_id: u64,
    },
    /// Forgets one listed choice, only while it is still saved as listed (DW-7).
    ForgetPermission {
        request_id: u64,
        listed: syrinx_store::RememberedPermission,
    },
    ConfirmRecovery {
        offer_id: u64,
    },
    /// The owner agreed to clear all history so an unusable deletion journal can be rebuilt.
    ConfirmHistoryRepair {
        offer_id: u64,
    },
    HomeEntry {
        request_id: u64,
        action: HomeEntryAction,
    },
    NewTab {
        address: String,
    },
    AdoptPopup {
        request: u64,
        opener: TabId,
        address: String,
    },
    NavigateActive {
        address: String,
    },
    CloseTab(TabId),
    ObserveClosed(TabId),
    CloseActive,
    CycleActive {
        backwards: bool,
    },
    FocusTab(TabId),
    OpenSaved(SavedPageId),
    SaveActive,
    Unsave(SavedPageId),
    /// Creates, renames, removes or moves saved folders and pages (P1-21).
    Folder(FolderCommand),
    UpdateMetadata {
        id: TabId,
        metadata: TabMetadata,
    },
    ObserveMetadata {
        id: TabId,
        title: Option<String>,
        address: Option<String>,
    },
    ReopenClosed,
    /// Writes a confirmed bookmark import as one new folder of unloaded saved pages (P1-22).
    ImportBookmarks {
        request_id: u64,
        import: Box<BookmarkImport>,
        folder_title: String,
    },
    /// Inserts or replaces documented settings keys (`https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-28-29-home-settings.md`).
    SaveSettings(Vec<(String, String)>),
    /// Saves a site's zoom level by host, or forgets it with `None` at 100% (#90).
    SiteZoom {
        host: String,
        factor: Option<f64>,
    },
    /// Changes one Quick access or Favorites tile list.
    EditDestinations {
        surface: DestinationSurface,
        edit: DestinationEdit,
    },
    Query {
        request_id: u64,
        text: String,
        scope: Scope,
        policy: SearchPolicy,
    },
    /// A page's navigation finished; a failure retracts the visit its address change recorded.
    ObserveNavigation {
        id: TabId,
        success: bool,
    },
    /// Opens a remembered history address in a new tab (P1-20).
    OpenHistory(NonZeroU64),
    /// Reads at most `limit` recent visits for the history list.
    RecentHistory {
        request_id: u64,
        limit: usize,
    },
    /// Clears history visited in `[start, end)` Unix seconds through D47's journal (P1-27).
    ClearHistory {
        request_id: u64,
        start: i64,
        end: i64,
    },
    /// P1-33's acceptance run only: grows the in-memory catalogue to `total` synthetic rows.
    /// Nothing is persisted, and a worker not started with `spawn_acceptance` refuses.
    GrowSyntheticCatalogue {
        total: usize,
    },
    /// Takes D13's extra snapshot before an update installs (P1-39b), outside the daily schedule.
    SnapshotBeforeUpdate,
    /// Turns usage counting on or off, or resets it (AN-01); answered with `Usage`.
    Usage(UsageControl),
    /// Builds one period's exact report text for the preview (AN-02); answered with
    /// `UsageReport`. Writes nothing and puts nothing on the clipboard.
    UsageReport {
        request_id: u64,
        period: ReportPeriod,
    },
    /// Earlier commands finish first; commands sent after this are not accepted.
    Shutdown,
}

impl fmt::Debug for SessionCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionCommand { contents: [redacted] }")
    }
}

#[derive(Clone, Debug)]
pub struct SessionSnapshot {
    pub revision: u64,
    pub model: TabModelSnapshot,
    /// Newly allocated logical tabs; restored rows are not all loaded at startup.
    pub created: Vec<TabId>,
    /// Explicit navigation applies to this committed active tab, not an older UI selection.
    pub navigate: Option<TabId>,
    /// UI intent acknowledged by this command only, never restored from storage.
    pub home_entry: Option<u64>,
    /// Native popup request correlated only after durable tab allocation.
    pub popup: Option<(u64, Option<TabId>)>,
    pub previous_shutdown: ShutdownState,
}

#[derive(Debug)]
pub enum SessionEvent {
    Permission {
        id: u64,
        request: u64,
        result: Result<Option<bool>, StoreError>,
    },
    /// The answer to `RememberedPermissions`.
    RememberedPermissions {
        request_id: u64,
        result: Result<Vec<syrinx_store::RememberedPermission>, SessionError>,
    },
    /// The answer to `ForgetPermission`: `false` when the choice had changed or was gone.
    PermissionForgotten {
        request_id: u64,
        result: Result<bool, SessionError>,
    },
    RecoveryOffered(RecoveryPrompt),
    /// The deletion journal cannot be trusted; continuing needs the owner to clear all history.
    HistoryRepairOffered(HistoryRepairPrompt),
    /// The stored settings and tiles, sent once before the first state.
    Preferences {
        settings: BTreeMap<String, String>,
        destinations: QuickDestinations,
        /// Each site's remembered zoom level by host (#90); absent hosts are at 100%.
        site_zooms: BTreeMap<String, f64>,
    },
    /// The tile lists after an accepted edit.
    Destinations(QuickDestinations),
    PopupRejected {
        request: u64,
    },
    State(SessionSnapshot),
    Search {
        request_id: u64,
        result: Result<SearchResults, SessionError>,
    },
    /// Sent just before `Search`, only by a worker started with `spawn_acceptance`.
    QueryTimed {
        request_id: u64,
        elapsed: Duration,
    },
    /// The answer to `GrowSyntheticCatalogue`.
    SyntheticCatalogue(Result<SyntheticCatalogue, SessionError>),
    /// Sent after the import's single save committed, or when the model refused it unchanged.
    Imported {
        request_id: u64,
        result: Result<ImportCommit, SessionError>,
    },
    History {
        request_id: u64,
        result: Result<Vec<HistoryVisit>, SessionError>,
    },
    /// What a `ClearHistory` request removed, or why it failed.
    HistoryCleared {
        request_id: u64,
        result: Result<ClearedHistory, SessionError>,
    },
    Failure {
        error: SessionError,
        fatal: bool,
    },
    /// The answer to `SnapshotBeforeUpdate`; an update installs only after `Ok`.
    UpdateSnapshot(Result<(), SessionError>),
    /// Whether usage counting is on: once at start-up, then after each `Usage` command.
    Usage {
        /// The worker's real status, sent with every problem so the window never shows a stale one.
        status: UsageStatus,
        problem: Option<SessionError>,
    },
    /// The answer to `UsageReport`: the exact text a Save or Copy would export.
    UsageReport {
        request_id: u64,
        period: ReportPeriod,
        result: Result<String, SessionError>,
    },
    /// Connections have closed before this event; joining the worker is now safe.
    Stopped,
}

/// Counts only, so the dialog can say how much went without naming any of it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClearedHistory {
    pub visits: usize,
    /// Addresses left with no visit, now gone from history and search.
    pub addresses: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionError {
    Location(LocationError),
    Store(StoreError),
    Model(TabError),
    Snapshot(SnapshotError),
    Search(SearchError),
    Destination(DestinationError),
    /// The history-deletion journal refused or failed a record (D47).
    Deletion(JournalError),
    /// The usage file could not be read, written or deleted (AN-01); never fatal.
    Usage(UsageError),
    NoActiveTab,
    /// The session worker could not take a request, or stopped before answering it.
    StorageUnavailable,
    HistoryUnavailable,
    ClockUnavailable,
    WorkerStart {
        code: Option<i32>,
    },
    WorkerPanicked,
    EventReceiverClosed,
    RecoveryUnavailable,
    /// A synthetic catalogue request asked for fewer rows than present, or too many.
    SyntheticCatalogue,
    /// Only a worker started for P1-33's acceptance run takes synthetic rows.
    AcceptanceOnly,
    Recovery(String),
    RecoveryFailed {
        startup: Box<SessionError>,
        recovery: String,
    },
    CleanupFailed {
        primary: Box<SessionError>,
        cleanup: StoreError,
    },
    /// The error that ended the worker's loop, with the fixed name of what it was doing.
    During {
        operation: &'static str,
        error: Box<SessionError>,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Every nested error carries only structural fields, never browsing values.
        match self {
            Self::Location(error) => write!(formatter, "Cannot open local storage: {error}"),
            Self::Store(error) => write!(formatter, "Cannot save or read the session: {error}"),
            Self::Model(error) => write!(formatter, "Cannot change this tab: {error}"),
            Self::Snapshot(error) => write!(formatter, "Cannot restore the session: {error}"),
            Self::Search(error) => write!(formatter, "Local search failed: {error}"),
            Self::Destination(error) => write!(formatter, "Cannot change this tile: {error}"),
            Self::Deletion(error) => write!(formatter, "Cannot clear history safely: {error}"),
            Self::Usage(error) => write!(formatter, "{error}"),
            Self::NoActiveTab => formatter.write_str("There is no active page to save"),
            Self::StorageUnavailable => formatter.write_str("Local storage is unavailable"),
            Self::HistoryUnavailable => {
                formatter.write_str("This history entry is no longer available")
            }
            Self::ClockUnavailable => formatter.write_str("The system clock is unavailable"),
            Self::WorkerStart { code } => {
                write!(formatter, "Cannot start the session worker ({code:?})")
            }
            Self::WorkerPanicked => formatter.write_str("The session worker stopped unexpectedly"),
            Self::EventReceiverClosed => formatter.write_str("The session event receiver closed"),
            Self::RecoveryUnavailable => {
                formatter.write_str("This recovery decision is no longer available")
            }
            Self::SyntheticCatalogue => {
                formatter.write_str("The synthetic catalogue cannot take that many rows")
            }
            Self::AcceptanceOnly => {
                formatter.write_str("Synthetic rows are only for the palette acceptance run")
            }
            Self::Recovery(error) => write!(formatter, "Cannot restore the backup: {error}"),
            Self::RecoveryFailed { startup, recovery } => {
                write!(
                    formatter,
                    "{startup}; backup recovery is unavailable: {recovery}"
                )
            }
            Self::CleanupFailed { primary, cleanup } => {
                write!(formatter, "{primary}; shutdown also failed: {cleanup}")
            }
            Self::During { operation, error } => write!(formatter, "{error} (while {operation})"),
        }
    }
}

impl std::error::Error for SessionError {}

#[must_use = "send Shutdown and join after receiving Stopped"]
pub struct SessionHandle {
    pub commands: mpsc::Sender<SessionCommand>,
    worker: JoinHandle<Result<(), SessionError>>,
}

impl SessionHandle {
    /// Call after `Stopped`, or from a shutdown worker; joining before then blocks.
    pub fn join(self) -> Result<(), SessionError> {
        drop(self.commands);
        self.worker
            .join()
            .map_err(|_| SessionError::WorkerPanicked)?
    }
}

/// Opens only the explicitly supplied directory, on the worker, with the platform guard.
/// `usage` counts only if that directory holds a usage file (AN-01).
pub fn spawn(
    data_dir: PathBuf,
    usage: Arc<UsageRecorder>,
) -> Result<(SessionHandle, UnboundedReceiver<SessionEvent>), SessionError> {
    spawn_worker(data_dir, usage, false)
}

/// As `spawn`, for P1-33's acceptance run: only this worker accepts synthetic rows and
/// reports each query's catalogue time.
pub fn spawn_acceptance(
    data_dir: PathBuf,
    usage: Arc<UsageRecorder>,
) -> Result<(SessionHandle, UnboundedReceiver<SessionEvent>), SessionError> {
    spawn_worker(data_dir, usage, true)
}

fn spawn_worker(
    data_dir: PathBuf,
    usage: Arc<UsageRecorder>,
    acceptance: bool,
) -> Result<(SessionHandle, UnboundedReceiver<SessionEvent>), SessionError> {
    let (commands, receiver) = mpsc::channel();
    let (events, event_receiver) = unbounded();
    let worker = thread::Builder::new()
        .name("syrinx-session".to_owned())
        .spawn(move || {
            let result = run(data_dir, usage, acceptance, receiver, &events);
            finish_worker(result, &events)
        })
        .map_err(|error| SessionError::WorkerStart {
            code: error.raw_os_error(),
        })?;
    Ok((SessionHandle { commands, worker }, event_receiver))
}

fn finish_worker(
    result: Result<(), SessionError>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    if let Err(error) = &result {
        // The reason is logged first, so a console still has it when the window is gone.
        stop::log_stop(error);
        let failure = SessionEvent::Failure {
            error: error.clone(),
            fatal: true,
        };
        // A failed receiver is still visible to the caller through the worker's join result.
        if emit(events, failure).is_err() {
            return result;
        }
    }
    let delivered = emit(events, SessionEvent::Stopped);
    result.and(delivered)
}

fn run(
    data_dir: PathBuf,
    usage: Arc<UsageRecorder>,
    acceptance: bool,
    commands: mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    let mut startup = StartupFacts::default();
    let Some((store, mut worker)) = startup_session(&data_dir, &commands, events, &mut startup)?
    else {
        return Ok(());
    };
    worker.acceptance = acceptance;
    // The guard has accepted the folder; `serve_accepted` keeps any stop beside the database.
    serve_accepted(usage, startup, store, worker, commands, events, &data_dir)
}

/// Serves commands on the accepted folder. The one place a stop is kept in the log, written
/// before the store closes so the guard's lease is still held, as for usage.json.
fn serve_accepted(
    usage: Arc<UsageRecorder>,
    startup: StartupFacts,
    store: Store,
    mut worker: Worker,
    commands: mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
    data_dir: &std::path::Path,
) -> Result<(), SessionError> {
    let result = serve_counted(
        &usage,
        startup,
        &store,
        &mut worker,
        commands,
        events,
        data_dir,
    );
    if let Err(error) = &result {
        stop::keep_stop(data_dir, error);
    }
    // A shutdown failure after the line is written goes to the window and stderr only.
    finish_store(result, store.shutdown())
}

/// Loads usage counting, serves until the loop ends and saves the counts.
fn serve_counted(
    usage: &Arc<UsageRecorder>,
    startup: StartupFacts,
    store: &Store,
    worker: &mut Worker,
    commands: mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
    data_dir: &std::path::Path,
) -> Result<(), SessionError> {
    let mut collector = start_usage(data_dir, usage, store, startup, events)?;
    worker.usage = usage.clone();
    let result = worker.serve(store, commands, events, &mut collector);
    // Counts are saved even after a storage failure; a failed save is reported, not fatal.
    let saved = collector.finish(Instant::now());
    match (result, saved) {
        (Ok(()), Err(error)) => emit_recoverable(events, SessionError::Usage(error)),
        (Err(primary), Err(error)) => {
            // The storage failure is the one the window shows; this one is only logged.
            eprintln!("{error}");
            Err(primary)
        }
        (result, Ok(())) => result,
    }
}

/// What happened while opening storage, counted once usage counting is known to be on.
#[derive(Default)]
struct StartupFacts {
    storage_open_failed: bool,
    recovery_offered: bool,
}

/// Loads the usage file beside the database, records start-up's facts and reports its status.
fn start_usage(
    data_dir: &std::path::Path,
    usage: &Arc<UsageRecorder>,
    store: &Store,
    startup: StartupFacts,
    events: &UnboundedSender<SessionEvent>,
) -> Result<UsageCollector, SessionError> {
    let (collector, problem) =
        UsageCollector::load(usage.clone(), data_dir, local_today(), Instant::now());
    if startup.storage_open_failed {
        usage.error(ErrorCategory::StorageOpen);
    }
    if startup.recovery_offered {
        usage.error(ErrorCategory::RecoveryOffered);
    }
    if store.previous_shutdown() == ShutdownState::Unclean {
        usage.error(ErrorCategory::UncleanShutdown);
    }
    emit(
        events,
        SessionEvent::Usage {
            status: collector.status(),
            problem: problem.map(SessionError::Usage),
        },
    )?;
    Ok(collector)
}

/// Today's local date; the UTC day when Windows cannot say, and `None` only without any clock.
fn local_today() -> Option<UsageDate> {
    match syrinx_platform::calendar::local_iso_date() {
        Ok(text) => UsageDate::parse(&text),
        Err(error) => {
            // Only the Windows error code is logged; the UTC day is at most one day off.
            eprintln!(
                "Windows could not report today's local date ({:?})",
                error.code()
            );
            utc_today()
        }
    }
}

/// The UTC calendar day, used only when Windows cannot report the local one.
fn utc_today() -> Option<UsageDate> {
    let seconds = match utc_seconds() {
        Ok(seconds) => seconds,
        Err(_) => return None,
    };
    let days = i32::try_from(seconds.div_euclid(86_400));
    let epoch = UsageDate::from_ymd(1970, 1, 1);
    match (days, epoch) {
        (Ok(days), Some(epoch)) => Some(epoch.plus_days(days)),
        _ => None,
    }
}

fn startup_session(
    data_dir: &std::path::Path,
    commands: &mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
    facts: &mut StartupFacts,
) -> Result<Option<(Store, Worker)>, SessionError> {
    // The guarded preflight retains either the existing primary or the exact recovery offer.
    let confirmed = match recovery::prepare_startup(data_dir)? {
        StartupStorage::Ready(lease) => match open_session(lease, false) {
            Ok(session) => return Ok(Some(session)),
            Err(error) if history_repair::repairable(&error).is_some() => {
                return open_with_repair(data_dir, error, commands, events);
            }
            Err(error) => {
                facts.storage_open_failed = true;
                recovery::confirm_failed_start(data_dir, error, commands, events)?
            }
        },
        StartupStorage::Recovery(offer) => recovery::confirm_offer(offer, commands, events)?,
    };
    // Both routes reach here only after offering a backup.
    facts.recovery_offered = true;
    if !confirmed {
        return Ok(None);
    }
    // A confirmed restore gets one guarded reopen, never an automatic second offer.
    let StartupStorage::Ready(lease) = recovery::prepare_startup(data_dir)? else {
        return Err(SessionError::RecoveryUnavailable);
    };
    match open_session(lease, false) {
        Err(error) if history_repair::repairable(&error).is_some() => {
            open_with_repair(data_dir, error, commands, events)
        }
        result => result.map(Some),
    }
}

/// Asks the owner to clear all history; on confirmation rebuilds the journal and opens.
fn open_with_repair(
    data_dir: &std::path::Path,
    error: SessionError,
    commands: &mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<Option<(Store, Worker)>, SessionError> {
    let Some(reason) = history_repair::repairable(&error) else {
        return Err(error);
    };
    if !history_repair::confirm(reason, commands, events)? {
        return Ok(None);
    }
    let StartupStorage::Ready(lease) = recovery::prepare_startup(data_dir)? else {
        return Err(SessionError::RecoveryUnavailable);
    };
    open_session(lease, true).map(Some)
}

/// Opens the store and the deletion journal; `repair` rebuilds the journal and clears all
/// history first, which the owner has confirmed.
fn open_session(
    lease: WindowsDirectoryLease,
    repair: bool,
) -> Result<(Store, Worker), SessionError> {
    let lease = Arc::new(lease);
    let store = Store::open(lease.clone()).map_err(SessionError::Store)?;
    let opened =
        open_journal(&store, &lease, repair).and_then(|journal| Worker::restore(&store, journal));
    match opened {
        Ok(worker) => Ok((store, worker)),
        Err(error) => {
            // Recovery cannot overlap live connections or conceal a failed close.
            finish_store(Err(error), store.shutdown())
        }
    }
}

/// The journal lives beside the database and is held for the whole session (D47).
fn open_journal(
    store: &Store,
    lease: &WindowsDirectoryLease,
    repair: bool,
) -> Result<DeletionJournal, SessionError> {
    if !repair {
        return DeletionJournal::open(lease).map_err(SessionError::Deletion);
    }
    let applied = store
        .history_deletion_generation()
        .map_err(SessionError::Store)?;
    DeletionJournal::rebuild(lease, applied).map_err(SessionError::Deletion)
}

fn finish_store<T>(
    result: Result<T, SessionError>,
    cleanup: Result<(), StoreError>,
) -> Result<T, SessionError> {
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(SessionError::Store(error)),
        (Err(error), Ok(())) => Err(error),
        (Err(primary), Err(cleanup)) => Err(SessionError::CleanupFailed {
            primary: Box::new(primary),
            cleanup,
        }),
    }
}

struct Worker {
    model: TabModel,
    catalogue: Catalogue,
    /// Tabs adopted from a popup, which the palette names as the sidebar does.
    popup_tabs: HashSet<TabId>,
    history: HistoryRecorder,
    deletions: DeletionJournal,
    /// When history retention next expires old visits, and how far it last reached (DW-4).
    retention: retention::Schedule,
    revision: u64,
    previous_shutdown: ShutdownState,
    destinations: QuickDestinations,
    /// The settings read at startup, handed to the window once.
    startup_settings: BTreeMap<String, String>,
    /// The per-site zoom levels read at startup, handed to the window once (#90).
    startup_site_zooms: BTreeMap<String, f64>,
    /// P1-33's synthetic rows in the catalogue; non-zero only in an acceptance run.
    synthetic_rows: usize,
    /// Set only by `spawn_acceptance`: synthetic rows are accepted and queries are timed.
    acceptance: bool,
    /// The shared usage counters (AN-01); they count only while the owner has them on.
    usage: Arc<UsageRecorder>,
}

impl Worker {
    fn restore(store: &Store, mut deletions: DeletionJournal) -> Result<Self, SessionError> {
        // Cleared history must be gone before the catalogue or any list can read it.
        deletions::reconcile(store, &mut deletions)?;
        let mut stored_preferences = preferences::restore(store)?;
        let site_zooms = preferences::restore_site_zooms(store)?;
        // Expired visits go before the catalogue loads; a large backlog continues while idle.
        let mut schedule = retention::Schedule::take_saved(&mut stored_preferences.settings);
        retention::expire_at_startup(
            store,
            &mut deletions,
            utc_seconds()?,
            &mut schedule,
            retention::EXPIRY_BATCH,
        )?;
        let stored = store.load().map_err(SessionError::Store)?;
        let model = TabModel::from_snapshot(stored.snapshot).map_err(SessionError::Snapshot)?;
        let snapshot = model.snapshot();
        let mut catalogue = Catalogue::new();
        sync_catalogue(
            &mut catalogue,
            &HashSet::new(),
            &TabModelSnapshot::default(),
            &snapshot,
        )?;
        // Durable history joins the catalogue once; later visits update it row by row.
        // Retention keeps this load to about 90 days of history.
        for entry in store.history_entries().map_err(SessionError::Store)? {
            catalogue
                .upsert(history::history_entry(&entry))
                .map_err(SessionError::Search)?;
        }
        Ok(Self {
            model,
            catalogue,
            popup_tabs: HashSet::new(),
            history: HistoryRecorder::restored(&snapshot),
            deletions,
            retention: schedule,
            revision: stored.revision,
            previous_shutdown: store.previous_shutdown(),
            destinations: stored_preferences.destinations,
            startup_settings: stored_preferences.settings,
            startup_site_zooms: site_zooms,
            synthetic_rows: 0,
            acceptance: false,
            usage: Arc::default(),
        })
    }

    fn serve(
        &mut self,
        store: &Store,
        commands: mpsc::Receiver<SessionCommand>,
        events: &UnboundedSender<SessionEvent>,
        usage: &mut UsageCollector,
    ) -> Result<(), SessionError> {
        emit(
            events,
            SessionEvent::Preferences {
                settings: std::mem::take(&mut self.startup_settings),
                destinations: self.destinations.clone(),
                site_zooms: std::mem::take(&mut self.startup_site_zooms),
            },
        )
        .map_err(stop::during("sending the saved settings"))?;
        self.publish(events, self.model.snapshot(), Vec::new(), None, None, None)
            .map_err(stop::during("sending the restored tabs"))?;
        let mut delay = IDLE_DELAY;
        loop {
            // Held page titles are written once they settle, even while nothing else happens.
            let wait = if self.history.has_pending_titles() {
                delay.min(history::TITLE_SETTLE)
            } else {
                delay
            };
            let received = commands.recv_timeout(wait);
            // The worker's existing wake-ups carry the five-minute usage write; it adds none,
            // and reads no clock unless a write is waiting.
            if usage.pending()
                && let Err(error) = usage.write_if_due(Instant::now())
            {
                emit_recoverable(events, SessionError::Usage(error))
                    .map_err(stop::during("reporting a usage write failure"))?;
            }
            match received {
                Ok(SessionCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // A title still held at exit most likely names the tab's current page.
                    return self
                        .settle_titles(store, true, events)
                        .map_err(stop::during("saving page titles at shutdown"));
                }
                Ok(SessionCommand::Usage(control)) => {
                    let (status, problem) = usage.control(control, local_today(), Instant::now());
                    let problem = problem.map(SessionError::Usage);
                    emit(events, SessionEvent::Usage { status, problem })
                        .map_err(stop::during(stop::CHANGING_USAGE))?;
                }
                Ok(SessionCommand::UsageReport { request_id, period }) => {
                    let result = usage
                        .report(period, local_today())
                        .map_err(SessionError::Usage);
                    let answer = SessionEvent::UsageReport {
                        request_id,
                        period,
                        result,
                    };
                    emit(events, answer).map_err(stop::during(stop::BUILDING_USAGE_REPORT))?;
                }
                Ok(command) => {
                    let operation = command.operation();
                    self.handle(store, command, events)
                        .map_err(stop::during(operation))?;
                    delay = IDLE_DELAY;
                }
                Err(mpsc::RecvTimeoutError::Timeout) if self.history.has_pending_titles() => {
                    self.settle_titles(store, false, events)
                        .map_err(stop::during("saving settled page titles"))?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let now = utc_seconds();
                    // A clock failure is reported once, by the backup check below.
                    if let Ok(now) = &now {
                        self.expire_while_idle(store, *now, retention::EXPIRY_BATCH, events)
                            .map_err(stop::during("expiring old history"))?;
                    }
                    // The store retains its daily due state; retries remain idle and bounded.
                    backup_while_idle(store, now, &self.usage, events)
                        .map_err(stop::during("taking the daily backup"))?;
                    delay = BACKUP_CHECK_INTERVAL;
                }
            }
        }
    }

    fn settle_titles(
        &mut self,
        store: &Store,
        force: bool,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let result = self.history.settle(
            store,
            &mut self.catalogue,
            Instant::now(),
            force,
            utc_seconds,
        );
        route_history_result(result, events)
    }

    fn handle(
        &mut self,
        store: &Store,
        command: SessionCommand,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        // Titles that settled before this command name the page they were reported on.
        self.settle_titles(store, false, events)?;
        let command = match command {
            SessionCommand::ConfirmRecovery { .. }
            | SessionCommand::ConfirmHistoryRepair { .. } => {
                return recovery::reject_command(events);
            }
            SessionCommand::ObserveNavigation { id, success } => {
                let result = self.history.navigation(
                    store,
                    &mut self.catalogue,
                    id,
                    success,
                    Instant::now(),
                    utc_seconds,
                );
                return route_history_result(result, events);
            }
            SessionCommand::Query {
                request_id,
                text,
                scope,
                policy,
            } => return self.answer_query(request_id, &text, scope, policy, events),
            SessionCommand::GrowSyntheticCatalogue { total } => {
                return self.grow_synthetic(total, events);
            }
            SessionCommand::RecentHistory { request_id, limit } => {
                let result = store.recent_visits(limit).map_err(SessionError::Store);
                return emit(events, SessionEvent::History { request_id, result });
            }
            SessionCommand::SnapshotBeforeUpdate => {
                let result = utc_seconds().and_then(|now| {
                    store
                        .backup_before_update(now)
                        .map(drop)
                        .map_err(SessionError::Store)
                });
                return emit(events, SessionEvent::UpdateSnapshot(result));
            }
            SessionCommand::ClearHistory {
                request_id,
                start,
                end,
            } => return self.clear_history(store, request_id, start, end, events),
            SessionCommand::OpenHistory(id) => match self.history_destination(id) {
                Ok(command) => command,
                Err(error) => return emit_recoverable(events, error),
            },
            SessionCommand::ImportBookmarks {
                request_id,
                import,
                folder_title,
            } => return self.import_bookmarks(store, request_id, &import, folder_title, events),
            SessionCommand::CheckPermission {
                id,
                request,
                origin,
                kind,
            } => {
                let result = store.permission(&origin, kind);
                return emit_permission(events, id, request, result);
            }
            SessionCommand::RememberPermission {
                id,
                request,
                origin,
                kind,
                allowed,
            } => {
                let result = store
                    .remember_permission(origin, kind, allowed)
                    .map(|()| Some(allowed));
                return emit_permission(events, id, request, result);
            }
            SessionCommand::RememberedPermissions { request_id } => {
                let result = store.remembered_permissions().map_err(SessionError::Store);
                return emit(
                    events,
                    SessionEvent::RememberedPermissions { request_id, result },
                );
            }
            SessionCommand::ForgetPermission { request_id, listed } => {
                let result = store
                    .forget_listed_permission(listed)
                    .map_err(SessionError::Store);
                return emit(
                    events,
                    SessionEvent::PermissionForgotten { request_id, result },
                );
            }
            command => command,
        };
        let Some(command) = preferences::intercept(store, &mut self.destinations, command, events)?
        else {
            return Ok(());
        };

        // Invalid or stale commands do not mutate the model and remain recoverable UI errors.
        let navigating = matches!(&command, SessionCommand::NavigateActive { .. });
        let home_entry = match &command {
            SessionCommand::HomeEntry { request_id, .. } => Some(*request_id),
            _ => None,
        };
        let observation = match &command {
            SessionCommand::ObserveMetadata { id, title, address } => Some(Observation {
                id: *id,
                title: title.clone(),
                address: address.clone(),
            }),
            _ => None,
        };
        let popup_request = match &command {
            SessionCommand::AdoptPopup { request, .. } => Some(*request),
            _ => None,
        };
        let counted = counted_feature(&command);
        let before = self.model.snapshot();
        if let Err(error) = apply_model(&mut self.model, command) {
            // A refused popup must not leave its native request waiting for a tab.
            if let Some(request) = popup_request {
                emit(events, SessionEvent::PopupRejected { request })?;
            }
            return emit_recoverable(events, error);
        }
        let after = self.persist(store, &before)?;
        self.history.retain(&after);
        if let Some(observation) = observation {
            self.record_history(store, &after, observation, events)?;
        }
        let created: Vec<_> = after
            .tabs
            .iter()
            .filter(|tab| tab.id.get() > before.last_tab_id)
            .map(|tab| tab.id)
            .collect();
        self.count_change(counted, before != after, !created.is_empty());
        let navigate = if navigating { after.active_id } else { None };
        let popup = popup_request.map(|request| (request, created.first().copied()));
        if let Some((_, Some(tab))) = popup {
            self.register_popup_tab(&after, tab)?;
        }
        self.publish(events, after, created, navigate, home_entry, popup)
    }

    /// Remembers an adopted popup and refreshes its palette row, which `persist` indexed
    /// before the tab was known to be a popup.
    fn register_popup_tab(
        &mut self,
        after: &TabModelSnapshot,
        id: TabId,
    ) -> Result<(), SessionError> {
        self.popup_tabs.insert(id);
        let Some(tab) = after.tabs.iter().find(|tab| tab.id == id) else {
            return Ok(());
        };
        upsert_tab(&mut self.catalogue, &self.popup_tabs, tab)
    }

    /// Counts an accepted command (AN-01): a tab-opening one only if it opened a tab, any
    /// other only if it changed the model.
    fn count_change(&self, counted: Counted, changed: bool, created: bool) {
        match counted {
            Counted::IfCreated(feature) if created => self.usage.count(feature),
            Counted::IfChanged(feature) if changed => self.usage.count(feature),
            Counted::IfCreated(_) | Counted::IfChanged(_) | Counted::Nothing => {}
        }
    }

    /// Saves the model if it changed since `before`, in one store transaction, then reindexes.
    fn persist(
        &mut self,
        store: &Store,
        before: &TabModelSnapshot,
    ) -> Result<TabModelSnapshot, SessionError> {
        let after = self.model.snapshot();
        if *before != after {
            // The clock is read only while the owner counts usage.
            let started = self.usage.is_enabled().then(Instant::now);
            let saved = store.save(self.revision, after.clone());
            if saved.is_err() {
                self.usage.error(ErrorCategory::StorageWrite);
            }
            self.revision = saved.map_err(SessionError::Store)?;
            if let Some(started) = started {
                self.usage.time(Timing::StoreSave, started.elapsed());
            }
            sync_catalogue(&mut self.catalogue, &self.popup_tabs, before, &after)?;
        }
        Ok(after)
    }

    /// Adds the whole import in memory, then commits it with a single save.
    fn import_bookmarks(
        &mut self,
        store: &Store,
        request_id: u64,
        import: &BookmarkImport,
        folder_title: String,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let before = self.model.snapshot();
        // A refused import leaves the model unchanged and is not fatal.
        let commit = match commit_bookmark_import(&mut self.model, import, folder_title) {
            Ok(commit) => commit,
            Err(error) => {
                let result = Err(SessionError::Model(error));
                return emit(events, SessionEvent::Imported { request_id, result });
            }
        };
        let after = self.persist(store, &before)?;
        self.usage.count(Feature::ImportCompleted);
        let result = Ok(commit);
        emit(events, SessionEvent::Imported { request_id, result })?;
        self.publish(events, after, Vec::new(), None, None, None)
    }

    fn answer_query(
        &mut self,
        request_id: u64,
        text: &str,
        scope: Scope,
        policy: SearchPolicy,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        // Only an acceptance worker, or one counting usage, reads a clock here.
        let started = (self.acceptance || self.usage.is_enabled()).then(Instant::now);
        let result = utc_seconds().and_then(|now| {
            self.catalogue
                .query(text, scope, now, policy)
                .map_err(SessionError::Search)
        });
        if let Some(started) = started {
            self.usage.time(Timing::SearchTier1, started.elapsed());
        }
        if let Some(started) = started.filter(|_| self.acceptance) {
            let elapsed = started.elapsed();
            emit(
                events,
                SessionEvent::QueryTimed {
                    request_id,
                    elapsed,
                },
            )?;
        }
        // Failures carry the same identity as successful replies, so stale queries stay silent.
        emit(events, SessionEvent::Search { request_id, result })
    }

    /// Grows P1-33's in-memory synthetic rows; any other worker refuses.
    fn grow_synthetic(
        &mut self,
        total: usize,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        if !self.acceptance {
            let refused = Err(SessionError::AcceptanceOnly);
            return emit(events, SessionEvent::SyntheticCatalogue(refused));
        }
        let started = Instant::now();
        let result = utc_seconds()
            .and_then(|now| {
                synthetic::grow(&mut self.catalogue, &mut self.synthetic_rows, total, now)
            })
            .map(|()| SyntheticCatalogue {
                synthetic_rows: self.synthetic_rows,
                catalogue_rows: self.catalogue.len(),
                elapsed: started.elapsed(),
            });
        emit(events, SessionEvent::SyntheticCatalogue(result))
    }

    fn clear_history(
        &mut self,
        store: &Store,
        request_id: u64,
        start: i64,
        end: i64,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let result = deletions::clear(
            store,
            &mut self.deletions,
            &mut self.catalogue,
            &mut self.history,
            start,
            end,
        );
        // The dialog always hears the outcome; storage failures still stop the session.
        let fatal = match &result {
            Err(SessionError::Store(error)) => Some(SessionError::Store(error.clone())),
            _ => None,
        };
        emit(events, SessionEvent::HistoryCleared { request_id, result })?;
        fatal.map_or(Ok(()), Err)
    }

    /// Expires history past the retention when due, one bounded batch per idle check.
    fn expire_while_idle(
        &mut self,
        store: &Store,
        now: i64,
        limit: usize,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        if self.retention.waiting(now) {
            return Ok(());
        }
        let expired = retention::expire_live(
            store,
            &mut self.deletions,
            &mut self.catalogue,
            &mut self.history,
            now,
            &mut self.retention,
            limit,
        );
        match expired {
            Ok(()) => Ok(()),
            // Storage failures stop the session, as they do for an owner's clear.
            Err(error @ SessionError::Store(_)) => Err(error),
            Err(error) => {
                // A journal refusal is shown once a day rather than at every idle check.
                self.retention.due = now.saturating_add(retention::DAY_SECS);
                emit_recoverable(events, error)
            }
        }
    }

    fn history_destination(&self, id: NonZeroU64) -> Result<SessionCommand, SessionError> {
        // The catalogue mirrors durable history, so it already holds the address to open.
        let entry = self
            .catalogue
            .get(Target::History(id))
            .ok_or(SessionError::HistoryUnavailable)?;
        Ok(SessionCommand::NewTab {
            address: entry.address.clone(),
        })
    }

    fn record_history(
        &mut self,
        store: &Store,
        model: &TabModelSnapshot,
        observation: Observation,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let result = self.history.observe(
            store,
            &mut self.catalogue,
            model,
            observation,
            Instant::now(),
            utc_seconds,
        );
        route_history_result(result, events)
    }

    /// Sends `model`, the snapshot just persisted, so a large saved tree is not copied again.
    fn publish(
        &self,
        events: &UnboundedSender<SessionEvent>,
        model: TabModelSnapshot,
        created: Vec<TabId>,
        navigate: Option<TabId>,
        home_entry: Option<u64>,
        popup: Option<(u64, Option<TabId>)>,
    ) -> Result<(), SessionError> {
        emit(
            events,
            SessionEvent::State(SessionSnapshot {
                revision: self.revision,
                model,
                created,
                navigate,
                home_entry,
                popup,
                previous_shutdown: self.previous_shutdown,
            }),
        )
    }
}

fn backup_while_idle(
    store: &Store,
    now: Result<i64, SessionError>,
    usage: &UsageRecorder,
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    let started = usage.is_enabled().then(Instant::now);
    let backup = now.and_then(|now| store.backup_if_due(now, true).map_err(SessionError::Store));
    // Only a snapshot actually taken is timed; a clock failure is not a failed backup.
    match &backup {
        Ok(BackupOutcome::Created(_)) => {
            if let Some(started) = started {
                usage.time(Timing::BackupSnapshot, started.elapsed());
            }
        }
        Err(SessionError::Store(_)) => usage.error(ErrorCategory::BackupFailed),
        Ok(_) | Err(_) => {}
    }
    if let Err(error) = backup {
        emit(
            events,
            SessionEvent::Failure {
                error,
                fatal: false,
            },
        )?;
    }
    Ok(())
}

/// What an accepted tab command adds to the usage counts (AN-01).
#[derive(Clone, Copy)]
enum Counted {
    /// Only when the command opened a new tab instance.
    IfCreated(Feature),
    /// Only when the command changed the model.
    IfChanged(Feature),
    Nothing,
}

/// The feature counter a tab command can count; switching by click or key is counted in the
/// window, which knows whether a key or a palette command asked.
fn counted_feature(command: &SessionCommand) -> Counted {
    use HomeEntryAction as Home;
    match command {
        SessionCommand::NewTab { .. }
        | SessionCommand::AdoptPopup { .. }
        | SessionCommand::HomeEntry {
            action: Home::NewTab,
            ..
        } => Counted::IfCreated(Feature::TabCreated),
        SessionCommand::ReopenClosed
        | SessionCommand::HomeEntry {
            action: Home::ReopenClosed,
            ..
        } => Counted::IfCreated(Feature::TabReopened),
        SessionCommand::OpenSaved(_)
        | SessionCommand::HomeEntry {
            action: Home::OpenSaved(_),
            ..
        } => Counted::IfCreated(Feature::SavedOpenedUnloaded),
        SessionCommand::CloseTab(_)
        | SessionCommand::CloseActive
        | SessionCommand::ObserveClosed(_) => Counted::IfChanged(Feature::TabClosed),
        SessionCommand::SaveActive => Counted::IfChanged(Feature::SavedSaved),
        SessionCommand::Unsave(_) => Counted::IfChanged(Feature::SavedRemoved),
        SessionCommand::Folder(FolderCommand::Create { .. }) => {
            Counted::IfChanged(Feature::FolderCreated)
        }
        SessionCommand::Folder(
            FolderCommand::Remove { .. } | FolderCommand::DeleteWithContents { .. },
        ) => Counted::IfChanged(Feature::FolderRemoved),
        _ => Counted::Nothing,
    }
}

fn apply_model(model: &mut TabModel, command: SessionCommand) -> Result<(), SessionError> {
    match command {
        SessionCommand::HomeEntry { action, .. } => {
            // UI correlation does not change the authoritative tab lifecycle.
            let command = match action {
                HomeEntryAction::NewTab => SessionCommand::NewTab {
                    address: "about:blank".into(),
                },
                HomeEntryAction::ReopenClosed => SessionCommand::ReopenClosed,
                HomeEntryAction::OpenSaved(id) => SessionCommand::OpenSaved(id),
            };
            apply_model(model, command)?;
        }
        SessionCommand::NavigateActive { address } => {
            // Resolve queued selection/close/new-tab actions before choosing the navigation target.
            let resolved = match model.active_id() {
                Some(id) => SessionCommand::UpdateMetadata {
                    id,
                    metadata: TabMetadata {
                        address,
                        ..TabMetadata::default()
                    },
                },
                None => SessionCommand::NewTab { address },
            };
            apply_model(model, resolved)?;
        }
        SessionCommand::AdoptPopup {
            opener, address, ..
        } => {
            if model
                .tabs()
                .iter()
                .any(|tab| tab.id() == opener && tab.kind() == TabKind::Web)
            {
                apply_model(model, SessionCommand::NewTab { address })?;
            }
        }
        SessionCommand::NewTab { address } => {
            model
                .open(
                    TabKind::Web,
                    TabMetadata {
                        title: address.clone(),
                        address,
                        ..TabMetadata::default()
                    },
                    Activation::Foreground,
                )
                .map_err(SessionError::Model)?;
        }
        SessionCommand::CloseTab(id) => {
            model.close(id).map_err(SessionError::Model)?;
        }
        SessionCommand::ObserveClosed(id) => {
            // Native callbacks refer to one web instance, never a replacement or plugin tab.
            if model
                .tabs()
                .iter()
                .any(|tab| tab.id() == id && tab.kind() == TabKind::Web)
            {
                model.close(id).map_err(SessionError::Model)?;
            }
        }
        SessionCommand::CloseActive => {
            if let Some(id) = model.active_id() {
                model.close(id).map_err(SessionError::Model)?;
            }
        }
        SessionCommand::CycleActive { backwards } => cycle_active(model, backwards)?,
        SessionCommand::FocusTab(id) => model.select(id).map_err(SessionError::Model)?,
        SessionCommand::OpenSaved(id) => {
            model
                .open_saved(id, Activation::Foreground)
                .map_err(SessionError::Model)?;
        }
        SessionCommand::SaveActive => {
            let id = model.active_id().ok_or(SessionError::NoActiveTab)?;
            model.save_page(id).map_err(SessionError::Model)?;
        }
        SessionCommand::Unsave(id) => {
            model.remove_saved(id).map_err(SessionError::Model)?;
        }
        SessionCommand::Folder(command) => command.apply(model).map_err(SessionError::Model)?,
        SessionCommand::UpdateMetadata { id, metadata } => model
            .update_metadata(id, metadata)
            .map_err(SessionError::Model)?,
        SessionCommand::ObserveMetadata { id, title, address } => {
            // Merge callbacks against the worker's latest state, never an older UI snapshot.
            let Some(tab) = model.tabs().iter().find(|tab| tab.id() == id) else {
                return Ok(());
            };
            let mut metadata = tab.metadata().clone();
            if let Some(title) = title {
                metadata.title = title;
            }
            if let Some(address) = address {
                metadata.address = address;
            }
            model
                .update_metadata(id, metadata)
                .map_err(SessionError::Model)?;
        }
        SessionCommand::ReopenClosed => {
            model.reopen_closed().map_err(SessionError::Model)?;
        }
        SessionCommand::ConfirmRecovery { .. } | SessionCommand::ConfirmHistoryRepair { .. } => {
            return Err(SessionError::RecoveryUnavailable);
        }
        // Reads, history lookups, permissions and preferences are answered before the model.
        SessionCommand::Query { .. }
        | SessionCommand::CheckPermission { .. }
        | SessionCommand::RememberPermission { .. }
        | SessionCommand::RememberedPermissions { .. }
        | SessionCommand::ForgetPermission { .. }
        | SessionCommand::ImportBookmarks { .. }
        | SessionCommand::RecentHistory { .. }
        | SessionCommand::ClearHistory { .. }
        | SessionCommand::OpenHistory(_)
        | SessionCommand::ObserveNavigation { .. }
        | SessionCommand::SaveSettings(_)
        | SessionCommand::SiteZoom { .. }
        | SessionCommand::EditDestinations { .. }
        | SessionCommand::GrowSyntheticCatalogue { .. }
        | SessionCommand::SnapshotBeforeUpdate
        | SessionCommand::Usage(_)
        | SessionCommand::UsageReport { .. }
        | SessionCommand::Shutdown => {}
    }
    Ok(())
}

fn cycle_active(model: &mut TabModel, backwards: bool) -> Result<(), SessionError> {
    // Resolve each queued key press against current state, not the UI's last save acknowledgment.
    let Some(active) = model.active_id() else {
        return Ok(());
    };
    let current = model
        .tabs()
        .iter()
        .position(|tab| tab.id() == active)
        .ok_or(SessionError::Model(TabError::UnknownTab(active)))?;
    let last = model.tabs().len() - 1;
    let next = match (backwards, current) {
        (true, 0) => last,
        (true, _) => current - 1,
        (false, index) if index == last => 0,
        (false, _) => current + 1,
    };
    model
        .select(model.tabs()[next].id())
        .map_err(SessionError::Model)
}

fn sync_catalogue(
    catalogue: &mut Catalogue,
    popups: &HashSet<TabId>,
    before: &TabModelSnapshot,
    after: &TabModelSnapshot,
) -> Result<(), SessionError> {
    // The feed contains only model-derived rows; it creates no browsing history or usage log.
    sync_tabs(catalogue, popups, before, after)?;
    let open_before = open_saved_pages(before);
    let open_after = open_saved_pages(after);
    // Most changes touch only live tabs; then only saved rows whose open state flipped change.
    if before.saved_pages == after.saved_pages {
        let flipped: HashSet<SavedPageId> = open_before
            .symmetric_difference(&open_after)
            .copied()
            .collect();
        let changed = after
            .saved_pages
            .iter()
            .filter(|page| flipped.contains(&page.id));
        return upsert_saved(catalogue, changed, &open_after);
    }
    let kept: HashSet<SavedPageId> = after.saved_pages.iter().map(|page| page.id).collect();
    for page in &before.saved_pages {
        if !kept.contains(&page.id) {
            catalogue
                .remove(Target::Saved(page.id))
                .map_err(SessionError::Search)?;
        }
    }
    upsert_saved(catalogue, after.saved_pages.iter(), &open_after)
}

/// Removes closed tabs and upserts open ones; the catalogue skips unchanged entries.
fn sync_tabs(
    catalogue: &mut Catalogue,
    popups: &HashSet<TabId>,
    before: &TabModelSnapshot,
    after: &TabModelSnapshot,
) -> Result<(), SessionError> {
    let open: HashSet<TabId> = after.tabs.iter().map(|tab| tab.id).collect();
    for tab in &before.tabs {
        if !open.contains(&tab.id) {
            catalogue
                .remove(Target::Tab(tab.id))
                .map_err(SessionError::Search)?;
        }
    }
    for tab in &after.tabs {
        upsert_tab(catalogue, popups, tab)?;
    }
    Ok(())
}

/// Upserts one open tab; a blank tab takes the core's Home title, as in the sidebar. Other
/// titles stay as reported, since the palette falls back to the address itself.
fn upsert_tab(
    catalogue: &mut Catalogue,
    popups: &HashSet<TabId>,
    tab: &TabSnapshot,
) -> Result<(), SessionError> {
    let origin = PageOrigin::from_adopted_popup(popups.contains(&tab.id));
    let mut row = entry(Target::Tab(tab.id), &tab.metadata, true);
    if let Some(home) = tab.metadata.home_title(origin) {
        row.title = home.to_owned();
    }
    catalogue.upsert(row).map_err(SessionError::Search)
}

fn open_saved_pages(model: &TabModelSnapshot) -> HashSet<SavedPageId> {
    model
        .tabs
        .iter()
        .filter_map(|tab| tab.saved_page_id)
        .collect()
}

fn upsert_saved<'a>(
    catalogue: &mut Catalogue,
    pages: impl Iterator<Item = &'a SavedPageSnapshot>,
    open: &HashSet<SavedPageId>,
) -> Result<(), SessionError> {
    for page in pages {
        catalogue
            .upsert(entry(
                Target::Saved(page.id),
                &page.metadata,
                open.contains(&page.id),
            ))
            .map_err(SessionError::Search)?;
    }
    Ok(())
}

fn entry(target: Target, metadata: &TabMetadata, open: bool) -> Entry {
    Entry {
        target,
        title: metadata.title.clone(),
        address: metadata.address.clone(),
        usage: Usage {
            visits: 0,
            last_used: None,
        },
        open,
    }
}

/// Storage failures stop the session as saves do; an unindexable row or clock does not.
fn route_history_result(
    result: Result<(), SessionError>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    match result {
        Err(SessionError::Store(error)) => Err(SessionError::Store(error)),
        Err(error) => emit_recoverable(events, error),
        Ok(()) => Ok(()),
    }
}

/// Answers a page's permission request with the remembered choice, if any.
fn emit_permission(
    events: &UnboundedSender<SessionEvent>,
    id: u64,
    request: u64,
    result: Result<Option<bool>, StoreError>,
) -> Result<(), SessionError> {
    emit(
        events,
        SessionEvent::Permission {
            id,
            request,
            result,
        },
    )
}

/// Reports a failure that leaves the session running.
fn emit_recoverable(
    events: &UnboundedSender<SessionEvent>,
    error: SessionError,
) -> Result<(), SessionError> {
    emit(
        events,
        SessionEvent::Failure {
            error,
            fatal: false,
        },
    )
}

fn emit(events: &UnboundedSender<SessionEvent>, event: SessionEvent) -> Result<(), SessionError> {
    events
        .unbounded_send(event)
        .map_err(|_| SessionError::EventReceiverClosed)
}

pub(crate) fn utc_seconds() -> Result<i64, SessionError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SessionError::ClockUnavailable)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| SessionError::ClockUnavailable)
}

#[cfg(test)]
#[path = "session/worker_tests.rs"]
mod worker_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_metadata_callbacks_preserve_both_fields() -> Result<(), SessionError> {
        let mut model = TabModel::new();
        apply_model(
            &mut model,
            SessionCommand::NewTab {
                address: "about:blank".to_owned(),
            },
        )?;
        let id = model.tabs()[0].id();
        apply_model(
            &mut model,
            SessionCommand::ObserveMetadata {
                id,
                title: None,
                address: Some("https://navigation.example.test/".to_owned()),
            },
        )?;
        apply_model(
            &mut model,
            SessionCommand::ObserveMetadata {
                id,
                title: Some("Synthetic title".to_owned()),
                address: None,
            },
        )?;
        assert_eq!(
            model.tabs()[0].metadata().address,
            "https://navigation.example.test/"
        );
        assert_eq!(model.tabs()[0].metadata().title, "Synthetic title");
        Ok(())
    }

    #[test]
    fn saved_rows_keep_their_destination_after_navigation_and_close() -> Result<(), SessionError> {
        let mut model = TabModel::new();
        apply_model(
            &mut model,
            SessionCommand::NewTab {
                address: "https://start.example.test".to_owned(),
            },
        )?;
        apply_model(&mut model, SessionCommand::SaveActive)?;
        let saved = model.saved_pages()[0].id();
        let live = model.tabs()[0].id();
        apply_model(
            &mut model,
            SessionCommand::UpdateMetadata {
                id: live,
                metadata: TabMetadata {
                    address: "https://later.example.test".to_owned(),
                    ..TabMetadata::default()
                },
            },
        )?;
        apply_model(&mut model, SessionCommand::CloseTab(live))?;
        assert_eq!(model.active_id(), None);
        assert_eq!(
            model.saved_pages()[0].metadata().address,
            "https://start.example.test"
        );
        apply_model(&mut model, SessionCommand::OpenSaved(saved))?;
        assert_ne!(model.tabs()[0].id(), live);
        assert_eq!(
            model.tabs()[0].metadata().address,
            "https://start.example.test"
        );
        Ok(())
    }

    #[test]
    fn catalogue_updates_remove_closed_instances_but_keep_unloaded_saved_rows()
    -> Result<(), SessionError> {
        let mut model = TabModel::new();
        let mut catalogue = Catalogue::new();
        apply_model(
            &mut model,
            SessionCommand::NewTab {
                address: "https://catalogue.example.test".to_owned(),
            },
        )?;
        apply_model(&mut model, SessionCommand::SaveActive)?;
        let before = model.snapshot();
        sync_catalogue(
            &mut catalogue,
            &HashSet::new(),
            &TabModelSnapshot::default(),
            &before,
        )?;
        let tab = model.tabs()[0].id();
        let saved = model.saved_pages()[0].id();
        apply_model(&mut model, SessionCommand::CloseTab(tab))?;
        sync_catalogue(&mut catalogue, &HashSet::new(), &before, &model.snapshot())?;
        assert!(catalogue.get(Target::Tab(tab)).is_none());
        assert!(
            !catalogue
                .get(Target::Saved(saved))
                .expect("saved synthetic row")
                .open
        );
        let generation = catalogue.generation();
        sync_catalogue(
            &mut catalogue,
            &HashSet::new(),
            &model.snapshot(),
            &model.snapshot(),
        )?;
        assert_eq!(catalogue.generation(), generation);
        Ok(())
    }

    #[test]
    fn a_blank_tab_is_titled_home_in_the_palette_catalogue() -> Result<(), SessionError> {
        let mut model = TabModel::new();
        let mut catalogue = Catalogue::new();
        apply_model(
            &mut model,
            SessionCommand::NewTab {
                address: "about:blank".to_owned(),
            },
        )?;
        let tab = model.tabs()[0].id();
        sync_catalogue(
            &mut catalogue,
            &HashSet::new(),
            &TabModelSnapshot::default(),
            &model.snapshot(),
        )?;
        let row = catalogue.get(Target::Tab(tab)).expect("synthetic tab row");
        assert_eq!(row.title, "Home");
        Ok(())
    }

    #[test]
    fn an_untitled_page_keeps_an_empty_palette_title() -> Result<(), SessionError> {
        let mut model = TabModel::new();
        let mut catalogue = Catalogue::new();
        let id = model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: "https://untitled.example.test/".to_owned(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .map_err(SessionError::Model)?;
        sync_catalogue(
            &mut catalogue,
            &HashSet::new(),
            &TabModelSnapshot::default(),
            &model.snapshot(),
        )?;
        let row = catalogue.get(Target::Tab(id)).expect("synthetic tab row");
        assert_eq!(row.title, "");
        Ok(())
    }

    #[test]
    fn a_blank_adopted_popup_keeps_its_address_in_the_palette_catalogue() -> Result<(), SessionError>
    {
        let mut model = TabModel::new();
        let mut catalogue = Catalogue::new();
        apply_model(
            &mut model,
            SessionCommand::NewTab {
                address: "about:blank".to_owned(),
            },
        )?;
        let tab = model.tabs()[0].id();
        let popups = HashSet::from([tab]);
        sync_catalogue(
            &mut catalogue,
            &popups,
            &TabModelSnapshot::default(),
            &model.snapshot(),
        )?;
        let row = catalogue
            .get(Target::Tab(tab))
            .expect("synthetic popup row");
        assert_eq!(row.title, "about:blank");
        Ok(())
    }

    #[test]
    fn empty_reopen_and_invalid_commands_preserve_the_model() -> Result<(), SessionError> {
        let mut model = TabModel::new();
        apply_model(&mut model, SessionCommand::ReopenClosed)?;
        let before = model.snapshot();
        assert_eq!(
            apply_model(&mut model, SessionCommand::SaveActive),
            Err(SessionError::NoActiveTab)
        );
        let id = TabId::new(99).map_err(SessionError::Model)?;
        assert_eq!(
            apply_model(&mut model, SessionCommand::FocusTab(id)),
            Err(SessionError::Model(TabError::UnknownTab(id)))
        );
        assert_eq!(model.snapshot(), before);
        Ok(())
    }

    #[test]
    fn observed_close_ignores_absent_and_plugin_ids_and_remembers_each_web_instance_once()
    -> Result<(), SessionError> {
        let mut model = TabModel::new();
        let plugin = model
            .open(
                TabKind::PluginView,
                TabMetadata::default(),
                Activation::Foreground,
            )
            .map_err(SessionError::Model)?;
        let before = model.snapshot();
        for id in [plugin, TabId::new(99).map_err(SessionError::Model)?] {
            apply_model(&mut model, SessionCommand::ObserveClosed(id))?;
            assert_eq!(model.snapshot(), before);
            assert_eq!(model.closed_count(), 0);
        }

        apply_model(
            &mut model,
            SessionCommand::NewTab {
                address: "https://close-once.example.test/".to_owned(),
            },
        )?;
        let web = model.active_id().expect("synthetic web tab is active");
        apply_model(&mut model, SessionCommand::SaveActive)?;
        let saved = model.saved_pages()[0].clone();
        apply_model(&mut model, SessionCommand::ObserveClosed(web))?;
        apply_model(&mut model, SessionCommand::ObserveClosed(web))?;
        assert_eq!(model.closed_count(), 1);
        assert_eq!(model.saved_pages(), &[saved]);
        assert_eq!(model.active_id(), Some(plugin));
        Ok(())
    }

    #[test]
    fn command_and_snapshot_diagnostics_redact_user_content() -> Result<(), SessionError> {
        let address = "https://redacted-session-sentinel.example.test";
        let command = SessionCommand::NewTab {
            address: address.to_owned(),
        };
        assert!(!format!("{command:?}").contains(address));
        let mut model = TabModel::new();
        apply_model(&mut model, command)?;
        let event = SessionEvent::State(SessionSnapshot {
            revision: 1,
            model: model.snapshot(),
            created: Vec::new(),
            navigate: None,
            home_entry: None,
            popup: None,
            previous_shutdown: ShutdownState::Clean,
        });
        assert!(!format!("{event:?}").contains(address));
        Ok(())
    }
}

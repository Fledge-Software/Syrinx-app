//! Keeps model, persistence and catalogue work off the browser's event thread.

mod recovery;

pub use recovery::RecoveryPrompt;

use std::{
    collections::HashSet,
    fmt,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use atlas_core::{
    Activation, SavedPageId, SnapshotError, TabError, TabId, TabKind, TabMetadata, TabModel,
    TabModelSnapshot,
};
use atlas_platform::{StartupStorage, WindowsDirectoryLease};
use atlas_search::{
    Catalogue, Entry, Scope, SearchError, SearchPolicy, SearchResults, Target, Usage,
};
use atlas_store::{LocationError, ShutdownState, Store, StoreError};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

const IDLE_DELAY: Duration = Duration::from_secs(5);
const BACKUP_CHECK_INTERVAL: Duration = Duration::from_secs(60);

pub enum HomeEntryAction {
    NewTab,
    ReopenClosed,
    OpenSaved(SavedPageId),
}

pub enum SessionCommand {
    ConfirmRecovery {
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
    Query {
        request_id: u64,
        text: String,
        scope: Scope,
        policy: SearchPolicy,
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
    RecoveryOffered(RecoveryPrompt),
    PopupRejected {
        request: u64,
    },
    State(SessionSnapshot),
    Search {
        request_id: u64,
        result: Result<SearchResults, SessionError>,
    },
    Failure {
        error: SessionError,
        fatal: bool,
    },
    /// Connections have closed before this event; joining the worker is now safe.
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionError {
    Location(LocationError),
    Store(StoreError),
    Model(TabError),
    Snapshot(SnapshotError),
    Search(SearchError),
    NoActiveTab,
    ClockUnavailable,
    WorkerStart {
        code: Option<i32>,
    },
    WorkerPanicked,
    EventReceiverClosed,
    RecoveryUnavailable,
    Recovery(String),
    RecoveryFailed {
        startup: Box<SessionError>,
        recovery: String,
    },
    CleanupFailed {
        primary: Box<SessionError>,
        cleanup: StoreError,
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
            Self::NoActiveTab => formatter.write_str("There is no active page to save"),
            Self::ClockUnavailable => formatter.write_str("The system clock is unavailable"),
            Self::WorkerStart { code } => {
                write!(formatter, "Cannot start the session worker ({code:?})")
            }
            Self::WorkerPanicked => formatter.write_str("The session worker stopped unexpectedly"),
            Self::EventReceiverClosed => formatter.write_str("The session event receiver closed"),
            Self::RecoveryUnavailable => {
                formatter.write_str("This recovery decision is no longer available")
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
pub fn spawn(
    data_dir: PathBuf,
) -> Result<(SessionHandle, UnboundedReceiver<SessionEvent>), SessionError> {
    let (commands, receiver) = mpsc::channel();
    let (events, event_receiver) = unbounded();
    let worker = thread::Builder::new()
        .name("atlas-session".to_owned())
        .spawn(move || finish_worker(run(data_dir, receiver, &events), &events))
        .map_err(|error| SessionError::WorkerStart {
            code: error.raw_os_error(),
        })?;
    Ok((SessionHandle { commands, worker }, event_receiver))
}

fn finish_worker(
    result: Result<(), SessionError>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    // A failed receiver is still visible to the caller through the worker's join result.
    if let Err(error) = &result
        && emit(
            events,
            SessionEvent::Failure {
                error: error.clone(),
                fatal: true,
            },
        )
        .is_err()
    {
        return result;
    }
    let delivered = emit(events, SessionEvent::Stopped);
    result.and(delivered)
}

fn run(
    data_dir: PathBuf,
    commands: mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    let Some((store, mut worker)) = startup_session(&data_dir, &commands, events)? else {
        return Ok(());
    };
    let result = worker.serve(&store, commands, events);
    finish_store(result, store.shutdown())
}

fn startup_session(
    data_dir: &std::path::Path,
    commands: &mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<Option<(Store, Worker)>, SessionError> {
    // The guarded preflight retains either the existing primary or the exact recovery offer.
    let confirmed = match recovery::prepare_startup(data_dir)? {
        StartupStorage::Ready(lease) => match open_session(lease) {
            Ok(session) => return Ok(Some(session)),
            Err(error) => recovery::confirm_failed_start(data_dir, error, commands, events)?,
        },
        StartupStorage::Recovery(offer) => recovery::confirm_offer(offer, commands, events)?,
    };
    if !confirmed {
        return Ok(None);
    }
    // A confirmed restore gets one guarded reopen, never an automatic second offer.
    let StartupStorage::Ready(lease) = recovery::prepare_startup(data_dir)? else {
        return Err(SessionError::RecoveryUnavailable);
    };
    open_session(lease).map(Some)
}

fn open_session(lease: WindowsDirectoryLease) -> Result<(Store, Worker), SessionError> {
    let store = Store::open(Arc::new(lease)).map_err(SessionError::Store)?;
    match Worker::restore(&store) {
        Ok(worker) => Ok((store, worker)),
        Err(error) => {
            // Recovery cannot overlap live connections or conceal a failed close.
            finish_store(Err(error), store.shutdown())
        }
    }
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
    revision: u64,
    previous_shutdown: ShutdownState,
}

impl Worker {
    fn restore(store: &Store) -> Result<Self, SessionError> {
        let stored = store.load().map_err(SessionError::Store)?;
        let model = TabModel::from_snapshot(stored.snapshot).map_err(SessionError::Snapshot)?;
        let mut catalogue = Catalogue::new();
        sync_catalogue(
            &mut catalogue,
            &TabModelSnapshot::default(),
            &model.snapshot(),
        )?;
        Ok(Self {
            model,
            catalogue,
            revision: stored.revision,
            previous_shutdown: store.previous_shutdown(),
        })
    }

    fn serve(
        &mut self,
        store: &Store,
        commands: mpsc::Receiver<SessionCommand>,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        self.publish(events, Vec::new(), None, None, None)?;
        let mut delay = IDLE_DELAY;
        loop {
            match commands.recv_timeout(delay) {
                Ok(SessionCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Ok(());
                }
                Ok(command) => {
                    self.handle(store, command, events)?;
                    delay = IDLE_DELAY;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // The store retains its daily due state; retries remain idle and bounded.
                    backup_while_idle(store, utc_seconds(), events)?;
                    delay = BACKUP_CHECK_INTERVAL;
                }
            }
        }
    }

    fn handle(
        &mut self,
        store: &Store,
        command: SessionCommand,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        if matches!(command, SessionCommand::ConfirmRecovery { .. }) {
            return recovery::reject_command(events);
        }
        if let SessionCommand::Query {
            request_id,
            text,
            scope,
            policy,
        } = command
        {
            let result = utc_seconds().and_then(|now| {
                self.catalogue
                    .query(&text, scope, now, policy)
                    .map_err(SessionError::Search)
            });
            // Failures carry the same identity as successful replies, so stale queries stay silent.
            return emit(events, SessionEvent::Search { request_id, result });
        }

        // Invalid or stale commands do not mutate the model and remain recoverable UI errors.
        let navigating = matches!(&command, SessionCommand::NavigateActive { .. });
        let home_entry = match &command {
            SessionCommand::HomeEntry { request_id, .. } => Some(*request_id),
            _ => None,
        };
        let popup_request = match &command {
            SessionCommand::AdoptPopup { request, .. } => Some(*request),
            _ => None,
        };
        let before = self.model.snapshot();
        if let Err(error) = apply_model(&mut self.model, command) {
            if let Some(request) = popup_request {
                emit(events, SessionEvent::PopupRejected { request })?;
            }
            return emit(
                events,
                SessionEvent::Failure {
                    error,
                    fatal: false,
                },
            );
        }
        let after = self.model.snapshot();
        if before != after {
            self.revision = store
                .save(self.revision, after.clone())
                .map_err(SessionError::Store)?;
            sync_catalogue(&mut self.catalogue, &before, &after)?;
        }
        let created: Vec<_> = after
            .tabs
            .iter()
            .filter(|tab| tab.id.get() > before.last_tab_id)
            .map(|tab| tab.id)
            .collect();
        let navigate = if navigating { after.active_id } else { None };
        let popup = popup_request.map(|request| (request, created.first().copied()));
        self.publish(events, created, navigate, home_entry, popup)
    }

    fn publish(
        &self,
        events: &UnboundedSender<SessionEvent>,
        created: Vec<TabId>,
        navigate: Option<TabId>,
        home_entry: Option<u64>,
        popup: Option<(u64, Option<TabId>)>,
    ) -> Result<(), SessionError> {
        emit(
            events,
            SessionEvent::State(SessionSnapshot {
                revision: self.revision,
                model: self.model.snapshot(),
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
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    if let Err(error) = now.and_then(|now| {
        store
            .backup_if_due(now, true)
            .map(|_| ())
            .map_err(SessionError::Store)
    }) {
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
        SessionCommand::ConfirmRecovery { .. } => return Err(SessionError::RecoveryUnavailable),
        SessionCommand::Query { .. } | SessionCommand::Shutdown => {}
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
    before: &TabModelSnapshot,
    after: &TabModelSnapshot,
) -> Result<(), SessionError> {
    // The feed contains only model-derived rows; it creates no browsing history or usage log.
    let targets: HashSet<_> = after
        .tabs
        .iter()
        .map(|tab| Target::Tab(tab.id))
        .chain(after.saved_pages.iter().map(|page| Target::Saved(page.id)))
        .collect();
    for target in before
        .tabs
        .iter()
        .map(|tab| Target::Tab(tab.id))
        .chain(before.saved_pages.iter().map(|page| Target::Saved(page.id)))
    {
        if !targets.contains(&target) {
            catalogue.remove(target).map_err(SessionError::Search)?;
        }
    }
    let open_saved: HashSet<_> = after
        .tabs
        .iter()
        .filter_map(|tab| tab.saved_page_id)
        .collect();
    for tab in &after.tabs {
        catalogue
            .upsert(entry(Target::Tab(tab.id), &tab.metadata, true))
            .map_err(SessionError::Search)?;
    }
    for page in &after.saved_pages {
        catalogue
            .upsert(entry(
                Target::Saved(page.id),
                &page.metadata,
                open_saved.contains(&page.id),
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

fn emit(events: &UnboundedSender<SessionEvent>, event: SessionEvent) -> Result<(), SessionError> {
    events
        .unbounded_send(event)
        .map_err(|_| SessionError::EventReceiverClosed)
}

fn utc_seconds() -> Result<i64, SessionError> {
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
        sync_catalogue(&mut catalogue, &TabModelSnapshot::default(), &before)?;
        let tab = model.tabs()[0].id();
        let saved = model.saved_pages()[0].id();
        apply_model(&mut model, SessionCommand::CloseTab(tab))?;
        sync_catalogue(&mut catalogue, &before, &model.snapshot())?;
        assert!(catalogue.get(Target::Tab(tab)).is_none());
        assert!(
            !catalogue
                .get(Target::Saved(saved))
                .expect("saved synthetic row")
                .open
        );
        let generation = catalogue.generation();
        sync_catalogue(&mut catalogue, &model.snapshot(), &model.snapshot())?;
        assert_eq!(catalogue.generation(), generation);
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

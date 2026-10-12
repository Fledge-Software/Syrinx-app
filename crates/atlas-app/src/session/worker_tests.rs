//! Exercises the session command boundary with disposable native storage and real restarts.

use std::{
    fs, io,
    num::NonZeroU64,
    path::{Path, PathBuf},
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll, Wake, Waker},
    time::Instant,
};

use atlas_search::{BlankQuery, BoostWeights};
use atlas_store::{BackupOutcome, DirectoryLease, StoredSnapshot, validate_backup};
use futures::Stream;

use super::*;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
const SAVED_ADDRESS: &str = "https://saved-session.example.test/original";
const LIVE_ADDRESS: &str = "https://live-session.example.test/latest";

struct Fixture(PathBuf, PathBuf);

impl Fixture {
    fn new() -> Self {
        // Exclusive creation proves test ownership before any files can be written or removed.
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("synthetic clock follows epoch")
            .as_nanos();
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "atlas-session-test-{}-{stamp}-{sequence}",
            std::process::id()
        ));
        checked_io(fs::create_dir(&path));
        Self(path.join("State"), path)
    }

    fn start(&self) -> (SessionHandle, UnboundedReceiver<SessionEvent>) {
        spawn(self.0.clone()).expect("start synthetic session worker")
    }

    fn cleanup(self) {
        assert_eq!(self.0.parent(), Some(self.1.as_path()));
        assert_eq!(self.0, self.1.join("State"));
        // Native capabilities remove only recognized backups; unknown entries prevent cleanup.
        let lease = WindowsDirectoryLease::prepare(&self.0).expect("guard owned fixture cleanup");
        let archive = lease.backup_archive().expect("native archive exists");
        for candidate in archive
            .candidates()
            .expect("read synthetic backup inventory")
        {
            candidate
                .file
                .remove()
                .expect("remove recognized synthetic backup");
        }
        drop(archive);
        drop(lease);
        for name in [
            "atlas.db",
            "atlas.db-wal",
            "atlas.db-shm",
            "atlas.db-journal",
            "atlas.lifecycle",
        ] {
            remove_if_present(&self.0.join(name), false);
        }
        remove_if_present(&self.0.join("backups"), true);
        checked_io(fs::remove_dir(self.0));
        checked_io(fs::remove_dir(self.1));
    }
}

fn checked_io<T>(result: io::Result<T>) -> T {
    result.unwrap_or_else(|error| {
        panic!(
            "synthetic file operation failed: {:?}, code {:?}",
            error.kind(),
            error.raw_os_error()
        )
    })
}

fn remove_if_present(path: &Path, directory: bool) {
    let result = if directory {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    match result {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => checked_io(Err(error)),
    }
}

struct ThreadWake(thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn next_event(events: &mut UnboundedReceiver<SessionEvent>) -> SessionEvent {
    // Wake directly on events, with a deadline so a broken worker cannot hang the gate.
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match Pin::new(&mut *events).poll_next(&mut context) {
            Poll::Ready(Some(event)) => return event,
            Poll::Ready(None) => panic!("synthetic worker closed its events unexpectedly"),
            Poll::Pending => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                assert!(
                    !remaining.is_zero(),
                    "synthetic worker event deadline expired"
                );
                thread::park_timeout(remaining);
            }
        }
    }
}

fn state(events: &mut UnboundedReceiver<SessionEvent>) -> SessionSnapshot {
    match next_event(events) {
        SessionEvent::State(state) => state,
        event => panic!("expected synthetic session state, got {event:?}"),
    }
}

fn send(handle: &SessionHandle, command: SessionCommand) {
    handle
        .commands
        .send(command)
        .expect("send synthetic session command");
}

fn change(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    command: SessionCommand,
) -> SessionSnapshot {
    send(handle, command);
    state(events)
}

fn stop(handle: SessionHandle, events: &mut UnboundedReceiver<SessionEvent>) {
    send(&handle, SessionCommand::Shutdown);
    assert!(matches!(next_event(events), SessionEvent::Stopped));
    handle
        .join()
        .expect("join clean synthetic session shutdown");
}

fn query_policy() -> SearchPolicy {
    SearchPolicy {
        limit: 20,
        max_typos: 0,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 120,
            frequency: 0,
            recency: 0,
            recency_window_secs: NonZeroU64::new(1).expect("nonzero synthetic window"),
        },
    }
}

#[test]
fn worker_restart_preserves_saved_destination_and_latest_live_navigation() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    let initial = state(&mut events);
    assert_eq!(initial.revision, 0);
    assert!(initial.model.tabs.is_empty());
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: SAVED_ADDRESS.to_owned(),
        },
    );
    let original = opened.model.tabs[0].id;
    assert_eq!(opened.created, vec![original]);
    let saved = change(&handle, &mut events, SessionCommand::SaveActive);
    let saved_id = saved.model.saved_pages[0].id;
    change(
        &handle,
        &mut events,
        SessionCommand::ObserveMetadata {
            id: original,
            title: Some("Synthetic navigated title".to_owned()),
            address: Some(LIVE_ADDRESS.to_owned()),
        },
    );
    let closed = change(&handle, &mut events, SessionCommand::CloseTab(original));
    assert!(closed.model.tabs.is_empty());
    assert_eq!(closed.model.active_id, None);
    assert_eq!(closed.model.saved_pages[0].metadata.address, SAVED_ADDRESS);

    // Closed saved instances reopen their fixed destination with a fresh live identity.
    let reopened = change(&handle, &mut events, SessionCommand::ReopenClosed);
    let live = reopened.model.tabs[0].id;
    assert_ne!(live, original);
    assert_eq!(reopened.created, vec![live]);
    assert_eq!(reopened.model.tabs[0].metadata.address, SAVED_ADDRESS);
    let latest = change(
        &handle,
        &mut events,
        SessionCommand::ObserveMetadata {
            id: live,
            title: Some("Synthetic latest title".to_owned()),
            address: Some(LIVE_ADDRESS.to_owned()),
        },
    );
    assert_eq!(latest.model.saved_pages[0].metadata.address, SAVED_ADDRESS);
    assert_eq!(latest.model.saved_pages[0].metadata.title, SAVED_ADDRESS);
    assert_eq!(latest.model.tabs[0].saved_page_id, Some(saved_id));
    stop(handle, &mut events);

    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.previous_shutdown, ShutdownState::Clean);
    assert_eq!(restored.revision, latest.revision);
    assert_eq!(restored.model, latest.model);
    assert!(restored.created.is_empty());
    assert_eq!(restored.model.tabs[0].metadata.address, LIVE_ADDRESS);

    // Restart reconstructs search from durable state without changing the fixed saved row.
    send(
        &handle,
        SessionCommand::Query {
            request_id: 17,
            text: "saved-session".to_owned(),
            scope: Scope::Saved,
            policy: query_policy(),
        },
    );
    let SessionEvent::Search { request_id, result } = next_event(&mut events) else {
        panic!("expected synthetic saved search result")
    };
    assert_eq!(request_id, 17);
    let results = result.expect("synthetic saved search succeeds");
    assert_eq!(results.counts.saved, 1);
    assert_eq!(results.hits.len(), 1);
    assert_eq!(results.hits[0].target, Target::Saved(saved_id));
    assert_eq!(results.hits[0].address, SAVED_ADDRESS);
    change(&handle, &mut events, SessionCommand::CloseTab(live));
    let opened = change(&handle, &mut events, SessionCommand::OpenSaved(saved_id));
    assert!(opened.model.tabs[0].id.get() > live.get());
    assert_eq!(opened.model.tabs[0].metadata.address, SAVED_ADDRESS);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn failed_query_keeps_its_identity_and_does_not_stop_later_queries_or_mutate_storage() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let before = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: SAVED_ADDRESS.to_owned(),
        },
    );
    let id = before.model.active_id.expect("synthetic page is active");

    // Both requests are queued before either reply can change a palette's pending identity.
    for (request_id, text) in [
        (41, "x".repeat(usize::from(u16::MAX) + 1)),
        (42, "saved-session".to_owned()),
    ] {
        send(
            &handle,
            SessionCommand::Query {
                request_id,
                text,
                scope: Scope::Everything,
                policy: query_policy(),
            },
        );
    }
    assert!(matches!(
        next_event(&mut events),
        SessionEvent::Search {
            request_id: 41,
            result: Err(SessionError::Search(SearchError::QueryTooLong))
        }
    ));
    let SessionEvent::Search {
        request_id: 42,
        result: Ok(results),
    } = next_event(&mut events)
    else {
        panic!("expected identified successful query after recoverable query failure")
    };
    assert_eq!(results.counts.tabs, 1);
    assert_eq!(results.hits.len(), 1);
    assert_eq!(results.hits[0].target, Target::Tab(id));

    let after = change(&handle, &mut events, SessionCommand::FocusTab(id));
    assert_eq!(after.model, before.model);
    assert_eq!(after.revision, before.revision);
    stop(handle, &mut events);
    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model, before.model);
    assert_eq!(restored.revision, before.revision);
    assert_eq!(restored.previous_shutdown, ShutdownState::Clean);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn worker_shutdown_drains_queued_changes_and_ignores_stale_page_callbacks() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: SAVED_ADDRESS.to_owned(),
        },
    );
    let id = opened.model.tabs[0].id;

    // Queue independent page callbacks and shutdown together, without awaiting intermediate saves.
    send(
        &handle,
        SessionCommand::ObserveMetadata {
            id,
            title: None,
            address: Some(LIVE_ADDRESS.to_owned()),
        },
    );
    send(
        &handle,
        SessionCommand::ObserveMetadata {
            id,
            title: Some("Synthetic queued title".to_owned()),
            address: None,
        },
    );
    send(&handle, SessionCommand::SaveActive);
    send(&handle, SessionCommand::CloseTab(id));
    send(
        &handle,
        SessionCommand::ObserveMetadata {
            id,
            title: Some("Synthetic stale title".to_owned()),
            address: Some("https://stale.example.test/".to_owned()),
        },
    );
    send(&handle, SessionCommand::Shutdown);
    for revision in 2..=5 {
        assert_eq!(state(&mut events).revision, revision);
    }
    let latest = state(&mut events);
    assert_eq!(latest.revision, 5);
    assert!(latest.model.tabs.is_empty());
    assert_eq!(latest.model.saved_pages[0].metadata.address, LIVE_ADDRESS);
    assert_eq!(
        latest.model.saved_pages[0].metadata.title,
        "Synthetic queued title"
    );
    assert!(matches!(next_event(&mut events), SessionEvent::Stopped));
    handle.join().expect("join drained synthetic worker");

    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model, latest.model);
    assert_eq!(restored.revision, latest.revision);
    assert_eq!(restored.previous_shutdown, ShutdownState::Clean);
    let unchanged = change(&handle, &mut events, SessionCommand::ReopenClosed);
    assert_eq!(unchanged.model, restored.model);
    assert_eq!(unchanged.revision, restored.revision);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn queued_relative_shortcuts_advance_and_close_the_authoritative_active_tab() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let mut ids = Vec::new();
    for index in 0..3 {
        let opened = change(
            &handle,
            &mut events,
            SessionCommand::NewTab {
                address: format!("https://relative-{index}.example.test/"),
            },
        );
        ids.push(opened.model.active_id.expect("new synthetic tab is active"));
    }
    change(&handle, &mut events, SessionCommand::FocusTab(ids[0]));

    // Both commands are submitted before either acknowledgement can update a UI snapshot.
    for _ in 0..2 {
        send(&handle, SessionCommand::CycleActive { backwards: false });
    }
    assert_eq!(state(&mut events).model.active_id, Some(ids[1]));
    assert_eq!(state(&mut events).model.active_id, Some(ids[2]));
    for _ in 0..2 {
        send(&handle, SessionCommand::CycleActive { backwards: true });
    }
    assert_eq!(state(&mut events).model.active_id, Some(ids[1]));
    assert_eq!(state(&mut events).model.active_id, Some(ids[0]));

    let wrapped = change(
        &handle,
        &mut events,
        SessionCommand::CycleActive { backwards: true },
    );
    assert_eq!(wrapped.model.active_id, Some(ids[2]));
    let wrapped = change(
        &handle,
        &mut events,
        SessionCommand::CycleActive { backwards: false },
    );
    assert_eq!(wrapped.model.active_id, Some(ids[0]));
    send(&handle, SessionCommand::CloseActive);
    send(&handle, SessionCommand::CloseActive);
    let first_close = state(&mut events);
    let second_close = state(&mut events);
    assert_eq!(first_close.model.active_id, Some(ids[1]));
    assert_eq!(second_close.model.active_id, Some(ids[2]));
    assert_eq!(second_close.model.tabs.len(), 1);
    assert_eq!(second_close.model.tabs[0].id, ids[2]);

    // Closing the last page leaves Home; additional relative keys remain harmless no-ops.
    let home = change(&handle, &mut events, SessionCommand::CloseActive);
    assert!(home.model.tabs.is_empty());
    assert_eq!(home.model.active_id, None);
    send(&handle, SessionCommand::CycleActive { backwards: false });
    send(&handle, SessionCommand::CycleActive { backwards: true });
    send(&handle, SessionCommand::CloseActive);
    send(&handle, SessionCommand::Shutdown);
    for _ in 0..3 {
        let unchanged = state(&mut events);
        assert_eq!(unchanged.model, home.model);
        assert_eq!(unchanged.revision, home.revision);
    }
    assert!(matches!(next_event(&mut events), SessionEvent::Stopped));
    handle.join().expect("join synthetic shortcut session");
    fixture.cleanup();
}

#[test]
fn queued_navigation_uses_worker_selection_and_preserves_saved_destination() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let first = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: "https://first-navigation.example.test/".into(),
        },
    )
    .model
    .active_id
    .expect("first synthetic tab");
    let second = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: SAVED_ADDRESS.into(),
        },
    )
    .model
    .active_id
    .expect("second synthetic tab");
    let saved = change(&handle, &mut events, SessionCommand::SaveActive);

    // Both commands precede reading any acknowledgement, as with a busy session worker.
    send(&handle, SessionCommand::FocusTab(first));
    send(
        &handle,
        SessionCommand::NavigateActive {
            address: LIVE_ADDRESS.into(),
        },
    );
    assert_eq!(state(&mut events).navigate, None);
    let navigated = state(&mut events);
    assert_eq!(navigated.navigate, Some(first));
    assert_eq!(
        navigated
            .model
            .tabs
            .iter()
            .find(|tab| tab.id == first)
            .expect("first tab")
            .metadata
            .address,
        LIVE_ADDRESS
    );
    assert_eq!(
        navigated
            .model
            .tabs
            .iter()
            .find(|tab| tab.id == second)
            .expect("second tab")
            .metadata
            .address,
        SAVED_ADDRESS
    );

    // Relative selection followed by navigation targets the newly active saved instance.
    send(&handle, SessionCommand::CycleActive { backwards: false });
    send(
        &handle,
        SessionCommand::NavigateActive {
            address: LIVE_ADDRESS.into(),
        },
    );
    assert_eq!(state(&mut events).navigate, None);
    let navigated = state(&mut events);
    assert_eq!(navigated.navigate, Some(second));
    assert_eq!(navigated.model.saved_pages, saved.model.saved_pages);

    // A close preceding navigation selects its surviving neighbour on the worker.
    send(&handle, SessionCommand::CloseActive);
    send(
        &handle,
        SessionCommand::NavigateActive {
            address: "https://after-close.example.test/".into(),
        },
    );
    assert_eq!(state(&mut events).navigate, None);
    let navigated = state(&mut events);
    assert_eq!(navigated.navigate, Some(first));
    assert_eq!(navigated.model.tabs.len(), 1);
    assert_eq!(
        navigated.model.tabs[0].metadata.address,
        "https://after-close.example.test/"
    );
    assert_eq!(navigated.model.saved_pages, saved.model.saved_pages);
    stop(handle, &mut events);

    // Only committed state survives restart; navigation acknowledgements are never replayed.
    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model, navigated.model);
    assert_eq!(restored.navigate, None);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn queued_new_tab_and_home_navigation_acknowledge_the_created_instance() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let home_navigation = change(
        &handle,
        &mut events,
        SessionCommand::NavigateActive {
            address: LIVE_ADDRESS.into(),
        },
    );
    let first = home_navigation
        .model
        .active_id
        .expect("navigation opens Home's tab");
    assert_eq!(home_navigation.navigate, Some(first));
    assert_eq!(home_navigation.created, [first]);

    // An explicit reload of the same destination still needs a native navigation acknowledgement.
    let repeated = change(
        &handle,
        &mut events,
        SessionCommand::NavigateActive {
            address: LIVE_ADDRESS.into(),
        },
    );
    let repeated_again = change(
        &handle,
        &mut events,
        SessionCommand::NavigateActive {
            address: LIVE_ADDRESS.into(),
        },
    );
    assert_eq!(repeated_again.revision, repeated.revision);
    assert_eq!(repeated_again.navigate, Some(first));

    send(
        &handle,
        SessionCommand::NewTab {
            address: "about:blank".into(),
        },
    );
    send(
        &handle,
        SessionCommand::NavigateActive {
            address: SAVED_ADDRESS.into(),
        },
    );
    let created = state(&mut events);
    let second = created.model.active_id.expect("new synthetic tab");
    assert_ne!(first, second);
    assert_eq!(created.navigate, None);
    let navigated = state(&mut events);
    assert_eq!(navigated.navigate, Some(second));
    assert_eq!(
        navigated
            .model
            .tabs
            .iter()
            .find(|tab| tab.id == first)
            .expect("first remains")
            .metadata
            .address,
        LIVE_ADDRESS
    );
    assert_eq!(
        navigated
            .model
            .tabs
            .iter()
            .find(|tab| tab.id == second)
            .expect("second remains")
            .metadata
            .address,
        SAVED_ADDRESS
    );
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn worker_idle_path_publishes_the_acknowledged_snapshot_without_waiting_for_a_timer() {
    let fixture = Fixture::new();
    let lease =
        Arc::new(WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic store"));
    let archive = lease.backup_archive().expect("native backup archive");
    let store = Store::open(lease.clone()).expect("open guarded synthetic store");
    let mut worker = Worker::restore(&store).expect("restore synthetic worker");
    let (sender, mut events) = unbounded();
    worker
        .handle(
            &store,
            SessionCommand::NewTab {
                address: SAVED_ADDRESS.to_owned(),
            },
            &sender,
        )
        .expect("commit synthetic tab through worker");
    state(&mut events);
    worker
        .handle(&store, SessionCommand::SaveActive, &sender)
        .expect("commit synthetic saved row through worker");
    let acknowledged = state(&mut events);

    // Invoke the same private branch as the idle timer with a fixed synthetic UTC time.
    backup_while_idle(&store, Ok(1_800_000_000), &sender).expect("run idle backup branch");
    let first = archive
        .candidates()
        .expect("enumerate published synthetic backups");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].metadata.revision, acknowledged.revision);
    let verified =
        validate_backup(first[0].file.as_ref()).expect("validate published synthetic snapshot");
    assert_eq!(verified.revision, acknowledged.revision);
    assert_eq!(verified.snapshot, acknowledged.model);
    let first_id = first[0].metadata.id;
    drop(first);
    backup_while_idle(&store, Ok(1_800_000_060), &sender)
        .expect("check daily schedule on next idle poll");
    let repeated = archive
        .candidates()
        .expect("enumerate unchanged synthetic archive");
    assert_eq!(repeated.len(), 1);
    assert_eq!(repeated[0].metadata.id, first_id);
    drop(repeated);

    backup_while_idle(&store, Err(SessionError::ClockUnavailable), &sender)
        .expect("surface idle clock failure");
    assert!(matches!(
        next_event(&mut events),
        SessionEvent::Failure {
            error: SessionError::ClockUnavailable,
            fatal: false
        }
    ));
    store.shutdown().expect("close synthetic backup store");
    drop(archive);
    drop(lease);
    fixture.cleanup();
}

#[test]
fn queued_page_closes_preserve_saved_rows_and_ignore_replaced_instances_across_restart() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let original = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: SAVED_ADDRESS.into(),
        },
    )
    .model
    .active_id
    .expect("synthetic page is active");
    let saved = change(&handle, &mut events, SessionCommand::SaveActive);
    let navigated = change(
        &handle,
        &mut events,
        SessionCommand::ObserveMetadata {
            id: original,
            title: Some("Synthetic page after navigation".into()),
            address: Some(LIVE_ADDRESS.into()),
        },
    );

    // The worker processes duplicate callbacks and a late old-ID callback in queue order.
    send(&handle, SessionCommand::ObserveClosed(original));
    send(&handle, SessionCommand::ObserveClosed(original));
    send(&handle, SessionCommand::ReopenClosed);
    send(&handle, SessionCommand::ObserveClosed(original));
    let closed = state(&mut events);
    assert_eq!(closed.revision, navigated.revision + 1);
    assert!(closed.model.tabs.is_empty());
    let duplicate = state(&mut events);
    assert_eq!(duplicate.model, closed.model);
    assert_eq!(duplicate.revision, closed.revision);
    let reopened = state(&mut events);
    let replacement = reopened.model.active_id.expect("saved page reopened");
    assert_ne!(replacement, original);
    assert_eq!(reopened.model.tabs[0].metadata.address, SAVED_ADDRESS);
    let stale = state(&mut events);
    assert_eq!(stale.model, reopened.model);
    assert_eq!(stale.revision, reopened.revision);

    // A manual close followed by the native observation enters closed history only once.
    send(&handle, SessionCommand::CloseTab(replacement));
    send(&handle, SessionCommand::ObserveClosed(replacement));
    send(&handle, SessionCommand::ReopenClosed);
    send(&handle, SessionCommand::ReopenClosed);
    let manual = state(&mut events);
    let observed = state(&mut events);
    assert_eq!(observed.model, manual.model);
    assert_eq!(observed.revision, manual.revision);
    let reopened = state(&mut events);
    let repeated_reopen = state(&mut events);
    assert_eq!(repeated_reopen.model, reopened.model);
    assert_eq!(repeated_reopen.revision, reopened.revision);
    let saved_live = reopened.model.active_id.expect("saved page reopened once");
    let ordinary = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: "https://ordinary-close.example.test/".into(),
        },
    )
    .model
    .active_id
    .expect("ordinary page is active");
    send(&handle, SessionCommand::ObserveClosed(saved_live));
    send(&handle, SessionCommand::ObserveClosed(ordinary));
    send(&handle, SessionCommand::Shutdown);
    assert_eq!(state(&mut events).model.active_id, Some(ordinary));
    let home = state(&mut events);
    assert!(home.model.tabs.is_empty());
    assert_eq!(home.model.active_id, None);
    assert_eq!(home.model.saved_pages, saved.model.saved_pages);
    assert!(matches!(next_event(&mut events), SessionEvent::Stopped));
    handle.join().expect("join observed-close session");

    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model, home.model);
    assert_eq!(restored.revision, home.revision);
    assert_eq!(restored.previous_shutdown, ShutdownState::Clean);
    assert!(restored.created.is_empty());
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn failed_close_save_reports_fatal_failure_without_acknowledging_native_cleanup() {
    let fixture = Fixture::new();
    let lease = Arc::new(
        WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic failure store"),
    );
    let store = Store::open(lease).expect("open synthetic failure store");
    let mut worker = Worker::restore(&store).expect("restore synthetic worker");
    let (sender, mut events) = unbounded();
    worker
        .handle(
            &store,
            SessionCommand::NewTab {
                address: SAVED_ADDRESS.into(),
            },
            &sender,
        )
        .expect("commit synthetic page");
    let acknowledged = state(&mut events);
    let id = acknowledged
        .model
        .active_id
        .expect("synthetic page is active");

    // Advance storage independently to force a real transactional rejection of the close.
    let revision = store
        .save(acknowledged.revision, acknowledged.model.clone())
        .expect("advance synthetic store revision");
    let expected = SessionError::Store(StoreError::StaleRevision {
        expected: acknowledged.revision,
        actual: revision,
    });
    let result = worker.handle(&store, SessionCommand::ObserveClosed(id), &sender);
    assert_eq!(result, Err(expected.clone()));
    assert_eq!(finish_worker(result, &sender), Err(expected.clone()));
    assert!(matches!(
        next_event(&mut events),
        SessionEvent::Failure { error, fatal: true } if error == expected
    ));
    assert!(matches!(next_event(&mut events), SessionEvent::Stopped));
    // No State preceded those events, so the UI still owns its previously acknowledged page.
    let stored = store.load().expect("read rejected-close persistence");
    assert_eq!(stored.snapshot, acknowledged.model);
    assert_eq!(stored.revision, revision);
    store.shutdown().expect("close synthetic failure store");
    fixture.cleanup();
}

#[test]
fn home_entry_acknowledgments_follow_their_command_and_do_not_survive_restart() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    assert_eq!(state(&mut events).home_entry, None);
    send(
        &handle,
        SessionCommand::HomeEntry {
            request_id: 1,
            action: HomeEntryAction::NewTab,
        },
    );
    let home = state(&mut events);
    let original = home.model.active_id.expect("synthetic Home is active");
    assert_eq!(home.home_entry, Some(1));
    assert_eq!(home.created, [original]);
    let metadata = change(
        &handle,
        &mut events,
        SessionCommand::ObserveMetadata {
            id: original,
            title: Some("Synthetic Home".into()),
            address: None,
        },
    );
    assert_eq!(metadata.home_entry, None);
    change(&handle, &mut events, SessionCommand::CloseTab(original));
    let reopened = change(
        &handle,
        &mut events,
        SessionCommand::HomeEntry {
            request_id: 2,
            action: HomeEntryAction::ReopenClosed,
        },
    );
    let replacement = reopened.model.active_id.expect("synthetic Home reopened");
    assert_eq!(reopened.home_entry, Some(2));
    assert_eq!(reopened.created, [replacement]);
    assert_eq!(reopened.model.tabs[0].metadata.address, "about:blank");
    let saved = change(&handle, &mut events, SessionCommand::SaveActive);
    let saved_id = saved.model.saved_pages[0].id;
    assert_eq!(saved.home_entry, None);
    change(&handle, &mut events, SessionCommand::CloseTab(replacement));
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::HomeEntry {
            request_id: 3,
            action: HomeEntryAction::OpenSaved(saved_id),
        },
    );
    assert_eq!(opened.home_entry, Some(3));
    assert_eq!(opened.created.len(), 1);
    let existing = change(
        &handle,
        &mut events,
        SessionCommand::HomeEntry {
            request_id: 4,
            action: HomeEntryAction::OpenSaved(saved_id),
        },
    );
    // A no-op still acknowledges its request; it cannot replay an earlier creation.
    assert_eq!(existing.home_entry, Some(4));
    assert!(existing.created.is_empty());
    assert_eq!(existing.revision, opened.revision);
    stop(handle, &mut events);
    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.home_entry, None);
    assert!(restored.created.is_empty());
    assert_eq!(restored.model, existing.model);
    stop(handle, &mut events);
    fixture.cleanup();
}

fn assert_only_live_search(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    id: TabId,
    metadata: &TabMetadata,
) {
    send(
        handle,
        SessionCommand::Query {
            request_id: 73,
            text: String::new(),
            scope: Scope::Everything,
            policy: query_policy(),
        },
    );
    let SessionEvent::Search { request_id, result } = next_event(events) else {
        panic!("expected synthetic search after removing a saved row")
    };
    assert_eq!(request_id, 73);
    let results = result.expect("synthetic live-only search succeeds");
    assert_eq!(results.counts.total(), 1);
    assert_eq!(results.counts.tabs, 1);
    assert_eq!(results.counts.saved, 0);
    assert_eq!(results.hits.len(), 1);
    assert_eq!(results.hits[0].target, Target::Tab(id));
    assert_eq!(results.hits[0].title, metadata.title);
    assert_eq!(results.hits[0].address, metadata.address);
}

#[test]
fn removed_saved_row_stays_removed_after_live_reopen_stale_callbacks_and_restart() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: SAVED_ADDRESS.into(),
        },
    );
    let original = opened
        .model
        .active_id
        .expect("synthetic saved page is active");
    let saved = change(&handle, &mut events, SessionCommand::SaveActive);
    let saved_id = saved.model.saved_pages[0].id;
    let navigated = change(
        &handle,
        &mut events,
        SessionCommand::ObserveMetadata {
            id: original,
            title: Some("Synthetic unlinked live page".into()),
            address: Some(LIVE_ADDRESS.into()),
        },
    );
    assert_eq!(navigated.model.saved_pages, saved.model.saved_pages);
    let metadata = navigated.model.tabs[0].metadata.clone();

    // Removing the saved destination leaves its current page as an ordinary live row.
    let unlinked = change(&handle, &mut events, SessionCommand::Unsave(saved_id));
    assert_eq!(unlinked.revision, navigated.revision + 1);
    assert!(unlinked.model.saved_pages.is_empty());
    assert_eq!(unlinked.model.tabs.len(), 1);
    assert_eq!(unlinked.model.tabs[0].id, original);
    assert_eq!(unlinked.model.tabs[0].saved_page_id, None);
    assert_eq!(unlinked.model.tabs[0].metadata, metadata);
    assert_eq!(unlinked.model.active_id, Some(original));
    assert_only_live_search(&handle, &mut events, original, &metadata);

    // Delayed callbacks from the closed instance cannot affect its fresh replacement.
    send(&handle, SessionCommand::CloseTab(original));
    send(&handle, SessionCommand::ReopenClosed);
    send(
        &handle,
        SessionCommand::ObserveMetadata {
            id: original,
            title: Some("Synthetic stale removed instance".into()),
            address: Some("https://stale-unlinked.example.test/".into()),
        },
    );
    send(&handle, SessionCommand::ObserveClosed(original));
    let closed = state(&mut events);
    assert_eq!(closed.revision, unlinked.revision + 1);
    assert!(closed.model.tabs.is_empty());
    assert!(closed.model.saved_pages.is_empty());
    let reopened = state(&mut events);
    let replacement = reopened.model.active_id.expect("ordinary page reopened");
    assert!(replacement.get() > original.get());
    assert_eq!(reopened.revision, closed.revision + 1);
    assert_eq!(reopened.created, [replacement]);
    assert!(reopened.model.saved_pages.is_empty());
    assert_eq!(reopened.model.tabs.len(), 1);
    assert_eq!(reopened.model.tabs[0].saved_page_id, None);
    assert_eq!(reopened.model.tabs[0].metadata, metadata);
    for _ in 0..2 {
        let stale = state(&mut events);
        assert_eq!(stale.model, reopened.model);
        assert_eq!(stale.revision, reopened.revision);
        assert!(stale.created.is_empty());
        assert_eq!(stale.navigate, None);
    }
    assert_only_live_search(&handle, &mut events, replacement, &metadata);
    stop(handle, &mut events);

    // The durable reload must keep the removed row absent from both model and search.
    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.previous_shutdown, ShutdownState::Clean);
    assert_eq!(restored.model, reopened.model);
    assert_eq!(restored.revision, reopened.revision);
    assert!(restored.created.is_empty());
    assert_eq!(restored.navigate, None);
    assert_only_live_search(&handle, &mut events, replacement, &metadata);
    stop(handle, &mut events);
    fixture.cleanup();
}

const DAMAGED_RECOVERY_PRIMARY: &[u8] = b"synthetic damaged session recovery primary";
const RECOVERY_BACKUP_TIME: i64 = 1_800_086_400;

fn seed_recovery(fixture: &Fixture) -> StoredSnapshot {
    // Two snapshots make the offered date and restored model distinguish the newest backup.
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic recovery store");
    let store = Store::open(Arc::new(lease)).expect("open synthetic recovery store");
    let mut model = TabModel::new();
    let saved = model
        .open(
            TabKind::Web,
            TabMetadata {
                title: "Synthetic saved recovery page".to_owned(),
                address: SAVED_ADDRESS.to_owned(),
                ..TabMetadata::default()
            },
            Activation::Foreground,
        )
        .expect("create synthetic saved page");
    model
        .save_page(saved)
        .expect("save synthetic recovery destination");
    let revision = store
        .save(0, model.snapshot())
        .expect("save first recovery state");
    assert!(matches!(
        store.backup_if_due(RECOVERY_BACKUP_TIME - 86_400, true),
        Ok(BackupOutcome::Created(_))
    ));
    model
        .update_metadata(
            saved,
            TabMetadata {
                title: "Synthetic latest recovery page".to_owned(),
                address: LIVE_ADDRESS.to_owned(),
                ..TabMetadata::default()
            },
        )
        .expect("navigate synthetic saved instance");
    let snapshot = model.snapshot();
    let revision = store
        .save(revision, snapshot.clone())
        .expect("save newest recovery state");
    assert!(matches!(
        store.backup_if_due(RECOVERY_BACKUP_TIME, true),
        Ok(BackupOutcome::Created(metadata)) if metadata.created_at == RECOVERY_BACKUP_TIME
    ));
    store.shutdown().expect("close synthetic recovery seed");
    StoredSnapshot { revision, snapshot }
}

fn damage_recovery_primary(fixture: &Fixture) {
    checked_io(fs::write(
        fixture.0.join("atlas.db"),
        DAMAGED_RECOVERY_PRIMARY,
    ));
}

fn recovery_offer(events: &mut UnboundedReceiver<SessionEvent>) -> u64 {
    match next_event(events) {
        SessionEvent::RecoveryOffered(prompt) => {
            assert_eq!(prompt.created_at, RECOVERY_BACKUP_TIME);
            prompt.offer_id
        }
        event => panic!("expected a synthetic recovery offer before any state, got {event:?}"),
    }
}

fn recovery_unavailable(events: &mut UnboundedReceiver<SessionEvent>) {
    assert!(matches!(
        next_event(events),
        SessionEvent::Failure {
            error: SessionError::RecoveryUnavailable,
            fatal: false
        }
    ));
}

fn assert_unreplaced_primary(fixture: &Fixture) {
    assert_eq!(
        checked_io(fs::read(fixture.0.join("atlas.db"))),
        DAMAGED_RECOVERY_PRIMARY
    );
    assert!(!fixture.0.join("atlas.recovery-intent").exists());
}

fn remove_preserved_recovery_bundle(fixture: &Fixture, expected_primary: Option<&[u8]>) {
    use std::os::windows::fs::MetadataExt;

    // Only a successful recovery in this exclusively created fixture owns these originals.
    let mut bundles = Vec::new();
    for entry in checked_io(fs::read_dir(&fixture.0)) {
        let entry = checked_io(entry);
        let name = entry.file_name();
        let Some(name) = name
            .to_str()
            .and_then(|name| name.strip_prefix("recovery-"))
        else {
            continue;
        };
        let (stamp, sequence) = name
            .split_once('-')
            .expect("recognized recovery bundle name");
        assert_eq!(stamp.len(), 32);
        assert_eq!(sequence.len(), 16);
        assert!(
            stamp
                .bytes()
                .chain(sequence.bytes())
                .all(|byte| byte.is_ascii_hexdigit())
        );
        let path = entry.path();
        assert_eq!(path.parent(), Some(fixture.0.as_path()));
        let metadata = checked_io(fs::symlink_metadata(&path));
        assert!(metadata.is_dir());
        assert_eq!(
            metadata.file_attributes() & 0x400,
            0,
            "refuse reparse points"
        );
        bundles.push(path);
    }
    assert_eq!(
        bundles.len(),
        1,
        "one confirmation creates one recovery bundle"
    );
    let bundle = &bundles[0];
    if let Some(expected) = expected_primary {
        assert_eq!(checked_io(fs::read(bundle.join("atlas.db"))), expected);
    } else {
        assert!(checked_io(fs::read_dir(bundle)).next().is_none());
    }
    for entry in checked_io(fs::read_dir(bundle)) {
        let entry = checked_io(entry);
        assert!(matches!(
            entry.file_name().to_str(),
            Some("atlas.db" | "atlas.db-wal" | "atlas.db-shm" | "atlas.db-journal")
        ));
        let path = entry.path();
        assert_eq!(path.parent(), Some(bundle.as_path()));
        let metadata = checked_io(fs::symlink_metadata(&path));
        assert!(metadata.is_file());
        assert_eq!(
            metadata.file_attributes() & 0x400,
            0,
            "refuse reparse points"
        );
        checked_io(fs::remove_file(path));
    }
    checked_io(fs::remove_dir(bundle));
}

#[test]
fn recovery_offer_waits_for_confirmation_and_shutdown_cancels_without_replacement() {
    let fixture = Fixture::new();
    seed_recovery(&fixture);
    damage_recovery_primary(&fixture);
    let (handle, mut events) = fixture.start();
    let offer_id = recovery_offer(&mut events);
    assert_unreplaced_primary(&fixture);
    assert!(matches!(
        WindowsDirectoryLease::prepare(&fixture.0),
        Err(LocationError::StorageBusy)
    ));

    // Neither a stale confirmation nor a normal tab command can bypass the offer.
    send(
        &handle,
        SessionCommand::ConfirmRecovery {
            offer_id: offer_id.wrapping_add(1),
        },
    );
    recovery_unavailable(&mut events);
    send(
        &handle,
        SessionCommand::NewTab {
            address: "https://blocked-recovery.example.test/".to_owned(),
        },
    );
    recovery_unavailable(&mut events);
    assert_unreplaced_primary(&fixture);
    stop(handle, &mut events);
    assert_unreplaced_primary(&fixture);

    // A second worker can acquire the cancelled offer; no restoration occurred implicitly.
    let (handle, mut events) = fixture.start();
    let next_offer_id = recovery_offer(&mut events);
    assert_ne!(next_offer_id, offer_id);
    send(&handle, SessionCommand::ConfirmRecovery { offer_id });
    recovery_unavailable(&mut events);
    assert_unreplaced_primary(&fixture);
    stop(handle, &mut events);
    assert_unreplaced_primary(&fixture);
    fixture.cleanup();
}

#[test]
fn matching_recovery_confirmation_restores_once_and_survives_restart() {
    let fixture = Fixture::new();
    let expected = seed_recovery(&fixture);
    damage_recovery_primary(&fixture);
    let (handle, mut events) = fixture.start();
    let offer_id = recovery_offer(&mut events);
    assert_unreplaced_primary(&fixture);
    send(&handle, SessionCommand::ConfirmRecovery { offer_id });
    let restored = state(&mut events);
    assert_eq!(restored.revision, expected.revision);
    assert_eq!(restored.model, expected.snapshot);
    assert!(restored.created.is_empty());
    assert_eq!(restored.navigate, None);
    assert!(!fixture.0.join("atlas.recovery-intent").exists());

    send(&handle, SessionCommand::ConfirmRecovery { offer_id });
    recovery_unavailable(&mut events);
    stop(handle, &mut events);
    let (handle, mut events) = fixture.start();
    let restarted = state(&mut events);
    assert_eq!(restarted.revision, expected.revision);
    assert_eq!(restarted.model, expected.snapshot);
    assert_eq!(restarted.previous_shutdown, ShutdownState::Clean);
    stop(handle, &mut events);
    remove_preserved_recovery_bundle(&fixture, Some(DAMAGED_RECOVERY_PRIMARY));
    fixture.cleanup();
}

#[test]
fn dropping_recovery_event_receiver_releases_offer_without_confirmation() {
    let fixture = Fixture::new();
    seed_recovery(&fixture);
    damage_recovery_primary(&fixture);
    let (handle, mut events) = fixture.start();
    recovery_offer(&mut events);
    disconnect_recovery(handle, events);
    assert_unreplaced_primary(&fixture);
    let (handle, mut events) = fixture.start();
    recovery_offer(&mut events);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn busy_startup_remains_the_original_fatal_error_without_a_recovery_offer() {
    let fixture = Fixture::new();
    seed_recovery(&fixture);
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("hold synthetic storage lock");
    let before = checked_io(fs::read(fixture.0.join("atlas.db")));
    let (handle, mut events) = fixture.start();
    let expected = SessionError::Location(LocationError::StorageBusy);
    assert!(matches!(
        next_event(&mut events),
        SessionEvent::Failure { error, fatal: true } if error == expected
    ));
    assert!(matches!(next_event(&mut events), SessionEvent::Stopped));
    assert_eq!(handle.join(), Err(expected));
    assert_eq!(checked_io(fs::read(fixture.0.join("atlas.db"))), before);
    drop(lease);
    fixture.cleanup();
}

#[test]
fn damaged_startup_without_a_backup_preserves_both_failure_and_primary() {
    let fixture = Fixture::new();
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic empty store");
    let store = Store::open(Arc::new(lease)).expect("open synthetic empty store");
    store
        .shutdown()
        .expect("close synthetic store without backups");
    damage_recovery_primary(&fixture);
    let (handle, mut events) = fixture.start();
    let error = match next_event(&mut events) {
        SessionEvent::Failure { error, fatal: true } => error,
        event => panic!("expected failed synthetic recovery without an offer, got {event:?}"),
    };
    let SessionError::RecoveryFailed { startup, recovery } = &error else {
        panic!("expected both synthetic startup and recovery failures");
    };
    assert!(matches!(
        startup.as_ref(),
        SessionError::Store(StoreError::Database { code: Some(code), .. })
            if code & 0xff == 26
    ));
    assert_eq!(
        recovery,
        &atlas_platform::RecoveryError::NoValidBackup.to_string()
    );
    assert!(matches!(next_event(&mut events), SessionEvent::Stopped));
    assert_eq!(handle.join(), Err(error));
    assert_unreplaced_primary(&fixture);
    fixture.cleanup();
}

fn assert_no_recovery_event(events: &mut UnboundedReceiver<SessionEvent>) {
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    assert!(matches!(
        Pin::new(events).poll_next(&mut context),
        Poll::Pending
    ));
}

#[test]
fn ordinary_read_failure_on_healthy_primary_never_authorizes_recovery() {
    let fixture = Fixture::new();
    seed_recovery(&fixture);
    let before = checked_io(fs::read(fixture.0.join("atlas.db")));
    let original = SessionError::Store(StoreError::Database {
        operation: atlas_store::StoreOperation::Read,
        code: Some(10),
    });
    let (commands, receiver) = mpsc::channel();
    commands
        .send(SessionCommand::Shutdown)
        .expect("bound an unexpected recovery offer");
    let (sender, mut events) = unbounded();
    assert_eq!(
        recovery::confirm_failed_start(&fixture.0, original.clone(), &receiver, &sender),
        Err(SessionError::RecoveryFailed {
            startup: Box::new(original),
            recovery: atlas_platform::RecoveryError::HealthyPrimary.to_string(),
        })
    );
    assert_no_recovery_event(&mut events);
    assert!(matches!(receiver.try_recv(), Ok(SessionCommand::Shutdown)));
    assert_eq!(checked_io(fs::read(fixture.0.join("atlas.db"))), before);
    assert!(!fixture.0.join("atlas.recovery-intent").exists());
    fixture.cleanup();
}

#[test]
fn uncertain_cleanup_and_rollback_errors_refuse_even_a_recoverable_primary() {
    let fixture = Fixture::new();
    seed_recovery(&fixture);
    damage_recovery_primary(&fixture);
    let read = StoreError::Database {
        operation: atlas_store::StoreOperation::Read,
        code: Some(10),
    };
    let close = StoreError::Database {
        operation: atlas_store::StoreOperation::Close,
        code: Some(10),
    };
    for original in [
        SessionError::CleanupFailed {
            primary: Box::new(SessionError::Store(read.clone())),
            cleanup: close.clone(),
        },
        SessionError::Store(StoreError::CleanupFailed {
            primary: Box::new(read),
            cleanup: Box::new(close),
        }),
        SessionError::Store(StoreError::Database {
            operation: atlas_store::StoreOperation::Rollback,
            code: Some(10),
        }),
    ] {
        // A mistaken offer must terminate promptly, then fail the exact-error assertion.
        let (commands, receiver) = mpsc::channel();
        commands
            .send(SessionCommand::Shutdown)
            .expect("bound an unexpected recovery offer");
        let (sender, mut events) = unbounded();
        assert_eq!(
            recovery::confirm_failed_start(&fixture.0, original.clone(), &receiver, &sender),
            Err(original)
        );
        assert_no_recovery_event(&mut events);
        assert!(matches!(receiver.try_recv(), Ok(SessionCommand::Shutdown)));
        assert_unreplaced_primary(&fixture);
    }
    fixture.cleanup();
}

fn disconnect_recovery(handle: SessionHandle, events: UnboundedReceiver<SessionEvent>) {
    drop(events);
    // Retain the command sender: event receiver loss alone must release storage ownership.
    let deadline = Instant::now() + Duration::from_secs(60);
    while !handle.worker.is_finished() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "recovery disconnect deadline expired");
        thread::park_timeout(remaining.min(Duration::from_millis(10)));
    }
    assert_eq!(handle.join(), Err(SessionError::EventReceiverClosed));
}

fn recovery_backup_files(fixture: &Fixture) -> Vec<(PathBuf, Vec<u8>)> {
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard seeded backup inventory");
    let archive = lease.backup_archive().expect("seeded backup archive");
    let candidates = archive.candidates().expect("read seeded backup inventory");
    assert_eq!(candidates.len(), 2);
    let mut files = Vec::new();
    for candidate in candidates {
        let database = candidate.file.database_path().to_owned();
        let marker = database.parent().expect("backup bundle").join("complete");
        for path in [database, marker] {
            let bytes = checked_io(fs::read(&path));
            files.push((path, bytes));
        }
    }
    files
}

fn remove_seeded_primary(fixture: &Fixture) {
    // The seed has closed every connection; remove only this test's established primary.
    for name in ["atlas.db-wal", "atlas.db-shm", "atlas.db-journal"] {
        assert!(!fixture.0.join(name).exists());
    }
    checked_io(fs::remove_file(fixture.0.join("atlas.db")));
    assert_primary_absent(fixture);
}

fn assert_primary_absent(fixture: &Fixture) {
    assert!(!fixture.0.join("atlas.db").exists());
    assert!(!fixture.0.join("atlas.recovery-intent").exists());
}

fn assert_files_unchanged(files: &[(PathBuf, Vec<u8>)]) {
    for (path, expected) in files {
        assert_eq!(checked_io(fs::read(path)), *expected);
    }
}

fn refused_missing_primary(fixture: &Fixture) {
    let expected = SessionError::Recovery(atlas_platform::RecoveryError::NoValidBackup.to_string());
    let (handle, mut events) = fixture.start();
    assert!(matches!(
        next_event(&mut events),
        SessionEvent::Failure { error, fatal: true } if error == expected
    ));
    assert!(matches!(next_event(&mut events), SessionEvent::Stopped));
    assert_eq!(handle.join(), Err(expected));
    assert_primary_absent(fixture);
}

#[test]
fn missing_primary_offer_cancels_and_disconnects_without_creating_a_database() {
    let fixture = Fixture::new();
    seed_recovery(&fixture);
    let retained_files = recovery_backup_files(&fixture);
    let lifecycle = fixture.0.join("atlas.lifecycle");
    let lifecycle_before = checked_io(fs::read(&lifecycle));
    remove_seeded_primary(&fixture);

    let (handle, mut events) = fixture.start();
    let first_offer = recovery_offer(&mut events);
    assert_primary_absent(&fixture);
    assert_files_unchanged(&retained_files);
    assert!(matches!(
        WindowsDirectoryLease::prepare(&fixture.0),
        Err(LocationError::StorageBusy)
    ));
    stop(handle, &mut events);
    assert_primary_absent(&fixture);
    assert_files_unchanged(&retained_files);
    assert_eq!(checked_io(fs::read(&lifecycle)), lifecycle_before);

    let (handle, mut events) = fixture.start();
    let next_offer = recovery_offer(&mut events);
    assert_ne!(next_offer, first_offer);
    send(
        &handle,
        SessionCommand::ConfirmRecovery {
            offer_id: first_offer,
        },
    );
    recovery_unavailable(&mut events);
    assert_primary_absent(&fixture);
    disconnect_recovery(handle, events);
    assert_primary_absent(&fixture);
    assert_files_unchanged(&retained_files);
    assert_eq!(checked_io(fs::read(&lifecycle)), lifecycle_before);

    // Receiver loss releases the retained guard without installing the offered backup.
    let (handle, mut events) = fixture.start();
    recovery_offer(&mut events);
    stop(handle, &mut events);
    assert_primary_absent(&fixture);
    assert_files_unchanged(&retained_files);
    assert_eq!(checked_io(fs::read(&lifecycle)), lifecycle_before);
    fixture.cleanup();
}

#[test]
fn missing_primary_confirmation_restores_the_offered_snapshot_once() {
    let fixture = Fixture::new();
    let expected = seed_recovery(&fixture);
    let backups = recovery_backup_files(&fixture);
    remove_seeded_primary(&fixture);
    let (handle, mut events) = fixture.start();
    let offer_id = recovery_offer(&mut events);
    assert_primary_absent(&fixture);
    send(&handle, SessionCommand::ConfirmRecovery { offer_id });
    let restored = state(&mut events);
    assert_eq!(restored.model, expected.snapshot);
    assert_eq!(restored.revision, expected.revision);
    assert!(restored.created.is_empty());
    assert_eq!(restored.navigate, None);
    assert!(fixture.0.join("atlas.db").is_file());
    assert!(!fixture.0.join("atlas.recovery-intent").exists());
    send(&handle, SessionCommand::ConfirmRecovery { offer_id });
    recovery_unavailable(&mut events);
    stop(handle, &mut events);

    let (handle, mut events) = fixture.start();
    let restarted = state(&mut events);
    assert_eq!(restarted.model, expected.snapshot);
    assert_eq!(restarted.revision, expected.revision);
    assert_eq!(restarted.previous_shutdown, ShutdownState::Clean);
    stop(handle, &mut events);
    assert_files_unchanged(&backups);
    remove_preserved_recovery_bundle(&fixture, None);
    fixture.cleanup();
}

#[test]
fn existing_empty_state_is_refused_without_blank_initialization() {
    let fixture = Fixture::new();
    checked_io(fs::create_dir(&fixture.0));
    refused_missing_primary(&fixture);
    // A second attempt must not mistake the first attempt's empty lifecycle for freshness.
    refused_missing_primary(&fixture);
    fixture.cleanup();
}

#[test]
fn missing_primary_with_invalid_backup_markers_never_falls_through_to_empty_state() {
    let fixture = Fixture::new();
    seed_recovery(&fixture);
    let originals = recovery_backup_files(&fixture);
    let malformed = b"synthetic invalid backup completion marker";
    let mut retained_files = Vec::new();
    for (path, original) in &originals {
        if path.file_name().is_some_and(|name| name == "complete") {
            checked_io(fs::write(path, malformed));
            retained_files.push((path.clone(), malformed.to_vec()));
        } else {
            retained_files.push((path.clone(), original.clone()));
        }
    }
    remove_seeded_primary(&fixture);
    refused_missing_primary(&fixture);
    assert_files_unchanged(&retained_files);

    // Restore only the owned marker bytes so capability-based fixture cleanup can recognize them.
    for (path, original) in originals {
        if path.file_name().is_some_and(|name| name == "complete") {
            checked_io(fs::write(path, original));
        }
    }
    fixture.cleanup();
}

#[test]
fn healthy_existing_primary_starts_directly_with_its_saved_state() {
    let fixture = Fixture::new();
    let expected = seed_recovery(&fixture);
    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model, expected.snapshot);
    assert_eq!(restored.revision, expected.revision);
    assert_eq!(restored.previous_shutdown, ShutdownState::Clean);
    assert!(restored.created.is_empty());
    assert_eq!(restored.navigate, None);
    stop(handle, &mut events);
    fixture.cleanup();
}

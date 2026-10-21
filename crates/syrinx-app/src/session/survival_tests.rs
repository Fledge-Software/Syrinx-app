//! Proves that one refused write, a busy database or a window that stopped listening no longer
//! ends the session worker, while lost or untrustworthy storage still stops it (#9).

use syrinx_store::{EncodingField, StoreOperation};

use super::*;

/// The settings key the sidebar uses for open folders; its value grows with each open folder.
const EXPANDED_FOLDERS_KEY: &str = "sidebar.expanded_folders";

/// A storage failure as the store reports it, with a SQLite result code.
fn database(operation: StoreOperation, code: i32) -> SessionError {
    SessionError::Store(StoreError::Database {
        operation,
        code: Some(code),
    })
}

/// The next event, which must be a failure the worker survived.
fn survived(events: &mut UnboundedReceiver<SessionEvent>) -> SessionError {
    match next_event(events) {
        SessionEvent::Failure {
            error,
            fatal: false,
        } => error,
        event => panic!("expected a survived failure, got {event:?}"),
    }
}

/// The innermost error of an operation-tagged failure.
fn cause(error: &SessionError) -> &SessionError {
    match error {
        SessionError::During { error, .. } => cause(error),
        error => error,
    }
}

/// Holds SQLite's write lock from a second connection, as another writer would.
fn hold_write_lock(fixture: &Fixture) -> rusqlite::Connection {
    let connection = rusqlite::Connection::open(fixture.0.join("syrinx.db"))
        .expect("open a second synthetic connection");
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .expect("take the synthetic write lock");
    connection
}

fn release_write_lock(connection: rusqlite::Connection) {
    connection
        .execute_batch("ROLLBACK")
        .expect("release the synthetic write lock");
    connection
        .close()
        .map_err(|(_, error)| error)
        .expect("close the second synthetic connection");
}

#[test]
fn stop_classification_keeps_lost_storage_fatal_and_one_refused_write_survivable() {
    // A busy or locked database, a full disk, a refused row and an in-memory index refusal pass.
    for error in [
        database(StoreOperation::BeginWrite, 5),
        database(StoreOperation::Commit, 517),
        database(StoreOperation::Write, 6),
        database(StoreOperation::Write, 13),
        database(StoreOperation::Write, 275),
        SessionError::Store(StoreError::InvalidEncoding(EncodingField::Setting)),
        SessionError::Store(StoreError::TooManyRecords),
        SessionError::Search(syrinx_search::SearchError::TooManyEntries),
        SessionError::EventReceiverClosed,
        SessionError::ClockUnavailable,
    ] {
        let tagged = stop::during("switching tabs")(error.clone());
        assert!(stop::survivable(&error), "{error:?}");
        assert!(stop::survivable(&tagged), "{tagged:?}");
    }
    // Lost or damaged storage, a stale revision and an unfinished rollback still stop the loop.
    for error in [
        SessionError::Location(LocationError::StorageBusy),
        SessionError::Store(StoreError::Closed),
        SessionError::Store(StoreError::WorkerUnavailable),
        SessionError::Store(StoreError::ReplyAbandoned),
        SessionError::Store(StoreError::StaleRevision {
            expected: 4,
            actual: 5,
        }),
        database(StoreOperation::Write, 11),
        database(StoreOperation::Write, 26),
        database(StoreOperation::Write, 8),
        database(StoreOperation::Rollback, 5),
        SessionError::Store(StoreError::CleanupFailed {
            primary: Box::new(StoreError::TooManyRecords),
            cleanup: Box::new(StoreError::Closed),
        }),
        SessionError::WorkerPanicked,
        SessionError::RecoveryUnavailable,
    ] {
        assert!(!stop::survivable(&error), "{error:?}");
    }
}

#[test]
fn an_oversized_setting_is_refused_without_stopping_the_worker() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    // Sixty open folders' IDs exceed the settings table's 256-byte value limit.
    let open_folders = "1234 ".repeat(60);
    send(
        &handle,
        SessionCommand::SaveSettings(vec![(EXPANDED_FOLDERS_KEY.to_owned(), open_folders)]),
    );

    let refused = survived(&mut events);
    assert_eq!(
        cause(&refused),
        &SessionError::Store(StoreError::InvalidEncoding(EncodingField::Setting))
    );
    assert!(refused.to_string().ends_with("(while saving settings)"));

    // The worker still saves tabs, and the refusal is kept beside the database.
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: LIVE_ADDRESS.into(),
        },
    );
    assert_eq!(opened.model.tabs.len(), 1);
    stop(handle, &mut events);
    let lines = read_stop_log(&fixture);
    assert_eq!(lines, [stop::survived_line(&refused)]);
    assert!(!lines[0].contains("example.test"), "{lines:?}");

    let (restarted, mut events) = fixture.start();
    assert_eq!(state(&mut events).model, opened.model);
    stop(restarted, &mut events);
    fixture.cleanup();
}

#[test]
fn a_busy_database_delays_saving_without_stopping_the_worker() {
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
    let id = opened.model.active_id.expect("synthetic page is active");

    // Another writer holds the database past SQLite's busy timeout.
    let lock = hold_write_lock(&fixture);
    send(
        &handle,
        SessionCommand::ObserveMetadata {
            id,
            title: Some("Busy synthetic page".into()),
            address: Some(LIVE_ADDRESS.into()),
        },
    );
    // The window still sees the page, then hears that the save and the visit were refused.
    let shown = state(&mut events);
    assert_eq!(shown.model.tabs[0].metadata.address, LIVE_ADDRESS);
    assert_eq!(shown.revision, opened.revision);
    for _ in ["tab save", "history visit"] {
        let refused = survived(&mut events);
        assert!(matches!(
            cause(&refused),
            SessionError::Store(StoreError::Database { code: Some(code), .. }) if code & 0xff == 5
        ));
        assert!(
            refused
                .to_string()
                .ends_with("(while recording a page's title or address)")
        );
    }
    release_write_lock(lock);

    // The next command saves the change the busy database refused, and it survives a restart.
    let focused = change(&handle, &mut events, SessionCommand::FocusTab(id));
    assert!(focused.revision > opened.revision);
    stop(handle, &mut events);
    let (restarted, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model.tabs[0].metadata.address, LIVE_ADDRESS);
    stop(restarted, &mut events);
    fixture.cleanup();
}

#[test]
fn the_worker_keeps_saving_after_the_window_stops_listening() {
    let fixture = Fixture::new();
    let (commands, receiver) = mpsc::channel();
    let (sender, mut events) = unbounded();
    let worker = run_in_thread(&fixture, receiver, sender);
    // Once the restored tabs arrive the loop is serving; the window then stops listening.
    while !matches!(next_any_event(&mut events), SessionEvent::State(_)) {}
    drop(events);
    commands
        .send(SessionCommand::NewTab {
            address: LIVE_ADDRESS.into(),
        })
        .expect("queue a command whose answer cannot be delivered");
    drop(commands);

    worker
        .join()
        .expect("join the run thread")
        .expect("the loop ends only when the commands do");

    // The log names the lost window once, with the operation and no address.
    let lines = read_stop_log(&fixture);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("event receiver closed"), "{lines:?}");
    assert!(lines[0].ends_with("(while opening a new tab)"), "{lines:?}");
    assert!(!lines[0].contains("example.test"), "{lines:?}");

    // The tab opened after the window stopped listening was still saved.
    let (restarted, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model.tabs.len(), 1);
    assert_eq!(restored.model.tabs[0].metadata.address, LIVE_ADDRESS);
    stop(restarted, &mut events);
    fixture.cleanup();
}

#[test]
fn a_fatal_stop_is_still_kept_in_the_log_beside_the_database() {
    let fixture = Fixture::new();
    let lease =
        Arc::new(WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic stop store"));
    let journal = DeletionJournal::open(&lease).expect("open synthetic journal");
    let store = Store::open(lease).expect("open synthetic stop store");
    let mut worker = Worker::restore(&store, journal).expect("restore synthetic worker");
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

    // Storage advances behind the worker, so its next save is refused as stale.
    store
        .save(acknowledged.revision, acknowledged.model.clone())
        .expect("advance synthetic store revision");
    let (commands, receiver) = mpsc::channel();
    commands
        .send(SessionCommand::CloseActive)
        .expect("queue the failing command");
    let error = serve_accepted(
        Arc::default(),
        StartupFacts::default(),
        store,
        worker,
        receiver,
        &sender,
        &fixture.0,
    )
    .expect_err("a stale revision stops the loop");

    assert!(matches!(
        cause(&error),
        SessionError::Store(StoreError::StaleRevision { .. })
    ));
    assert_eq!(read_stop_log(&fixture), [stop::stop_line(&error)]);
    fixture.cleanup();
}

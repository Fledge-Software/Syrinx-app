//! Proves that a busy database, one refused setting or a window that stopped listening no
//! longer ends the session worker, while damaged or lost storage still stops it (#9).

use syrinx_store::{EncodingField, StoreOperation};

use super::*;

/// The settings key the sidebar uses for open folders; its value grows with each open folder.
const EXPANDED_FOLDERS_KEY: &str = "sidebar.expanded_folders";

/// A storage failure as the store reports it, with a SQLite result code.
fn database(operation: StoreOperation, code: Option<i32>) -> SessionError {
    SessionError::Store(StoreError::Database { operation, code })
}

/// SQLITE_BUSY from the store's writer.
fn busy() -> SessionError {
    database(StoreOperation::BeginWrite, Some(5))
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

fn is_busy(error: &SessionError) -> bool {
    matches!(
        cause(error),
        SessionError::Store(StoreError::Database { code: Some(code), .. }) if code & 0xff == 5
    )
}

fn read_survived_log(fixture: &Fixture) -> Vec<String> {
    let log = checked_io(fs::read_to_string(fixture.0.join(stop::SURVIVED_LOG_NAME)));
    log.lines().map(str::to_owned).collect()
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

/// A worker on the fixture's folder, driven directly so a test can run its idle step.
fn direct_worker(fixture: &Fixture) -> (Store, Worker) {
    let lease =
        Arc::new(WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic store"));
    let journal = DeletionJournal::open(&lease).expect("open synthetic journal");
    let page_text = page_text::PageText::open(&lease);
    let store = Store::open(lease).expect("open synthetic store");
    let worker = Worker::restore(&store, journal, page_text).expect("restore synthetic worker");
    (store, worker)
}

fn close_direct(fixture: Fixture, store: Store, worker: Worker) {
    // The worker holds the deletion journal until it is dropped.
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fixture.cleanup();
}

#[test]
fn stop_classification_keeps_damaged_storage_fatal_and_one_refused_write_survivable() {
    // A busy or locked database, a full disk, a refused setting and an index refusal pass.
    for error in [
        busy(),
        database(StoreOperation::Commit, Some(517)),
        database(StoreOperation::Write, Some(6)),
        database(StoreOperation::Write, Some(13)),
        database(StoreOperation::Write, Some(275)),
        SessionError::Store(StoreError::InvalidEncoding(EncodingField::Setting)),
        SessionError::Store(StoreError::TooManyRecords),
        SessionError::Search(syrinx_search::SearchError::TooManyEntries),
        SessionError::EventReceiverClosed,
        SessionError::ClockUnavailable,
        SessionError::AfterEarlier {
            earlier: Box::new(busy()),
            error: Box::new(SessionError::IndexFolder(
                LocationError::UnexpectedEntryKind,
            )),
        },
    ] {
        let tagged = stop::during("switching tabs")(error.clone());
        assert!(stop::survivable(&error), "{error:?}");
        assert!(stop::survivable(&tagged), "{tagged:?}");
    }
    // Lost, damaged or changed storage, a read-side encoding error, an unknown database error,
    // an unfinished rollback and a tab model that cannot be saved still stop the loop.
    for error in [
        SessionError::Location(LocationError::StorageBusy),
        SessionError::Store(StoreError::Closed),
        SessionError::Store(StoreError::WorkerUnavailable),
        SessionError::Store(StoreError::ReplyAbandoned),
        SessionError::Store(StoreError::StaleRevision {
            expected: 4,
            actual: 5,
        }),
        SessionError::Store(StoreError::InvalidEncoding(EncodingField::SessionState)),
        SessionError::Store(StoreError::InvalidEncoding(EncodingField::Metadata)),
        SessionError::Store(StoreError::InvalidEncoding(EncodingField::Destination)),
        SessionError::Store(StoreError::InvalidEncoding(EncodingField::History)),
        database(StoreOperation::Write, None),
        database(StoreOperation::Write, Some(11)),
        database(StoreOperation::Write, Some(26)),
        database(StoreOperation::Write, Some(8)),
        database(StoreOperation::Read, Some(8202)),
        database(StoreOperation::Write, Some(8458)),
        database(StoreOperation::Rollback, Some(5)),
        SessionError::Store(StoreError::CleanupFailed {
            primary: Box::new(StoreError::TooManyRecords),
            cleanup: Box::new(StoreError::Closed),
        }),
        SessionError::TabsNotSaved {
            attempts: 3,
            error: Box::new(busy()),
        },
        SessionError::WorkerPanicked,
        SessionError::RecoveryUnavailable,
        // A chain stops the loop when either of its failures would.
        SessionError::AfterEarlier {
            earlier: Box::new(SessionError::WorkerPanicked),
            error: Box::new(busy()),
        },
    ] {
        assert!(!stop::survivable(&error), "{error:?}");
    }
    // A window that stopped listening is recognised at the end of a chain.
    assert!(stop::window_lost(&SessionError::AfterEarlier {
        earlier: Box::new(busy()),
        error: Box::new(SessionError::EventReceiverClosed),
    }));
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
    let lines = read_survived_log(&fixture);
    assert_eq!(lines, [stop::survived_line(&refused)]);
    assert!(!lines[0].contains("example.test"), "{lines:?}");
    assert!(!fixture.0.join(stop::LOG_NAME).exists());

    let (restarted, mut events) = fixture.start();
    assert_eq!(state(&mut events).model, opened.model);
    stop(restarted, &mut events);
    fixture.cleanup();
}

#[test]
fn a_busy_database_holds_the_change_until_a_later_save_without_stopping_the_worker() {
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
        assert!(is_busy(&refused), "{refused:?}");
        assert!(
            refused
                .to_string()
                .ends_with("(while recording a page's title or address)")
        );
    }
    release_write_lock(lock);

    // Within the retry delay a command changes only memory, without waiting on the database.
    let focused = change(&handle, &mut events, SessionCommand::FocusTab(id));
    assert_eq!(focused.revision, opened.revision);
    // Shutdown saves the held change, and it survives a restart.
    stop(handle, &mut events);
    let (restarted, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model.tabs[0].metadata.address, LIVE_ADDRESS);
    stop(restarted, &mut events);
    fixture.cleanup();
}

#[test]
fn the_idle_step_saves_a_change_a_busy_database_refused() {
    let fixture = Fixture::new();
    let (store, mut worker) = direct_worker(&fixture);
    let (sender, mut events) = unbounded();
    let lock = hold_write_lock(&fixture);
    worker
        .handle(
            &store,
            SessionCommand::NewTab {
                address: LIVE_ADDRESS.into(),
            },
            &sender,
        )
        .expect("a busy save is survived");
    let shown = state(&mut events);
    assert!(is_busy(&survived(&mut events)));
    assert!(worker.unsaved.pending());
    release_write_lock(lock);

    // Once the delay has passed, the idle wake-up saves the model with no command needed.
    worker.unsaved.retry_now();
    worker.idle(&store, &sender).expect("idle retry succeeds");
    assert!(!worker.unsaved.pending());
    let stored = store.load().expect("read the retried save");
    assert_eq!(stored.snapshot, shown.model);
    close_direct(fixture, store, worker);
}

#[test]
fn a_save_still_refused_at_shutdown_stops_the_worker_with_its_reason() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let lock = hold_write_lock(&fixture);
    send(
        &handle,
        SessionCommand::NewTab {
            address: LIVE_ADDRESS.into(),
        },
    );
    state(&mut events);
    assert!(is_busy(&survived(&mut events)));

    // Shutdown is the last chance to save, so its refusal is the reason the worker stops.
    send(&handle, SessionCommand::Shutdown);
    let error = match next_event(&mut events) {
        SessionEvent::Failure { error, fatal: true } => error,
        event => panic!("expected the shutdown save's stop, got {event:?}"),
    };
    // The lock also holds up the store's closing optimisation, reported alongside.
    let SessionError::CleanupFailed { primary, .. } = &error else {
        panic!("expected the shutdown save and the busy close, got {error:?}");
    };
    assert!(is_busy(primary), "{primary:?}");
    assert!(
        primary
            .to_string()
            .ends_with("(while saving the tabs at shutdown)")
    );
    assert!(matches!(next_event(&mut events), SessionEvent::Stopped));
    assert_eq!(handle.join(), Err(error.clone()));
    release_write_lock(lock);
    // The stop line is written before the store closes, so it names the save itself.
    assert_eq!(read_stop_log(&fixture), [stop::stop_line(primary)]);
    fixture.cleanup();
}

#[test]
fn an_interrupted_history_clear_is_finished_while_idle() {
    let fixture = Fixture::new();
    let (store, mut worker) = direct_worker(&fixture);
    let (sender, mut events) = unbounded();
    worker
        .handle(
            &store,
            SessionCommand::NewTab {
                address: "about:blank".into(),
            },
            &sender,
        )
        .expect("open a synthetic tab");
    let id = state(&mut events).model.active_id.expect("tab is active");
    worker
        .handle(
            &store,
            SessionCommand::ObserveMetadata {
                id,
                title: None,
                address: Some(LIVE_ADDRESS.into()),
            },
            &sender,
        )
        .expect("record a synthetic visit");
    state(&mut events);
    // One row for the tab and one for the remembered address.
    assert_eq!(worker.catalogue.len(), 2);

    // A busy database interrupts the clear; the dialog hears it and the worker goes on.
    let lock = hold_write_lock(&fixture);
    worker
        .handle(
            &store,
            SessionCommand::ClearHistory {
                request_id: 7,
                start: 0,
                end: i64::MAX,
            },
            &sender,
        )
        .expect("a busy clear is survived");
    match next_event(&mut events) {
        SessionEvent::HistoryCleared {
            request_id: 7,
            result: Err(error),
        } => assert!(is_busy(&error), "{error:?}"),
        event => panic!("expected the refused clear, got {event:?}"),
    }
    assert_eq!(worker.catalogue.len(), 2);
    release_write_lock(lock);

    // The idle wake-up finishes the journaled clear, so the cleared address stops showing.
    worker
        .idle(&store, &sender)
        .expect("idle finishes the clear");
    assert_eq!(worker.catalogue.len(), 1);
    assert!(store.recent_visits(10).expect("read history").is_empty());
    close_direct(fixture, store, worker);
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
    let lines = read_survived_log(&fixture);
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
fn a_flood_of_survived_failures_leaves_the_stop_log_intact() {
    let fixture = Fixture::new();
    checked_io(fs::create_dir(&fixture.0));
    let stopped = stop::during("switching tabs")(busy());
    stop::append_stop(&fixture.0, &stopped).expect("write the earlier stop");

    // Repeats of one failure collapse into a line and a count.
    let mut survival = stop::Survival::new();
    survival.keep_in(fixture.0.clone());
    survival.operation = "saving the tabs";
    for _ in 0..1_000 {
        survival
            .survive_shown(busy())
            .expect("a busy database is survived");
    }
    survival.finish();
    let first = stop::survived_line(&stop::during("saving the tabs")(busy()));
    assert_eq!(
        read_survived_log(&fixture),
        [first, "The line above repeated 999 more times".to_owned()]
    );

    // Even distinct lines far past both logs' caps only rotate the survived log.
    for number in 0..4_000 {
        let line = format!("synthetic survived failure {number}");
        stop::append_survived(&fixture.0, &line).expect("append a survived line");
    }
    assert_eq!(read_stop_log(&fixture), [stop::stop_line(&stopped)]);
    assert!(fixture.0.join(stop::OLD_SURVIVED_LOG_NAME).exists());
    assert!(!fixture.0.join(stop::OLD_LOG_NAME).exists());
    fixture.cleanup();
}

#[test]
fn a_fatal_stop_is_still_kept_in_the_log_beside_the_database() {
    let fixture = Fixture::new();
    let (store, mut worker) = direct_worker(&fixture);
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

/// Whether the database holds page-text capture as on.
fn stored_capture_on(store: &Store) -> bool {
    let stored = preferences::restore(store).expect("read synthetic settings");
    PageTextSetting::restored(&stored.settings).setting.on
}

#[test]
fn a_page_text_switch_off_a_busy_database_refused_is_saved_later_and_stays_off() {
    let fixture = Fixture::new();
    let (store, mut worker) = direct_worker(&fixture);
    let (sender, _events) = unbounded();
    let on = PageTextSetting {
        on: true,
        excluded: Vec::new(),
    };
    worker
        .handle(&store, SessionCommand::SavePageTextSetting(on), &sender)
        .expect("switching capture on is saved");
    assert!(stored_capture_on(&store));

    // The switch-off is refused, and reported rather than swallowed.
    let lock = hold_write_lock(&fixture);
    let off = PageTextSetting::default();
    let refused = worker
        .handle(&store, SessionCommand::SavePageTextSetting(off), &sender)
        .expect_err("a busy database refuses the switch-off");
    assert!(is_busy(&refused), "{refused:?}");
    assert!(worker.page_text_setting.unsaved());
    release_write_lock(lock);

    // The idle step saves it, so a restart reads capture as off.
    worker.idle(&store, &sender).expect("idle retry succeeds");
    assert!(!worker.page_text_setting.unsaved());
    assert!(!stored_capture_on(&store));
    close_direct(fixture, store, worker);
}

#[test]
fn a_page_text_switch_off_still_refused_at_shutdown_is_tried_once_more() {
    let fixture = Fixture::new();
    let (store, mut worker) = direct_worker(&fixture);
    let (sender, _events) = unbounded();
    let on = PageTextSetting {
        on: true,
        excluded: Vec::new(),
    };
    worker
        .handle(&store, SessionCommand::SavePageTextSetting(on), &sender)
        .expect("switching capture on is saved");
    let lock = hold_write_lock(&fixture);
    let off = PageTextSetting::default();
    worker
        .handle(&store, SessionCommand::SavePageTextSetting(off), &sender)
        .expect_err("a busy database refuses the switch-off");
    release_write_lock(lock);

    worker
        .finish_serving(&store, &sender)
        .expect("shutdown saves the held switch-off");
    assert!(!stored_capture_on(&store));
    close_direct(fixture, store, worker);
}

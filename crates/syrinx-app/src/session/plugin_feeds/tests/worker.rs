//! The session loop's side: typing never moves a pull, unread events are reported, and a
//! never-ending feed does not hold the idle work off.

use super::*;
use crate::session::page_text::CapturedPage;
use crate::session::{BACKUP_CHECK_INTERVAL, SessionError, SessionEvent, utc_seconds};

/// A session worker over a synthetic store with the `notes` plugin installed.
fn session_worker(folder: &Folder) -> (Store, Worker) {
    let lease = Arc::new(
        WindowsDirectoryLease::prepare(&folder.0.join("State")).expect("a synthetic lease"),
    );
    let journal = DeletionJournal::open(&lease).expect("a synthetic journal");
    let page_text = page_text::PageText::open(&lease);
    let store = Store::open(lease).expect("a synthetic store");
    store
        .save_plugin_install(install())
        .expect("a synthetic install");
    let worker = Worker::restore(&store, journal, page_text).expect("a synthetic worker");
    (store, worker)
}

/// The ids of kept pages whose text holds `word`.
fn pages_with(worker: &Worker, word: &str) -> Vec<String> {
    let request = SearchRequest {
        text: word.to_owned(),
        prefix_last: false,
        namespaces: NamespaceFilter::Only(vec![Namespace::page()]),
        limit: 100,
    };
    let index = worker.page_text.index().expect("an open index");
    let hits = index.search(&request).expect("a full-text query");
    hits.into_iter().map(|hit| hit.id).collect()
}

#[test]
fn events_feeds_do_not_read_are_handed_back_whole() {
    let mut feeds = PluginFeeds::default();
    let stopped = PluginEvent::Deactivated {
        plugin: PLUGIN.to_owned(),
        reason: syrinx_plugin_host::worker::StopReason::Shutdown,
    };
    let unread = feeds.notice([feed_changed(), stopped]);
    let described: Vec<String> = unread.iter().map(ToString::to_string).collect();
    assert_eq!(described, ["plugin notes: the plugin stopped"]);
    assert!(feeds.busy(true));
}

#[test]
fn a_plugin_failure_reaches_the_window_by_plugin_and_kind_only() {
    let folder = Folder::new();
    let (store, mut worker) = session_worker(&folder);
    let (sender, mut events) = unbounded();
    let mut plugin = Scripted::default();
    plugin.events.push(PluginEvent::PrepareFailed {
        plugin: PLUGIN.to_owned(),
        error: syrinx_plugin_host::HostError::Unavailable("synthetic detail".to_owned()),
    });
    worker
        .pump_feeds_from(&mut plugin, &store, &sender)
        .expect("a reported failure");
    let Ok(Some(SessionEvent::Failure { error, fatal })) = events.try_recv().map(Some) else {
        panic!("the failure reaches the window");
    };
    assert!(!fatal);
    let SessionError::PluginEvent(text) = error else {
        panic!("a plugin event failure");
    };
    assert_eq!(text, "plugin notes: compiling it in the background failed");
    drop(worker);
    store.shutdown().expect("the store closes");
}

#[test]
fn typing_in_the_palette_never_starts_or_moves_a_pull() {
    let folder = Folder::new();
    let (store, mut worker) = session_worker(&folder);
    let (sender, _events) = unbounded();
    let mut plugin = Scripted::default().answer(None, first_batch());
    let unread = worker.plugin_feeds.notice([feed_changed()]);
    assert!(unread.is_empty(), "a feed waits to be pulled");

    // The owner types in the palette; the loop then does what it does after any command,
    // passing `is_typing` of the command as its command arm does.
    for typed in ["s", "sy", "syn", "synthetic"] {
        let query = SessionCommand::Query {
            request_id: 1,
            text: typed.to_owned(),
            scope: Scope::Everything,
            policy: policy(),
        };
        let typing = is_typing(&query);
        assert!(typing, "a palette query is typing");
        worker.handle(&store, query, &sender).expect("a query");
        worker
            .pull_after_command_from(typing, &mut plugin, &store, &sender)
            .expect("nothing to do");
    }
    assert!(plugin.requests.is_empty(), "no plugin call while typing");
    let paused = worker.plugin_feeds.typing_quiet_for(Instant::now());
    assert!(
        paused.is_some(),
        "the loop's own wake-ups wait for typing to pause"
    );

    // Another command straight after typing still waits for the pause.
    worker
        .pull_after_command_from(false, &mut plugin, &store, &sender)
        .expect("nothing yet");
    assert!(
        plugin.requests.is_empty(),
        "the pause holds for every command"
    );

    // Once the pause has passed, any other command moves the waiting pull on.
    let paused_at = Instant::now()
        .checked_sub(TYPING_PAUSE)
        .expect("a clock past the pause");
    worker.plugin_feeds.note_typing(paused_at);
    worker
        .pull_after_command_from(false, &mut plugin, &store, &sender)
        .expect("a pull");
    assert_eq!(plugin.requests, [None]);
    drop(worker);
    store.shutdown().expect("the store closes");
}

#[test]
fn a_never_ending_feed_does_not_stop_page_text_expiry() {
    let folder = Folder::new();
    let (store, mut worker) = session_worker(&folder);
    let (sender, _events) = unbounded();
    let a_month_ago = utc_seconds().expect("a synthetic clock") - 30 * 24 * 60 * 60;
    let page = CapturedPage {
        address: "https://example.com/old".to_owned(),
        title: "Synthetic old page".to_owned(),
        text: "A made-up paragraph about marrowbell.".to_owned(),
        visited_at: a_month_ago,
    };
    worker.page_text.store(page, a_month_ago);
    // Start-up already expired once today; the next expiry is made due straight away.
    worker.page_text.expiry_due_for_test();
    assert_eq!(pages_with(&worker, "marrowbell").len(), 1);
    let mut plugin = Endless::default();
    assert!(worker.plugin_feeds.notice([feed_changed()]).is_empty());

    // `pull_while_busy_from` stands in for the loop's busy arm, which calls `pull_while_busy`
    // with the real runtime and clock. Its wake-ups pull on and on; nothing expires yet.
    let start = Instant::now();
    for _ in 0..3 * BATCHES_PER_TRIGGER {
        worker
            .pull_while_busy_from(&mut plugin, &store, &sender, start)
            .expect("a pull");
    }
    assert!(plugin.requests > BATCHES_PER_TRIGGER as usize);
    assert!(worker.plugin_feeds.busy(true), "the feed never ends");
    assert_eq!(pages_with(&worker, "marrowbell").len(), 1);

    // Once the idle work has waited its delay behind pulling, it runs.
    worker
        .pull_while_busy_from(&mut plugin, &store, &sender, start + BACKUP_CHECK_INTERVAL)
        .expect("a pull and the idle work");
    assert!(pages_with(&worker, "marrowbell").is_empty());
    assert!(worker.plugin_feeds.busy(true), "pulling goes on");
    drop(worker);
    store.shutdown().expect("the store closes");
}

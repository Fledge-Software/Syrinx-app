//! Proves the plugin feed pull loop (#141) against a scripted plugin: replay and a crash
//! before the cursor is kept lose nothing, an emptied index is fed again from the start,
//! removal empties both tiers, and nothing calls a plugin while the owner types (D9).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use syrinx_plugin_host::bindings::syrinx::plugin::types::Icon;
use syrinx_search::fulltext::{NamespaceFilter, SearchRequest};
use syrinx_search::{BlankQuery, BoostWeights, Scope, SearchPolicy};
use syrinx_store::{DirectoryLease, LocationError, PluginInstall, PluginState};

use super::*;

const PLUGIN: &str = "notes";
const FEED: &str = "notes.items";
/// Unix seconds for every capture in these tests.
const NOW: i64 = 1_900_000_000;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A fresh folder holding a synthetic database and index folders, removed afterwards.
struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        let name = format!(
            "syrinx-plugin-feeds-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let folder = std::env::temp_dir().join(name);
        fs::create_dir(&folder).expect("a fresh synthetic folder");
        Self(folder)
    }

    fn store(&self) -> Store {
        let lease = Arc::new(Lease(self.0.join("syrinx.db")));
        let store = Store::open(lease).expect("a synthetic store");
        store
            .save_plugin_install(install())
            .expect("a synthetic install");
        store
    }

    /// The full-text index in `name`, created empty the first time.
    fn index(&self, name: &str) -> FullText {
        let folder = self.0.join(name);
        fs::create_dir_all(&folder).expect("a synthetic index folder");
        FullText::open(&folder).expect("a synthetic index")
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        // Only the folder this test created is removed.
        if fs::remove_dir_all(&self.0).is_err() {
            eprintln!("a synthetic plugin feed folder was left behind");
        }
    }
}

struct Lease(PathBuf);

impl DirectoryLease for Lease {
    fn database_path(&self) -> &Path {
        &self.0
    }

    fn revalidate(&self) -> Result<(), LocationError> {
        Ok(())
    }
}

fn install() -> PluginInstall {
    PluginInstall {
        plugin_id: PLUGIN.to_owned(),
        version: "0.1.0".to_owned(),
        module_sha256: [1; 32],
        manifest_sha256: [2; 32],
        signer_key: "synthetic-test-key".to_owned(),
        state: PluginState::Enabled,
        installed_at: 10,
        updated_at: 10,
    }
}

/// A batch already answered.
struct Ready(Batch);

impl BatchAnswer for Ready {
    fn try_answer(&self) -> Option<CallResult<Result<Batch, PluginError>>> {
        Some(Ok(Ok(self.0.clone())))
    }
}

/// A plugin that answers each cursor with a scripted batch, and records every request.
#[derive(Default)]
struct Scripted {
    batches: BTreeMap<Option<String>, Batch>,
    requests: Vec<Option<String>>,
    /// Counts every call, shared with checks made while this plugin is borrowed elsewhere.
    calls: Rc<Cell<usize>>,
}

impl Scripted {
    fn answer(mut self, after: Option<&str>, batch: Batch) -> Self {
        self.batches.insert(after.map(str::to_owned), batch);
        self
    }
}

impl FeedSource for Scripted {
    fn request(
        &mut self,
        plugin: &str,
        feed: &str,
        after: Option<String>,
    ) -> CallResult<Box<dyn BatchAnswer>> {
        assert_eq!((plugin, feed), (PLUGIN, FEED));
        self.calls.set(self.calls.get() + 1);
        self.requests.push(after.clone());
        let batch = self.batches.get(&after).expect("a scripted batch").clone();
        Ok(Box::new(Ready(batch)))
    }
}

fn entry(id: &str, revision: u64, title: &str) -> Change {
    Change::UpsertEntry(CatalogueEntry {
        id: id.to_owned(),
        revision,
        title: title.to_owned(),
        subtitle: "A made-up note".to_owned(),
        icon: Icon::Document,
        last_used: Some(1_900_000_000_000),
        keywords: vec!["synthetic".to_owned()],
    })
}

fn document(id: &str, revision: u64, word: &str) -> Change {
    Change::UpsertDocument(Document {
        id: id.to_owned(),
        revision,
        title: format!("Synthetic note {id}"),
        body: format!("A made-up paragraph about {word}."),
    })
}

fn batch(changes: Vec<Change>, cursor: &str, more: bool) -> Batch {
    Batch {
        changes,
        next_cursor: cursor.to_owned(),
        more,
    }
}

fn announce(feeds: &mut PluginFeeds) {
    feeds.notice([PluginEvent::FeedChanged {
        plugin: PLUGIN.to_owned(),
        feed: FEED.to_owned(),
    }]);
}

/// Pumps until no pull is in flight and nothing waits.
fn settle(
    feeds: &mut PluginFeeds,
    source: &mut Scripted,
    store: &Store,
    index: &mut FullText,
    catalogue: &mut Catalogue,
) {
    for _ in 0..64 {
        if !feeds.busy(true) {
            return;
        }
        let mut sinks = Sinks {
            store,
            index: Some(&mut *index),
            catalogue: &mut *catalogue,
            now: NOW,
        };
        feeds.pump(source, &mut sinks).expect("a pull");
    }
    panic!("pulling never settled");
}

fn policy() -> SearchPolicy {
    SearchPolicy {
        limit: 50,
        max_typos: 0,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 0,
            frequency: 0,
            recency: 0,
            recency_window_secs: NonZeroU64::new(100).expect("nonzero window"),
        },
    }
}

/// The catalogue's plugin rows matching `text`, by title, sorted.
fn titles(catalogue: &mut Catalogue, text: &str) -> Vec<String> {
    let results = catalogue
        .query(text, Scope::Everything, 100, policy())
        .expect("a catalogue query");
    let mut titles: Vec<String> = results
        .hits
        .into_iter()
        .filter(|hit| matches!(hit.target, Target::Plugin { .. }))
        .map(|hit| hit.title)
        .collect();
    titles.sort();
    titles
}

/// The ids of plugin documents in the index matching `word`, sorted.
fn documents(index: &FullText, word: &str) -> Vec<String> {
    let request = SearchRequest {
        text: word.to_owned(),
        prefix_last: false,
        namespaces: NamespaceFilter::All,
        limit: 100,
    };
    let mut ids: Vec<String> = index
        .search(&request)
        .expect("a full-text query")
        .into_iter()
        .map(|hit| hit.id)
        .collect();
    ids.sort();
    ids
}

fn first_batch() -> Batch {
    batch(
        vec![
            entry("a", 1, "Synthetic alpha"),
            entry("b", 1, "Synthetic bravo"),
            document("a", 1, "quillmark"),
        ],
        "c1",
        false,
    )
}

#[test]
fn an_announced_feed_reaches_both_tiers_and_a_replay_changes_nothing() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch()).answer(
        Some("c1"),
        batch(vec![entry("b", 2, "Synthetic bravo two")], "c2", false),
    );

    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
    );
    assert_eq!(documents(&index, "quillmark"), ["a"]);
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c1".to_owned()))
    );

    // A replay of the first batch, as after a crash before its cursor was kept, is a no-op.
    let mut replay = Scripted::default().answer(None, first_batch());
    feeds.pull = None;
    feeds.waiting.clear();
    let name = FeedName {
        plugin: PLUGIN.to_owned(),
        feed: FEED.to_owned(),
    };
    let pull = Pull {
        name,
        from_start: false,
        answer: Box::new(Ready(first_batch())),
        batches: 0,
    };
    feeds.pull = Some(pull);
    settle(&mut feeds, &mut replay, &store, &mut index, &mut catalogue);
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
    );
    assert_eq!(documents(&index, "quillmark"), ["a"]);

    // The next announcement goes on from the kept cursor.
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(plugin.requests, [None, Some("c1".to_owned())]);
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo two"]
    );
}

#[test]
fn a_crash_between_the_index_commit_and_the_cursor_replays_and_loses_nothing() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);

    // The second batch reaches the index, then the session stops before the store commit.
    let second = batch(
        vec![
            entry("c", 1, "Synthetic charlie"),
            document("c", 1, "copperleaf"),
            Change::Delete("a".to_owned()),
        ],
        "c2",
        false,
    );
    let namespace = Namespace::plugin_feed(PLUGIN, FEED).expect("a namespace");
    keep_documents(&mut index, &namespace, &second.changes, false, NOW).expect("the index part");
    drop(index);
    store.shutdown().expect("the store closes");

    // A restart shows the kept rows without running the plugin, then pulls from the old cursor.
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::restore(&store, &mut catalogue).expect("restored rows");
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
    );
    let mut plugin = Scripted::default().answer(Some("c1"), second);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(plugin.requests, [Some("c1".to_owned())]);
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic bravo", "Synthetic charlie"]
    );
    assert_eq!(documents(&index, "copperleaf"), ["c"]);
    assert!(documents(&index, "quillmark").is_empty());
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c2".to_owned()))
    );
}

#[test]
fn an_emptied_index_is_fed_again_from_the_start_dropping_old_rows_first() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let refed = batch(
        vec![
            entry("b", 1, "Synthetic bravo"),
            document("b", 1, "lanternfish"),
        ],
        "r1",
        false,
    );
    let mut plugin = Scripted::default()
        .answer(None, first_batch())
        .answer(Some("c1"), batch(Vec::new(), "c1", false));
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);

    // A history repair leaves a fresh index folder: the cursor no longer describes it.
    let mut emptied = folder.index("fulltext-emptied");
    plugin = plugin.answer(None, refed);
    announce(&mut feeds);
    settle(
        &mut feeds,
        &mut plugin,
        &store,
        &mut emptied,
        &mut catalogue,
    );
    assert_eq!(plugin.requests, [None, None]);
    assert_eq!(titles(&mut catalogue, "synthetic"), ["Synthetic bravo"]);
    assert_eq!(documents(&emptied, "lanternfish"), ["b"]);
    let kept: Vec<String> = store
        .plugin_feed_entries()
        .expect("kept rows")
        .into_iter()
        .map(|stored| stored.entry.entry_id)
        .collect();
    assert_eq!(kept, ["b"]);

    // The index is marked again, so the next announcement goes on from the new cursor.
    announce(&mut feeds);
    plugin = plugin.answer(Some("r1"), batch(Vec::new(), "r1", false));
    settle(
        &mut feeds,
        &mut plugin,
        &store,
        &mut emptied,
        &mut catalogue,
    );
    assert_eq!(plugin.requests.last(), Some(&Some("r1".to_owned())));
    drop(index);
}

#[test]
fn a_reset_in_a_batch_drops_what_came_before_it_in_both_tiers() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let after_reset = batch(
        vec![
            entry("z", 1, "Synthetic zulu, never kept"),
            Change::Reset,
            entry("c", 1, "Synthetic charlie"),
        ],
        "c2",
        false,
    );
    let mut plugin = Scripted::default()
        .answer(None, first_batch())
        .answer(Some("c1"), after_reset);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(titles(&mut catalogue, "synthetic"), ["Synthetic charlie"]);
    assert!(documents(&index, "quillmark").is_empty());
}

#[test]
fn a_long_feed_pulls_a_bounded_number_of_batches_per_announcement() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(
        None,
        batch(vec![entry("p0", 1, "Synthetic page 0")], "c1", true),
    );
    for page in 1..=BATCHES_PER_TRIGGER + 2 {
        let changes = vec![entry(
            &format!("p{page}"),
            1,
            &format!("Synthetic page {page}"),
        )];
        let cursor = format!("c{}", page + 1);
        plugin = plugin.answer(Some(&format!("c{page}")), batch(changes, &cursor, true));
    }
    announce(&mut feeds);
    let mut sinks = Sinks {
        store: &store,
        index: Some(&mut index),
        catalogue: &mut catalogue,
        now: NOW,
    };
    // Two pumps per batch: one starts the pull, one applies it.
    for _ in 0..BATCHES_PER_TRIGGER + 1 {
        feeds.pump(&mut plugin, &mut sinks).expect("a pull");
    }
    assert_eq!(plugin.requests.len(), BATCHES_PER_TRIGGER as usize);
    assert!(feeds.pull.is_none(), "the budget is spent");
    assert_eq!(feeds.waiting.len(), 1, "the feed waits behind any other");
    feeds.pump(&mut plugin, &mut sinks).expect("the next turn");
    let resumed = plugin.requests.last().cloned().flatten();
    assert_eq!(resumed, Some(format!("c{BATCHES_PER_TRIGGER}")));
}

#[test]
fn removing_a_plugin_empties_both_tiers_and_its_cursor() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert!(!titles(&mut catalogue, "synthetic").is_empty());

    assert_eq!(store.remove_plugin(PLUGIN.to_owned()), Ok(true));
    feeds
        .forget_plugin(PLUGIN, Some(&mut index), &mut catalogue)
        .expect("forgotten");
    assert!(titles(&mut catalogue, "synthetic").is_empty());
    assert!(documents(&index, "quillmark").is_empty());
    assert_eq!(store.plugin_feed_cursor(PLUGIN, FEED), Ok(None));
    assert_eq!(store.plugin_feed_entries(), Ok(Vec::new()));
    assert_eq!(
        index.is_fed(&Namespace::plugin_feed(PLUGIN, FEED).expect("ns")),
        Ok(false)
    );
}

#[test]
fn typing_in_the_palette_never_calls_a_plugin() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    let calls = Rc::clone(&plugin.calls);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(calls.get(), 1);

    // Every prefix the owner types is answered from the catalogue and the index alone.
    for typed in ["s", "sy", "syn", "synthetic", "synthetic al"] {
        assert!(
            catalogue
                .query(typed, Scope::Everything, 100, policy())
                .is_ok()
        );
        assert!(documents(&index, "quillmark").len() <= 1);
    }
    assert_eq!(calls.get(), 1, "no pull without an announcement");
    // An announcement alone queues; only the session's pump calls the plugin.
    announce(&mut feeds);
    assert_eq!(calls.get(), 1);
}

#[test]
fn page_text_expiry_leaves_a_pulled_plugin_document() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    let a_year_on = NOW + 365 * 24 * 60 * 60;
    assert_eq!(index.expire(a_year_on), Ok(0));
    assert_eq!(documents(&index, "quillmark"), ["a"]);
}

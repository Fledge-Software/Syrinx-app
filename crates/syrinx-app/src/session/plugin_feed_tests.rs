//! Proves the plugin feed pull loop (#141) against a scripted plugin: replay and a failure
//! before the cursor is kept lose nothing, an emptied index is fed again from the start,
//! removal empties both tiers, and page-text expiry leaves plugin documents (D40).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use syrinx_plugin_host::bindings::exports::syrinx::plugin::search_feed::{
    CatalogueEntry, Change, Document,
};
use syrinx_plugin_host::bindings::syrinx::plugin::types::Icon;
use syrinx_search::fulltext::{self, FullText, NamespaceFilter, Outcome, SearchRequest};
use syrinx_search::{BlankQuery, BoostWeights, Scope, SearchPolicy, Target};
use syrinx_store::{DirectoryLease, LocationError, PluginInstall, PluginState};

use super::*;
use crate::session::{
    DeletionJournal, SessionCommand, WindowsDirectoryLease, Worker, page_text, unbounded,
};

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
    /// Events the runtime reports at the next look.
    events: Vec<PluginEvent>,
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
    fn take_events(&mut self) -> Vec<PluginEvent> {
        std::mem::take(&mut self.events)
    }

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

/// The store, failing its next feed commits as a crash before the commit would.
struct FailingStore<'a> {
    store: &'a Store,
    failures: Cell<u32>,
}

impl FeedStore for FailingStore<'_> {
    fn cursor(&self, plugin: &str, feed: &str) -> Result<Option<String>, StoreError> {
        self.store.cursor(plugin, feed)
    }

    fn commit(&self, commit: PluginFeedCommit) -> Result<Vec<FeedRowOutcome>, StoreError> {
        if self.failures.get() > 0 {
            self.failures.set(self.failures.get() - 1);
            return Err(StoreError::Busy);
        }
        FeedStore::commit(self.store, commit)
    }

    fn installed(&self) -> Result<BTreeSet<String>, StoreError> {
        self.store.installed()
    }
}

/// The index, failing its next commit as a crash before the commit would.
struct FailingIndex<'a> {
    index: &'a mut FullText,
    failures: u32,
}

impl FeedIndex for FailingIndex<'_> {
    fn reset(&mut self, namespace: &Namespace) -> Result<(), FullTextError> {
        FeedIndex::reset(&mut *self.index, namespace)
    }

    fn stage(
        &mut self,
        changes: &[fulltext::Change],
        now: i64,
    ) -> Result<Vec<Outcome>, FullTextError> {
        FeedIndex::stage(&mut *self.index, changes, now)
    }

    fn held_revision(&self, namespace: &Namespace, id: &str) -> Result<Option<u64>, FullTextError> {
        FeedIndex::held_revision(&*self.index, namespace, id)
    }

    fn mark_fed(&mut self, namespace: &Namespace) -> Result<(), FullTextError> {
        FeedIndex::mark_fed(&mut *self.index, namespace)
    }

    fn is_fed(&self, namespace: &Namespace) -> Result<bool, FullTextError> {
        FeedIndex::is_fed(&*self.index, namespace)
    }

    fn commit(&mut self) -> Result<(), FullTextError> {
        if self.failures > 0 {
            self.failures -= 1;
            // What a crash before the commit leaves: nothing staged survives.
            self.index.discard()?;
            return Err(FullTextError::Io);
        }
        FeedIndex::commit(&mut *self.index)
    }

    fn plugin_ids(&self) -> Result<BTreeSet<String>, FullTextError> {
        FeedIndex::plugin_ids(&*self.index)
    }

    fn remove_plugin(&mut self, plugin: &str) -> Result<(), FullTextError> {
        FeedIndex::remove_plugin(&mut *self.index, plugin)
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

fn feed_changed() -> PluginEvent {
    PluginEvent::FeedChanged {
        plugin: PLUGIN.to_owned(),
        feed: FEED.to_owned(),
    }
}

fn announce(feeds: &mut PluginFeeds) {
    let unread = feeds.notice([feed_changed()]);
    assert!(unread.is_empty());
}

/// Pumps with `store` and `index` until no pull is in flight and nothing waits.
fn settle_with(
    feeds: &mut PluginFeeds,
    source: &mut Scripted,
    store: &dyn FeedStore,
    index: &mut dyn FeedIndex,
    catalogue: &mut Catalogue,
) -> Result<(), FeedError> {
    for _ in 0..64 {
        if !feeds.busy(true) {
            return Ok(());
        }
        let mut sinks = Sinks {
            store,
            index: Some(&mut *index),
            catalogue: &mut *catalogue,
            now: NOW,
        };
        feeds.pump(source, &mut sinks)?;
    }
    panic!("pulling never settled");
}

fn settle(
    feeds: &mut PluginFeeds,
    source: &mut Scripted,
    store: &Store,
    index: &mut FullText,
    catalogue: &mut Catalogue,
) {
    settle_with(feeds, source, store, index, catalogue).expect("a pull");
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

/// The store's kept rows as (id, revision).
fn kept_rows(store: &Store) -> Vec<(String, u64)> {
    store
        .plugin_feed_entries()
        .expect("kept rows")
        .into_iter()
        .map(|stored| (stored.entry.entry_id, stored.entry.revision))
        .collect()
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
    let second = batch(vec![entry("b", 2, "Synthetic bravo two")], "c2", false);
    let mut plugin = Scripted::default()
        .answer(None, first_batch())
        .answer(Some("c1"), second);

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

    // The first batch again, answered from its own cursor as a replay would be, is a no-op.
    let name = FeedName {
        plugin: PLUGIN.to_owned(),
        feed: FEED.to_owned(),
    };
    feeds.pull = Some(Pull {
        name,
        from_start: false,
        answer: Box::new(Ready(first_batch())),
        batches: 0,
    });
    let mut replay = Scripted::default();
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
fn an_older_revision_after_a_newer_one_leaves_the_newer_row_showing() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let newer = batch(vec![entry("a", 2, "Synthetic newer")], "c1", false);
    let older = batch(vec![entry("a", 1, "Synthetic older")], "c2", false);
    let mut plugin = Scripted::default()
        .answer(None, newer)
        .answer(Some("c1"), older);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(titles(&mut catalogue, "synthetic"), ["Synthetic newer"]);
    assert_eq!(kept_rows(&store), [("a".to_owned(), 2)]);
}

#[test]
fn a_failed_index_commit_keeps_the_old_cursor_and_the_next_pull_completes_both_tiers() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let second = batch(
        vec![
            entry("c", 1, "Synthetic charlie"),
            document("c", 1, "copperleaf"),
            Change::Delete("a".to_owned()),
        ],
        "c2",
        false,
    );
    let mut plugin = Scripted::default()
        .answer(None, first_batch())
        .answer(Some("c1"), second);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);

    // The second batch reaches apply, and the index commit fails.
    announce(&mut feeds);
    let mut failing = FailingIndex {
        index: &mut index,
        failures: 1,
    };
    let failed = settle_with(
        &mut feeds,
        &mut plugin,
        &store,
        &mut failing,
        &mut catalogue,
    );
    assert_eq!(failed, Err(FeedError::Index(FullTextError::Io)));
    // Nothing of the batch was kept: the store would be ahead of the index otherwise.
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c1".to_owned()))
    );
    assert_eq!(
        kept_rows(&store),
        [("a".to_owned(), 1), ("b".to_owned(), 1)]
    );

    // A restart shows the kept rows without running the plugin, then pulls from c1 again.
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::restore(&store, &mut catalogue).expect("restored rows");
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
    );
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(plugin.requests.last(), Some(&Some("c1".to_owned())));
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
fn a_failed_store_commit_on_a_refeed_from_the_start_pulls_from_the_start_again() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let first = batch(
        vec![
            entry("a", 1, "Synthetic alpha"),
            document("a", 1, "quillmark"),
        ],
        "c1",
        true,
    );
    let second = batch(
        vec![
            entry("b", 1, "Synthetic bravo"),
            document("b", 1, "lanternfish"),
        ],
        "c2",
        false,
    );
    let mut plugin = Scripted::default()
        .answer(None, first)
        .answer(Some("c1"), second)
        .answer(Some("c2"), batch(Vec::new(), "c2", false));
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c2".to_owned()))
    );

    // A history repair empties the index; the refeed's first batch reaches the index, then
    // the store commit fails, as a crash between the two would leave it.
    let mut emptied = folder.index("fulltext-emptied");
    let failing = FailingStore {
        store: &store,
        failures: Cell::new(1),
    };
    announce(&mut feeds);
    let failed = settle_with(
        &mut feeds,
        &mut plugin,
        &failing,
        &mut emptied,
        &mut catalogue,
    );
    assert_eq!(failed, Err(FeedError::Store(StoreError::Busy)));

    // The cursor is still the old feed's end, but the index is not marked as fed, so the next
    // pull starts again from the start instead of going on from c2 and losing batch two.
    announce(&mut feeds);
    settle(
        &mut feeds,
        &mut plugin,
        &store,
        &mut emptied,
        &mut catalogue,
    );
    let refed: Vec<Option<String>> = plugin.requests[2..].to_vec();
    assert_eq!(refed, [None, None, Some("c1".to_owned())]);
    assert_eq!(documents(&emptied, "quillmark"), ["a"]);
    assert_eq!(documents(&emptied, "lanternfish"), ["b"]);
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
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
    let mut plugin = Scripted::default().answer(None, first_batch());
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
    assert_eq!(kept_rows(&store), [("b".to_owned(), 1)]);

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
fn a_batch_either_tier_would_refuse_writes_to_neither() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    // The document is fine; the deletion names the index's reserved marker id, which the
    // index alone would refuse, and only after the document was written.
    let refused = batch(
        vec![
            document("d", 1, "copperleaf"),
            Change::Delete("\0fed".to_owned()),
        ],
        "c1",
        false,
    );
    let mut plugin = Scripted::default().answer(None, refused);
    announce(&mut feeds);
    let failed = settle_with(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert!(failed.is_err());
    assert!(documents(&index, "copperleaf").is_empty());
    assert_eq!(store.plugin_feed_cursor(PLUGIN, FEED), Ok(None));
}

#[test]
fn a_long_feed_pulls_a_bounded_number_of_batches_per_announcement() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let first = batch(vec![entry("p0", 1, "Synthetic page 0")], "c1", true);
    let mut plugin = Scripted::default().answer(None, first);
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
    // One pump starts the pull, then one applies each batch.
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
fn a_plugin_removed_mid_session_leaves_both_tiers_at_the_next_pump() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert!(!titles(&mut catalogue, "synthetic").is_empty());

    // The runtime removed it and the store cascaded; the next pump hears it.
    assert_eq!(store.remove_plugin(PLUGIN.to_owned()), Ok(true));
    plugin.events.push(PluginEvent::Removed {
        plugin: PLUGIN.to_owned(),
    });
    let unread = feeds.notice(plugin.take_events());
    assert!(unread.is_empty());
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert!(titles(&mut catalogue, "synthetic").is_empty());
    assert!(documents(&index, "quillmark").is_empty());
    assert_eq!(store.plugin_feed_cursor(PLUGIN, FEED), Ok(None));
    assert_eq!(store.plugin_feed_entries(), Ok(Vec::new()));
    let namespace = Namespace::plugin_feed(PLUGIN, FEED).expect("a namespace");
    assert_eq!(index.is_fed(&namespace), Ok(false));
}

#[test]
fn text_left_by_a_plugin_removed_while_the_index_was_closed_goes_at_the_next_pull() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);

    // Removed while the index is closed: the rows go, and the text is reported as left.
    assert_eq!(store.remove_plugin(PLUGIN.to_owned()), Ok(true));
    let closed = feeds.forget_plugin(PLUGIN, None, &mut catalogue);
    assert_eq!(closed, Err(FeedError::Index(FullTextError::NeedsReopen)));
    assert!(titles(&mut catalogue, "synthetic").is_empty());
    assert_eq!(documents(&index, "quillmark"), ["a"]);

    // A new session's first pump, with the index open, finds and removes it.
    let mut feeds = PluginFeeds::restore(&store, &mut catalogue).expect("nothing kept");
    let mut sinks = Sinks {
        store: &store,
        index: Some(&mut index),
        catalogue: &mut catalogue,
        now: NOW,
    };
    feeds.pump(&mut plugin, &mut sinks).expect("the sweep");
    assert!(documents(&index, "quillmark").is_empty());
}

#[test]
fn events_feeds_do_not_read_are_named_by_plugin_and_kind() {
    let mut feeds = PluginFeeds::default();
    let unread = feeds.notice([
        feed_changed(),
        PluginEvent::Deactivated {
            plugin: PLUGIN.to_owned(),
            reason: syrinx_plugin_host::worker::StopReason::Shutdown,
        },
    ]);
    assert_eq!(unread, ["notes: the plugin stopped"]);
    assert!(feeds.busy(true));
}

#[test]
fn typing_in_the_palette_never_starts_or_moves_a_pull() {
    let folder = Folder::new();
    let lease = Arc::new(
        WindowsDirectoryLease::prepare(&folder.0.join("State")).expect("a synthetic lease"),
    );
    let journal = DeletionJournal::open(&lease).expect("a synthetic journal");
    let page_text = page_text::PageText::open(&lease);
    let store = Store::open(lease).expect("a synthetic store");
    store
        .save_plugin_install(install())
        .expect("a synthetic install");
    let mut worker = Worker::restore(&store, journal, page_text).expect("a synthetic worker");
    let (sender, _events) = unbounded();
    let mut plugin = Scripted::default().answer(None, first_batch());
    let unread = worker.plugin_feeds.notice([feed_changed()]);
    assert!(unread.is_empty(), "a feed waits to be pulled");

    // The owner types in the palette; the loop then does what it does after any command.
    for typed in ["s", "sy", "syn", "synthetic"] {
        let query = SessionCommand::Query {
            request_id: 1,
            text: typed.to_owned(),
            scope: Scope::Everything,
            policy: policy(),
        };
        let typing = is_typing(&query);
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

    // Any other command moves the waiting pull on.
    worker
        .pull_after_command_from(false, &mut plugin, &store, &sender)
        .expect("a pull");
    assert_eq!(plugin.requests, [None]);
    drop(worker);
    store.shutdown().expect("the store closes");
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

//! Proves the plugin feed pull loop (#141) against a scripted plugin, by concern: `pull`
//! for keeping batches in both tiers, `removal` for a removed plugin's rows and text, and
//! `worker` for the session loop's side. Shared fixtures are here; the fakes in `fakes`.

mod fakes;
mod pull;
mod removal;
mod worker;

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

use fakes::{Endless, FailingIndex, FailingStore, Ready, Scripted, Sequence};

const PLUGIN: &str = "notes";
const FEED: &str = "notes.items";
/// A second synthetic plugin and its feed.
const OTHER: &str = "other";
const OTHER_FEED: &str = "other.items";
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
    source: &mut dyn FeedSource,
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
            at: Instant::now(),
        };
        feeds.pump(source, &mut sinks)?;
    }
    panic!("pulling never settled");
}

fn settle(
    feeds: &mut PluginFeeds,
    source: &mut dyn FeedSource,
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

//! Pulls plugin search feeds into Syrinx's own search (#141): only when a feed is announced
//! as changed, never while the owner types (D9), and a feed's cursor moves only once the rows
//! it covers are kept in both tiers.

mod keys;

use std::collections::{BTreeSet, VecDeque};
use std::fmt;
use std::time::Duration;

use syrinx_plugin_host::HostError;
use syrinx_plugin_host::bindings::exports::syrinx::plugin::search_feed::{
    Batch, CatalogueEntry, Change, Document,
};
use syrinx_plugin_host::bindings::syrinx::plugin::types::PluginError;
use syrinx_plugin_host::owner::OwnerRuntime;
use syrinx_plugin_host::session::CallResult;
use syrinx_plugin_host::worker::{Pending, PluginEvent};
use syrinx_search::fulltext::{
    self, Deletion, FullText, FullTextDocument, FullTextError, Namespace,
};
use syrinx_search::{Catalogue, Entry, FeedKey, SearchError, Target, Usage};
use syrinx_store::{
    FeedRowChange, FeedRowOutcome, PluginFeedCommit, PluginFeedEntry, Store, StoreError,
    StoredFeedEntry,
};

use futures::channel::mpsc::UnboundedSender;
use keys::FeedKeys;

use super::{SessionError, SessionEvent};

/// The most changes one pull asks a plugin for; the plugin contract allows 512.
pub(super) const BATCH_CHANGES: u32 = 256;
/// The most batches one announcement pulls before the feed waits behind every other one.
pub(super) const BATCHES_PER_TRIGGER: u32 = 8;

/// Why one pull, or one feed's removal, did not finish. None carries a plugin's text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedError {
    /// The plugin id or feed name cannot name a full-text namespace.
    Name,
    /// Every catalogue key this session could intern is taken.
    KeysExhausted,
    Host(HostError),
    /// The plugin answered with an error; its text is the plugin's and is not kept.
    Plugin,
    Store(StoreError),
    Index(FullTextError),
    Search(SearchError),
}

impl fmt::Display for FeedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name => formatter.write_str("a plugin feed's name cannot be indexed"),
            Self::KeysExhausted => formatter.write_str("too many plugin feed rows this session"),
            Self::Host(error) => write!(formatter, "{error}"),
            Self::Plugin => formatter.write_str("the plugin refused to send its feed"),
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Index(error) => write!(formatter, "{error}"),
            Self::Search(error) => write!(formatter, "{error}"),
        }
    }
}

/// The plugin's side of a pull, so tests can stand in for a running plugin.
pub(super) trait FeedSource {
    fn request(
        &mut self,
        plugin: &str,
        feed: &str,
        after: Option<String>,
    ) -> CallResult<Box<dyn BatchAnswer>>;
}

/// A batch the plugin is still working on; never blocks.
pub(super) trait BatchAnswer {
    fn try_answer(&self) -> Option<CallResult<Result<Batch, PluginError>>>;
}

impl BatchAnswer for Pending<Result<Batch, PluginError>> {
    fn try_answer(&self) -> Option<CallResult<Result<Batch, PluginError>>> {
        Pending::try_answer(self)
    }
}

impl FeedSource for OwnerRuntime {
    fn request(
        &mut self,
        plugin: &str,
        feed: &str,
        after: Option<String>,
    ) -> CallResult<Box<dyn BatchAnswer>> {
        let pending = self.pull_feed(plugin, feed, after, BATCH_CHANGES)?;
        Ok(Box::new(pending))
    }
}

/// What a pull writes to: the store, the full-text index and the palette's catalogue.
pub(super) struct Sinks<'a> {
    pub(super) store: &'a Store,
    pub(super) index: Option<&'a mut FullText>,
    pub(super) catalogue: &'a mut Catalogue,
    /// Unix seconds; plugin documents keep this as their capture time.
    pub(super) now: i64,
}

/// One feed by name.
#[derive(Clone, Eq, PartialEq)]
struct FeedName {
    plugin: String,
    feed: String,
}

/// The one pull in flight.
struct Pull {
    name: FeedName,
    /// Pulled from the feed's start, so both tiers drop what they held of it first.
    from_start: bool,
    answer: Box<dyn BatchAnswer>,
    /// Batches applied for this announcement so far.
    batches: u32,
}

/// The session's plugin feeds: their interned rows, the feeds waiting to be pulled and the
/// pull in flight. Nothing here runs a plugin unless a feed was announced.
#[derive(Default)]
pub(super) struct PluginFeeds {
    keys: FeedKeys,
    waiting: VecDeque<FeedName>,
    pull: Option<Pull>,
    /// Whether this session has looked for text a removed plugin left in the index.
    swept: bool,
}

impl PluginFeeds {
    /// Puts every kept tier 1 row into the catalogue, so each plugin's entries are found
    /// before the plugin runs again; plugins start only when first used.
    pub(super) fn restore(store: &Store, catalogue: &mut Catalogue) -> Result<Self, FeedError> {
        let mut feeds = Self::default();
        for stored in store.plugin_feed_entries().map_err(FeedError::Store)? {
            feeds.show_stored(catalogue, &stored)?;
        }
        Ok(feeds)
    }

    /// Queues each feed announced as changed; another event is no concern of feeds.
    pub(super) fn notice(&mut self, events: impl IntoIterator<Item = PluginEvent>) {
        for event in events {
            let PluginEvent::FeedChanged { plugin, feed } = event else {
                continue;
            };
            self.announce(FeedName { plugin, feed });
        }
    }

    /// Whether a pull is in flight or a feed waits while the index can take it.
    pub(super) fn busy(&self, index_open: bool) -> bool {
        self.pull.is_some() || (index_open && !self.waiting.is_empty())
    }

    /// Moves pulling on by at most one batch: applies an answered one, or starts the next
    /// waiting feed. Never waits for a plugin.
    pub(super) fn pump(
        &mut self,
        source: &mut dyn FeedSource,
        sinks: &mut Sinks<'_>,
    ) -> Result<(), FeedError> {
        self.sweep_once(sinks)?;
        let Some(pull) = self.pull.take() else {
            return self.start_next(source, sinks);
        };
        let Some(answer) = pull.answer.try_answer() else {
            self.pull = Some(pull);
            return Ok(());
        };
        let batch = answer
            .map_err(FeedError::Host)?
            .map_err(|_| FeedError::Plugin)?;
        self.apply(&pull, &batch, sinks)?;
        self.follow(pull, batch, source)
    }

    /// Takes a removed plugin's rows out of the catalogue and its text out of the index; its
    /// store rows and cursors went with its install record. Safe when its manifest is gone.
    pub(super) fn forget_plugin(
        &mut self,
        plugin: &str,
        index: Option<&mut FullText>,
        catalogue: &mut Catalogue,
    ) -> Result<(), FeedError> {
        self.waiting.retain(|name| name.plugin != plugin);
        if self
            .pull
            .as_ref()
            .is_some_and(|pull| pull.name.plugin == plugin)
        {
            self.pull = None;
        }
        for target in self.keys.forget_plugin(plugin) {
            catalogue.remove(target).map_err(FeedError::Search)?;
        }
        // A closed index cannot be reached, so the owner hears that the text was left.
        let index = index.ok_or(FeedError::Index(FullTextError::NeedsReopen))?;
        index.remove_plugin(plugin).map_err(FeedError::Index)
    }

    /// Once per session, with the index open, removes the text of every plugin the store no
    /// longer records as installed: one removed while the index was closed left it there.
    fn sweep_once(&mut self, sinks: &mut Sinks<'_>) -> Result<(), FeedError> {
        let Some(index) = sinks.index.as_deref_mut() else {
            return Ok(());
        };
        if self.swept {
            return Ok(());
        }
        self.swept = true;
        let installs = sinks.store.plugin_installs().map_err(FeedError::Store)?;
        let installed: BTreeSet<String> = installs
            .into_iter()
            .map(|install| install.plugin_id)
            .collect();
        let held = index.plugin_ids().map_err(FeedError::Index)?;
        for plugin in held.difference(&installed) {
            self.forget_plugin(plugin, Some(&mut *index), sinks.catalogue)?;
        }
        Ok(())
    }

    /// Queues a feed once; one already waiting covers this announcement too.
    fn announce(&mut self, name: FeedName) {
        if !self.waiting.contains(&name) {
            self.waiting.push_back(name);
        }
    }

    /// Starts the first waiting feed from its kept cursor, or from its start when nothing
    /// was kept or the index no longer holds what was fed (an emptied index, #141's comment).
    fn start_next(
        &mut self,
        source: &mut dyn FeedSource,
        sinks: &mut Sinks<'_>,
    ) -> Result<(), FeedError> {
        let Some(index) = sinks.index.as_deref() else {
            return Ok(());
        };
        let Some(name) = self.waiting.pop_front() else {
            return Ok(());
        };
        let namespace = namespace(&name)?;
        let cursor = sinks
            .store
            .plugin_feed_cursor(&name.plugin, &name.feed)
            .map_err(FeedError::Store)?;
        let fed = index.is_fed(&namespace).map_err(FeedError::Index)?;
        let after = cursor.filter(|_| fed);
        let from_start = after.is_none();
        let answer = source
            .request(&name.plugin, &name.feed, after)
            .map_err(FeedError::Host)?;
        self.pull = Some(Pull {
            name,
            from_start,
            answer,
            batches: 0,
        });
        Ok(())
    }

    /// Asks for the next batch while the plugin has more and this announcement's budget
    /// lasts; past it, the feed waits behind every other one.
    fn follow(
        &mut self,
        pull: Pull,
        batch: Batch,
        source: &mut dyn FeedSource,
    ) -> Result<(), FeedError> {
        if !batch.more {
            return Ok(());
        }
        let batches = pull.batches + 1;
        if batches >= BATCHES_PER_TRIGGER {
            self.announce(pull.name);
            return Ok(());
        }
        let after = Some(batch.next_cursor);
        let answer = source
            .request(&pull.name.plugin, &pull.name.feed, after)
            .map_err(FeedError::Host)?;
        self.pull = Some(Pull {
            name: pull.name,
            from_start: false,
            answer,
            batches,
        });
        Ok(())
    }

    /// Keeps one batch: documents and the fed marker in the index first, then the rows and
    /// the cursor in one store transaction, then the catalogue as the store kept it. A crash
    /// anywhere before the store commit replays the batch from the old cursor, which both
    /// tiers take as a no-op where they already hold it.
    fn apply(
        &mut self,
        pull: &Pull,
        batch: &Batch,
        sinks: &mut Sinks<'_>,
    ) -> Result<(), FeedError> {
        let reset = batch
            .changes
            .iter()
            .rposition(|change| matches!(change, Change::Reset));
        // Everything before the last reset is erased by it, so only what follows is applied.
        let changes = reset.map_or(&batch.changes[..], |last| &batch.changes[last + 1..]);
        let clear_first = pull.from_start || reset.is_some();
        let index = sinks
            .index
            .as_deref_mut()
            .ok_or(FeedError::Index(FullTextError::NeedsReopen))?;
        keep_documents(
            index,
            &namespace(&pull.name)?,
            changes,
            clear_first,
            sinks.now,
        )?;
        let rows = row_changes(changes);
        let commit = PluginFeedCommit {
            plugin_id: pull.name.plugin.clone(),
            feed: pull.name.feed.clone(),
            clear_first,
            changes: rows.clone(),
            next_cursor: batch.next_cursor.clone(),
        };
        let outcomes = sinks
            .store
            .commit_plugin_feed(commit)
            .map_err(FeedError::Store)?;
        self.show_kept(sinks.catalogue, &pull.name, clear_first, &rows, &outcomes)
    }

    /// Mirrors a committed batch in the catalogue: a cleared feed's rows leave, and each row
    /// the store applied is shown or removed.
    fn show_kept(
        &mut self,
        catalogue: &mut Catalogue,
        name: &FeedName,
        clear_first: bool,
        rows: &[FeedRowChange],
        outcomes: &[FeedRowOutcome],
    ) -> Result<(), FeedError> {
        let feed = self
            .keys
            .feed(&name.plugin, &name.feed)
            .ok_or(FeedError::KeysExhausted)?;
        if clear_first {
            for target in self.keys.forget_entries(feed) {
                catalogue.remove(target).map_err(FeedError::Search)?;
            }
        }
        let applied = rows
            .iter()
            .zip(outcomes)
            .filter(|(_, outcome)| **outcome == FeedRowOutcome::Applied);
        for (row, _) in applied {
            match row {
                FeedRowChange::Upsert(entry) => self.show(catalogue, feed, entry)?,
                FeedRowChange::Delete(entry_id) => self.hide(catalogue, feed, entry_id)?,
            }
        }
        Ok(())
    }

    fn show_stored(
        &mut self,
        catalogue: &mut Catalogue,
        stored: &StoredFeedEntry,
    ) -> Result<(), FeedError> {
        let feed = self
            .keys
            .feed(&stored.plugin_id, &stored.feed)
            .ok_or(FeedError::KeysExhausted)?;
        self.show(catalogue, feed, &stored.entry)
    }

    fn show(
        &mut self,
        catalogue: &mut Catalogue,
        feed: FeedKey,
        entry: &PluginFeedEntry,
    ) -> Result<(), FeedError> {
        let target = self
            .keys
            .target(feed, &entry.entry_id)
            .ok_or(FeedError::KeysExhausted)?;
        catalogue
            .upsert(catalogue_entry(target, entry))
            .map_err(FeedError::Search)
    }

    fn hide(
        &mut self,
        catalogue: &mut Catalogue,
        feed: FeedKey,
        entry_id: &str,
    ) -> Result<(), FeedError> {
        let Some(target) = self.keys.existing(feed, entry_id) else {
            return Ok(());
        };
        self.keys.forget_entry(feed, entry_id);
        catalogue
            .remove(target)
            .map_err(FeedError::Search)
            .map(drop)
    }
}

/// How often the session looks for a plugin's answer while a feed is being pulled.
const ANSWER_POLL: Duration = Duration::from_millis(20);

/// The session loop's side of pulling: it hears announcements and moves pulls on between
/// the owner's commands, so a slow plugin never holds up browsing.
impl super::Worker {
    /// Hears the runtime's announcements and moves pulling on by one batch. A failed pull is
    /// shown to the owner and the loop goes on; the feed is pulled again when next announced.
    pub(super) fn pump_plugin_feeds(
        &mut self,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let Some(plugins) = self.plugins.as_mut() else {
            return Ok(());
        };
        self.survival.operation = "pulling plugin search feeds";
        // Other events have no reader in the session yet; plugin tabs (#29) will add one.
        self.plugin_feeds.notice(plugins.runtime.take_events());
        let now = super::utc_seconds()?;
        let mut sinks = Sinks {
            store,
            index: self.page_text.index_mut(),
            catalogue: &mut self.catalogue,
            now,
        };
        let pumped = self.plugin_feeds.pump(&mut plugins.runtime, &mut sinks);
        let Err(error) = pumped else {
            return Ok(());
        };
        super::emit_recoverable(events, SessionError::PluginFeed(error))
    }

    /// Whether a pull is under way, so the loop wakes soon to collect its answer.
    pub(super) fn plugin_feeds_busy(&mut self) -> bool {
        let index_open = self.page_text.index_mut().is_some();
        self.plugins.is_some() && self.plugin_feeds.busy(index_open)
    }

    /// The loop's wait, cut short while a pull is under way.
    pub(super) fn plugin_feed_wait(&mut self, wait: Duration) -> Duration {
        if self.plugin_feeds_busy() {
            return wait.min(ANSWER_POLL);
        }
        wait
    }
}

fn namespace(name: &FeedName) -> Result<Namespace, FeedError> {
    Namespace::plugin_feed(&name.plugin, &name.feed).map_err(|_| FeedError::Name)
}

/// Commits the batch's documents and deletions, and the fed marker, to the feed's own
/// namespace, which D40's page-text expiry never reads.
fn keep_documents(
    index: &mut FullText,
    namespace: &Namespace,
    changes: &[Change],
    clear_first: bool,
    now: i64,
) -> Result<(), FeedError> {
    if clear_first {
        index.reset(namespace).map_err(FeedError::Index)?;
    }
    for change in changes {
        // One at a time, so a deletion sees a document staged earlier in the same batch.
        let Some(staged) = document_change(index, namespace, change, now)? else {
            continue;
        };
        index.stage(&[staged], now).map_err(FeedError::Index)?;
    }
    index.mark_fed(namespace).map_err(FeedError::Index)?;
    index.commit().map_err(FeedError::Index)
}

/// The index's form of one change; `None` for what tier 2 does not hold.
fn document_change(
    index: &FullText,
    namespace: &Namespace,
    change: &Change,
    now: i64,
) -> Result<Option<fulltext::Change>, FeedError> {
    match change {
        Change::UpsertDocument(document) => Ok(Some(fulltext::Change::Upsert(full_text_document(
            namespace, document, now,
        )))),
        Change::Delete(id) => {
            // The plugin names no revision; the tombstone covers the one held, if any.
            let held = index
                .held_revision(namespace, id)
                .map_err(FeedError::Index)?;
            Ok(held.map(|revision| {
                fulltext::Change::Delete(Deletion {
                    namespace: namespace.clone(),
                    id: id.clone(),
                    revision,
                    deleted_at: now,
                })
            }))
        }
        Change::UpsertEntry(_) | Change::Reset => Ok(None),
    }
}

fn full_text_document(namespace: &Namespace, document: &Document, now: i64) -> FullTextDocument {
    FullTextDocument {
        namespace: namespace.clone(),
        id: document.id.clone(),
        revision: document.revision,
        captured_at: now,
        title: document.title.clone(),
        body: document.body.clone(),
    }
}

/// The batch's tier 1 changes; a deletion reaches both tiers.
fn row_changes(changes: &[Change]) -> Vec<FeedRowChange> {
    changes
        .iter()
        .filter_map(|change| match change {
            Change::UpsertEntry(entry) => Some(FeedRowChange::Upsert(stored_entry(entry))),
            Change::Delete(id) => Some(FeedRowChange::Delete(id.clone())),
            Change::UpsertDocument(_) | Change::Reset => None,
        })
        .collect()
}

fn stored_entry(entry: &CatalogueEntry) -> PluginFeedEntry {
    PluginFeedEntry {
        entry_id: entry.id.clone(),
        revision: entry.revision,
        title: entry.title.clone(),
        subtitle: entry.subtitle.clone(),
        icon: icon_name(entry),
        last_used: entry.last_used,
        keywords: entry.keywords.clone(),
    }
}

/// The contract's icon name, as its WIT enum spells it.
fn icon_name(entry: &CatalogueEntry) -> String {
    format!("{:?}", entry.icon).to_lowercase()
}

/// A plugin row as the catalogue matches it: the title, then the subtitle and keywords in
/// the address field. How a plugin row looks is the owner's call (#265, question 1).
fn catalogue_entry(target: Target, entry: &PluginFeedEntry) -> Entry {
    let address = std::iter::once(entry.subtitle.as_str())
        .chain(entry.keywords.iter().map(String::as_str))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Entry {
        target,
        title: entry.title.clone(),
        address,
        usage: Usage {
            visits: 0,
            last_used: entry.last_used.map(|millis| millis.div_euclid(1000)),
        },
        open: false,
    }
}

#[cfg(test)]
#[path = "plugin_feed_tests.rs"]
mod tests;

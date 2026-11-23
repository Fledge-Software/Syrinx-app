//! Pulls plugin search feeds into Syrinx's own search (#141): only when a feed is announced
//! as changed, and a pull is never started or moved on because the owner typed (D9). A
//! feed's cursor moves only once the rows it covers are kept in both tiers.

mod batch;
mod keys;
pub(super) mod sinks;
mod worker;

use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, Instant};

use syrinx_plugin_host::HostError;
use syrinx_plugin_host::bindings::exports::syrinx::plugin::search_feed::Batch;
use syrinx_plugin_host::bindings::syrinx::plugin::types::PluginError;
use syrinx_plugin_host::owner::OwnerRuntime;
use syrinx_plugin_host::session::CallResult;
use syrinx_plugin_host::worker::{Pending, PluginEvent};
use syrinx_search::fulltext::{FullTextError, Namespace};
use syrinx_search::{Catalogue, FeedKey, SearchError};
use syrinx_store::{
    FeedRowChange, FeedRowOutcome, PluginFeedCommit, PluginFeedEntry, Store, StoreError,
    StoredFeedEntry,
};

use keys::FeedKeys;
use sinks::{FeedIndex, FeedStore, committed};
pub(super) use worker::is_typing;

/// The most changes one pull asks a plugin for; the plugin contract allows 512.
pub(super) const BATCH_CHANGES: u32 = 256;
/// The most batches one announcement pulls before the feed waits behind every other one.
pub(super) const BATCHES_PER_TRIGGER: u32 = 8;

/// Why one pull, or one plugin's removal, did not finish. None carries a plugin's text.
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
    /// What the runtime reported since the last look.
    fn take_events(&mut self) -> Vec<PluginEvent>;
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
    fn take_events(&mut self) -> Vec<PluginEvent> {
        OwnerRuntime::take_events(self)
    }

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
    pub(super) store: &'a dyn FeedStore,
    pub(super) index: Option<&'a mut dyn FeedIndex>,
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

/// The session's plugin feeds: their interned rows, the feeds waiting to be pulled, the pull
/// in flight and the removed plugins still to clear. Nothing here runs a plugin unless a feed
/// was announced.
#[derive(Default)]
pub(super) struct PluginFeeds {
    keys: FeedKeys,
    waiting: VecDeque<FeedName>,
    pull: Option<Pull>,
    /// Plugins the runtime removed whose rows and text are still to go.
    removed: Vec<String>,
    /// Whether this session has looked for text a removed plugin left in the index.
    swept: bool,
    /// When the owner last typed in the palette; pulls wait `TYPING_PAUSE` after it (D9).
    typed_at: Option<Instant>,
}

/// How long after the owner's last keystroke in the palette a pull may move on again.
const TYPING_PAUSE: Duration = Duration::from_millis(750);

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

    /// Queues each feed announced as changed and each plugin removed; returns a description
    /// of every other event, which feeds do not read.
    pub(super) fn notice(&mut self, events: impl IntoIterator<Item = PluginEvent>) -> Vec<String> {
        let mut unread = Vec::new();
        for event in events {
            match event {
                PluginEvent::FeedChanged { plugin, feed } => {
                    self.announce(FeedName { plugin, feed });
                }
                PluginEvent::Removed { plugin } => self.removed.push(plugin),
                other => unread.push(describe(&other)),
            }
        }
        unread
    }

    /// Records that the owner typed in the palette at `now`.
    pub(super) fn note_typing(&mut self, now: Instant) {
        self.typed_at = Some(now);
    }

    /// How long pulls still wait for the owner's typing to pause; `None` once it has.
    pub(super) fn typing_quiet_for(&self, now: Instant) -> Option<Duration> {
        let resumes = self.typed_at? + TYPING_PAUSE;
        let left = resumes.saturating_duration_since(now);
        (!left.is_zero()).then_some(left)
    }

    /// Whether a pull is in flight, or work waits while the index can take it.
    pub(super) fn busy(&self, index_open: bool) -> bool {
        let waiting = !self.waiting.is_empty() || !self.removed.is_empty();
        self.pull.is_some() || (index_open && waiting)
    }

    /// Moves pulling on by at most one batch: clears removed plugins, applies an answered
    /// batch, or starts the next waiting feed. Never waits for a plugin.
    pub(super) fn pump(
        &mut self,
        source: &mut dyn FeedSource,
        sinks: &mut Sinks<'_>,
    ) -> Result<(), FeedError> {
        self.forget_removed(sinks)?;
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
        index: Option<&mut dyn FeedIndex>,
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
        // A closed index cannot be reached, so the owner hears that the text was left; the
        // next session's sweep removes it.
        let index = index.ok_or(FeedError::Index(FullTextError::NeedsReopen))?;
        committed(index.remove_plugin(plugin)).map_err(FeedError::Index)
    }

    /// Clears every plugin the runtime reported removed this session.
    fn forget_removed(&mut self, sinks: &mut Sinks<'_>) -> Result<(), FeedError> {
        for plugin in std::mem::take(&mut self.removed) {
            // Re-typed, so the index's borrow can end with this one call.
            let index = sinks
                .index
                .as_deref_mut()
                .map(|index| index as &mut dyn FeedIndex);
            self.forget_plugin(&plugin, index, sinks.catalogue)?;
        }
        Ok(())
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
        let installed = sinks.store.installed().map_err(FeedError::Store)?;
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
            .cursor(&name.plugin, &name.feed)
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

    /// Keeps one batch, in this order: the whole batch is checked against both tiers; the
    /// documents go to the index; the rows and the new cursor to the store, in one
    /// transaction; only then is the feed marked as fed in the index, and the catalogue
    /// updated as the store kept it. A failure anywhere before the store commit leaves the
    /// old cursor, so the batch is replayed, which both tiers take as a no-op where they hold
    /// it; and a pull from the start is not marked as fed until its cursor is kept, so a
    /// failure there pulls from the start again rather than on from an older cursor.
    fn apply(
        &mut self,
        pull: &Pull,
        batch: &Batch,
        sinks: &mut Sinks<'_>,
    ) -> Result<(), FeedError> {
        let kept = batch::kept(batch, pull.from_start);
        let namespace = namespace(&pull.name)?;
        let rows = batch::row_changes(kept.changes);
        let commit = PluginFeedCommit {
            plugin_id: pull.name.plugin.clone(),
            feed: pull.name.feed.clone(),
            clear_first: kept.clear_first,
            changes: rows.clone(),
            next_cursor: batch.next_cursor.clone(),
        };
        batch::check(&commit, &namespace, kept.changes)?;
        let index = sinks
            .index
            .as_deref_mut()
            .ok_or(FeedError::Index(FullTextError::NeedsReopen))?;
        keep_documents(index, &namespace, &kept, sinks.now)?;
        let outcomes = sinks.store.commit(commit).map_err(FeedError::Store)?;
        mark_fed(index, &namespace)?;
        self.show_kept(
            sinks.catalogue,
            &pull.name,
            kept.clear_first,
            &rows,
            &outcomes,
        )
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
            .upsert(batch::catalogue_entry(target, entry))
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

fn namespace(name: &FeedName) -> Result<Namespace, FeedError> {
    Namespace::plugin_feed(&name.plugin, &name.feed).map_err(|_| FeedError::Name)
}

/// An event feeds do not read, named by plugin and kind only.
fn describe(event: &PluginEvent) -> String {
    let (plugin, kind) = match event {
        PluginEvent::Outcome { plugin, .. } => (Some(plugin), "a write's outcome"),
        PluginEvent::Failed { plugin, .. } => (Some(plugin), "the plugin failed"),
        PluginEvent::Deactivated { plugin, .. } => (Some(plugin), "the plugin stopped"),
        PluginEvent::Cache { plugin, .. } => (plugin.as_ref(), "a compiled-code cache problem"),
        PluginEvent::PrepareFailed { plugin, .. } => (Some(plugin), "a background compile failed"),
        PluginEvent::FeedChanged { plugin, .. } => (Some(plugin), "a feed changed"),
        PluginEvent::Removed { plugin } => (Some(plugin), "the plugin was removed"),
    };
    match plugin {
        Some(plugin) => format!("{plugin}: {kind}"),
        None => kind.to_owned(),
    }
}

/// Commits the kept part of a batch to the feed's own namespace, which D40's page-text
/// expiry never reads. The fed marker is not written here (see `apply`).
fn keep_documents(
    index: &mut dyn FeedIndex,
    namespace: &Namespace,
    kept: &batch::Kept<'_>,
    now: i64,
) -> Result<(), FeedError> {
    if kept.clear_first {
        committed(index.reset(namespace)).map_err(FeedError::Index)?;
    }
    for change in kept.changes {
        // One at a time, so a deletion sees a document staged earlier in the same batch.
        let Some(staged) = batch::document_change(index, namespace, change, now)? else {
            continue;
        };
        index.stage(&[staged], now).map_err(FeedError::Index)?;
    }
    committed(index.commit()).map_err(FeedError::Index)
}

/// Marks the feed as fed once its cursor is kept; a separate small commit only when the
/// marker is missing, as after a reset or a pull from the start.
fn mark_fed(index: &mut dyn FeedIndex, namespace: &Namespace) -> Result<(), FeedError> {
    if index.is_fed(namespace).map_err(FeedError::Index)? {
        return Ok(());
    }
    index.mark_fed(namespace).map_err(FeedError::Index)?;
    committed(index.commit()).map_err(FeedError::Index)
}

#[cfg(test)]
#[path = "plugin_feed_tests.rs"]
mod tests;

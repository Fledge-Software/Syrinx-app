//! Stand-ins for a plugin, the store and the index, so a test can script answers and
//! failures without a real plugin runtime.

use super::*;

/// A batch already answered.
pub(super) struct Ready(pub(super) Batch);

impl BatchAnswer for Ready {
    fn try_answer(&self) -> Option<CallResult<Result<Batch, PluginError>>> {
        Some(Ok(Ok(self.0.clone())))
    }
}

/// A plugin that answers each cursor with a scripted batch, whichever of its feeds is asked,
/// and records every request.
#[derive(Default)]
pub(super) struct Scripted {
    pub(super) batches: BTreeMap<Option<String>, Batch>,
    pub(super) requests: Vec<Option<String>>,
    /// Events the runtime reports at the next look.
    pub(super) events: Vec<PluginEvent>,
    /// Counts every call, shared with checks made while this plugin is borrowed elsewhere.
    pub(super) calls: Rc<Cell<usize>>,
}

impl Scripted {
    pub(super) fn answer(mut self, after: Option<&str>, batch: Batch) -> Self {
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
        assert!(feed.starts_with(plugin), "a feed of the plugin asked");
        self.calls.set(self.calls.get() + 1);
        self.requests.push(after.clone());
        let batch = self.batches.get(&after).expect("a scripted batch").clone();
        Ok(Box::new(Ready(batch)))
    }
}

/// A plugin whose feed never ends: every batch has one more entry, a new cursor and more.
#[derive(Default)]
pub(super) struct Endless {
    pub(super) requests: usize,
}

impl FeedSource for Endless {
    fn take_events(&mut self) -> Vec<PluginEvent> {
        Vec::new()
    }

    fn request(
        &mut self,
        _plugin: &str,
        _feed: &str,
        _after: Option<String>,
    ) -> CallResult<Box<dyn BatchAnswer>> {
        self.requests += 1;
        let page = self.requests;
        let changes = vec![entry(&format!("e{page}"), 1, "Synthetic endless entry")];
        Ok(Box::new(Ready(batch(changes, &format!("e{page}"), true))))
    }
}

/// The store, failing its next feed commits as a crash before the commit would.
pub(super) struct FailingStore<'a> {
    pub(super) store: &'a Store,
    pub(super) failures: Cell<u32>,
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

/// The index, failing commits after `passes` good ones, as a crash before the commit would.
pub(super) struct FailingIndex<'a> {
    pub(super) index: &'a mut FullText,
    /// Commits that succeed before the failures begin.
    pub(super) passes: u32,
    pub(super) failures: u32,
    /// Plugin removals that fail, as a failing disk would, before they work again.
    pub(super) remove_failures: u32,
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
        if self.passes > 0 {
            self.passes -= 1;
            return FeedIndex::commit(&mut *self.index);
        }
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
        if self.remove_failures > 0 {
            self.remove_failures -= 1;
            return Err(FullTextError::Io);
        }
        FeedIndex::remove_plugin(&mut *self.index, plugin)
    }
}

//! The store and the full-text index as a pull sees them, so tests can fail one step of a
//! batch and prove that what was kept before it is replayed, never lost (#141).

use std::collections::BTreeSet;

use syrinx_search::fulltext::{self, FullText, FullTextError, Namespace, Outcome};
use syrinx_store::{FeedRowOutcome, PluginFeedCommit, Store, StoreError};

/// What a pull reads from and writes to in the store.
pub(in crate::session) trait FeedStore {
    fn cursor(&self, plugin: &str, feed: &str) -> Result<Option<String>, StoreError>;
    fn commit(&self, commit: PluginFeedCommit) -> Result<Vec<FeedRowOutcome>, StoreError>;
    /// The id of every plugin the store records as installed.
    fn installed(&self) -> Result<BTreeSet<String>, StoreError>;
}

impl FeedStore for Store {
    fn cursor(&self, plugin: &str, feed: &str) -> Result<Option<String>, StoreError> {
        self.plugin_feed_cursor(plugin, feed)
    }

    fn commit(&self, commit: PluginFeedCommit) -> Result<Vec<FeedRowOutcome>, StoreError> {
        self.commit_plugin_feed(commit)
    }

    fn installed(&self) -> Result<BTreeSet<String>, StoreError> {
        let installs = self.plugin_installs()?;
        Ok(installs
            .into_iter()
            .map(|install| install.plugin_id)
            .collect())
    }
}

/// What a pull uses of the full-text index; `FullText` documents each operation.
pub(in crate::session) trait FeedIndex {
    fn reset(&mut self, namespace: &Namespace) -> Result<(), FullTextError>;
    fn stage(
        &mut self,
        changes: &[fulltext::Change],
        now: i64,
    ) -> Result<Vec<Outcome>, FullTextError>;
    fn held_revision(&self, namespace: &Namespace, id: &str) -> Result<Option<u64>, FullTextError>;
    fn mark_fed(&mut self, namespace: &Namespace) -> Result<(), FullTextError>;
    fn is_fed(&self, namespace: &Namespace) -> Result<bool, FullTextError>;
    fn commit(&mut self) -> Result<(), FullTextError>;
    fn plugin_ids(&self) -> Result<BTreeSet<String>, FullTextError>;
    fn remove_plugin(&mut self, plugin: &str) -> Result<(), FullTextError>;
}

impl FeedIndex for FullText {
    fn reset(&mut self, namespace: &Namespace) -> Result<(), FullTextError> {
        FullText::reset(self, namespace)
    }

    fn stage(
        &mut self,
        changes: &[fulltext::Change],
        now: i64,
    ) -> Result<Vec<Outcome>, FullTextError> {
        FullText::stage(self, changes, now)
    }

    fn held_revision(&self, namespace: &Namespace, id: &str) -> Result<Option<u64>, FullTextError> {
        FullText::held_revision(self, namespace, id)
    }

    fn mark_fed(&mut self, namespace: &Namespace) -> Result<(), FullTextError> {
        FullText::mark_fed(self, namespace)
    }

    fn is_fed(&self, namespace: &Namespace) -> Result<bool, FullTextError> {
        FullText::is_fed(self, namespace)
    }

    fn commit(&mut self) -> Result<(), FullTextError> {
        FullText::commit(self)
    }

    fn plugin_ids(&self) -> Result<BTreeSet<String>, FullTextError> {
        FullText::plugin_ids(self)
    }

    fn remove_plugin(&mut self, plugin: &str) -> Result<(), FullTextError> {
        FullText::remove_plugin(self, plugin)
    }
}

/// An index operation that commits: `CleanupPending` means the commit is durable and only
/// the replaced text's removal waits for the next commit or the idle retry, as page text
/// treats it (`page_text::cleanup_later`).
pub(super) fn committed(result: Result<(), FullTextError>) -> Result<(), FullTextError> {
    match result {
        Ok(()) | Err(FullTextError::CleanupPending) => Ok(()),
        Err(error) => Err(error),
    }
}

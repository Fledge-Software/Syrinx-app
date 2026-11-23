//! The store's plugin search feed records (#141): each feed's cursor and its tier 1 rows.

use std::sync::mpsc;

use super::{Store, WriterRequest};
use crate::error::StoreError;
use crate::{FeedRowOutcome, PluginFeedCommit, StoredFeedEntry};

impl Store {
    /// Keeps one pulled batch's tier 1 rows and then its cursor in one transaction, so the
    /// cursor never passes rows that were not kept. Returns one outcome per change.
    pub fn commit_plugin_feed(
        &self,
        commit: PluginFeedCommit,
    ) -> Result<Vec<FeedRowOutcome>, StoreError> {
        crate::plugin_feeds::validate(&commit)?;
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::PluginFeed { commit, reply })
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// The cursor the feed's last commit kept; `None` for a feed never committed.
    pub fn plugin_feed_cursor(
        &self,
        plugin_id: &str,
        feed: &str,
    ) -> Result<Option<String>, StoreError> {
        self.read(|connection| crate::plugin_feeds::cursor(connection, plugin_id, feed))
    }

    /// Every kept tier 1 row of every plugin, for the palette's catalogue at start-up.
    pub fn plugin_feed_entries(&self) -> Result<Vec<StoredFeedEntry>, StoreError> {
        self.read(|connection| crate::plugin_feeds::entries(connection))
    }
}

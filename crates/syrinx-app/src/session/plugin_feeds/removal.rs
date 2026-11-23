//! Clears a removed plugin from both search tiers: its catalogue rows at once, and its text
//! once the index can take it, even when the plugin's manifest is gone (#141).

use syrinx_search::Catalogue;
use syrinx_search::fulltext::FullTextError;

use super::sinks::{FeedIndex, committed};
use super::{FeedError, PluginFeeds, Sinks};

impl PluginFeeds {
    /// Takes a removed plugin's rows out of the catalogue and its text out of the index; its
    /// store rows and cursors went with its install record.
    pub(super) fn forget_plugin(
        &mut self,
        plugin: &str,
        index: Option<&mut dyn FeedIndex>,
        catalogue: &mut Catalogue,
    ) -> Result<(), FeedError> {
        self.forget_rows(plugin, catalogue)?;
        forget_text(plugin, index)
    }

    /// Lets the next turn with the index open try again the text it could not yet remove.
    /// The session calls this at each idle tick, whatever pulls are under way.
    pub(super) fn retry_removals(&mut self) {
        self.retry_due = true;
    }

    /// Drops a plugin's queued and running pulls, what they remembered, and its catalogue rows.
    fn forget_rows(&mut self, plugin: &str, catalogue: &mut Catalogue) -> Result<(), FeedError> {
        self.waiting.retain(|name| name.plugin != plugin);
        self.carried.retain(|name, _| name.plugin != plugin);
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
        Ok(())
    }

    /// Clears every plugin the runtime reported removed: its catalogue rows at once, whatever
    /// else fails, and its text once the index takes it. A closed index is reported once, when
    /// the removal is first heard. The first error is reported after every plugin's turn.
    pub(super) fn forget_removed(&mut self, sinks: &mut Sinks<'_>) -> Result<(), FeedError> {
        let fresh = std::mem::take(&mut self.removed);
        let mut first_error = None;
        for plugin in &fresh {
            if let Err(error) = self.forget_rows(plugin, sinks.catalogue) {
                first_error.get_or_insert(error);
            }
        }
        if !fresh.is_empty() {
            self.retry_due = true;
            if sinks.index.is_none() {
                first_error.get_or_insert(FeedError::Index(FullTextError::NeedsReopen));
            }
        }
        for plugin in fresh {
            if !self.unremoved.contains(&plugin) {
                self.unremoved.push(plugin);
            }
        }
        if let Err(error) = self.forget_unremoved_text(sinks) {
            first_error.get_or_insert(error);
        }
        first_error.map_or(Ok(()), Err)
    }

    /// When a retry is due and the index is open, removes the text of each removed plugin that
    /// still holds some; a plugin whose text could not be removed waits for the next retry.
    fn forget_unremoved_text(&mut self, sinks: &mut Sinks<'_>) -> Result<(), FeedError> {
        let (true, Some(index)) = (self.retry_due, sinks.index.as_deref_mut()) else {
            return Ok(());
        };
        self.retry_due = false;
        let mut first_error = None;
        for plugin in std::mem::take(&mut self.unremoved) {
            if let Err(error) = forget_text(&plugin, Some(&mut *index)) {
                self.unremoved.push(plugin);
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Once per session, with the index open, removes the text of every plugin the store no
    /// longer records as installed: one removed while the index was closed left it there.
    pub(super) fn sweep_once(&mut self, sinks: &mut Sinks<'_>) -> Result<(), FeedError> {
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
}

/// Removes a plugin's text from the index; a closed index cannot, and says so.
fn forget_text(plugin: &str, index: Option<&mut dyn FeedIndex>) -> Result<(), FeedError> {
    let index = index.ok_or(FeedError::Index(FullTextError::NeedsReopen))?;
    committed(index.remove_plugin(plugin)).map_err(FeedError::Index)
}

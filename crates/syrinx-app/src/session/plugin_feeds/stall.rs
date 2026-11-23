//! Notices a feed going round: a plugin that keeps saying "more" while sending cursors it has
//! already sent would otherwise be pulled for as long as Syrinx stays open (#141).

use std::collections::{BTreeMap, BTreeSet};

use super::{FeedError, FeedName};

/// The cursors the pull in flight and its feed's previous budget-spent announcement asked
/// after, and the feeds whose stall the owner has already been shown this session.
#[derive(Default)]
pub(super) struct CursorHistory {
    /// Every cursor the pull in flight's announcement asked after.
    asked: Vec<String>,
    /// What the feed's previous announcement asked after, when it spent its budget.
    earlier: Vec<String>,
    /// The cursors of each feed waiting after spending its budget, for its next `earlier`.
    carried: BTreeMap<FeedName, Vec<String>>,
    /// Feeds whose stall the owner has been shown; repeats are only logged.
    stalled: BTreeSet<FeedName>,
}

impl CursorHistory {
    /// Starts the history of a new pull of `name` after `after`, with what the feed's previous
    /// announcement carried.
    pub(super) fn start(&mut self, name: &FeedName, after: Option<&str>) {
        self.earlier = self.carried.remove(name).unwrap_or_default();
        self.asked = after.map(str::to_owned).into_iter().collect();
    }

    /// Whether this pull or the feed's previous announcement already asked after `cursor`.
    pub(super) fn seen(&self, cursor: &str) -> bool {
        let held = |cursors: &[String]| cursors.iter().any(|asked| asked == cursor);
        held(&self.asked) || held(&self.earlier)
    }

    /// Records that the pull asks after `cursor` next.
    pub(super) fn ask(&mut self, cursor: &str) {
        self.asked.push(cursor.to_owned());
    }

    /// The announcement spent its budget at `cursor`: keeps what it asked after for the feed's
    /// next announcement.
    pub(super) fn carry(&mut self, name: &FeedName, cursor: &str) {
        self.ask(cursor);
        self.carried
            .insert(name.clone(), std::mem::take(&mut self.asked));
    }

    /// Forgets the pull's cursors. A pull from the feed's start begins it again, so a plugin
    /// may rightly send a cursor it sent before.
    pub(super) fn clear(&mut self) {
        self.asked.clear();
        self.earlier.clear();
    }

    /// Forgets what a removed plugin's feeds carried, and that they stalled.
    pub(super) fn forget_plugin(&mut self, plugin: &str) {
        self.carried.retain(|name, _| name.plugin != plugin);
        self.stalled.retain(|name| name.plugin != plugin);
    }

    /// Stops a feed that went round: the owner is shown its first stall this session, and the
    /// rest are only logged, by plugin id, so a looping plugin cannot flood the window.
    pub(super) fn report_stall(&mut self, name: FeedName) -> Result<(), FeedError> {
        let plugin = name.plugin.clone();
        if self.stalled.insert(name) {
            return Err(FeedError::Stalled { plugin });
        }
        eprintln!("Syrinx: plugin {plugin}: a search feed went round again and was stopped");
        Ok(())
    }
}

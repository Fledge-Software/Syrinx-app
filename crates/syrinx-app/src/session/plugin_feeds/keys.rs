//! Interns plugin feed rows as the `Copy` catalogue targets `Target::Plugin` carries (#141).

use std::collections::HashMap;
use std::num::NonZeroU32;

use syrinx_search::{EntryKey, FeedKey, Target};

/// One feed's interned entries. Keys are never reused within a session, so a stale target a
/// palette result still holds can never name another entry.
struct FeedEntries {
    plugin: String,
    feed: String,
    by_id: HashMap<String, EntryKey>,
    last: u32,
}

/// The session's interned feeds; a feed's key is its position plus one.
#[derive(Default)]
pub(super) struct FeedKeys {
    feeds: Vec<Option<FeedEntries>>,
}

impl FeedKeys {
    /// The key of `(plugin, feed)`, interned on first use; `None` once every key is taken.
    pub(super) fn feed(&mut self, plugin: &str, feed: &str) -> Option<FeedKey> {
        if let Some(key) = self.find(plugin, feed) {
            return Some(key);
        }
        let key = position_key(self.feeds.len())?;
        self.feeds.push(Some(FeedEntries {
            plugin: plugin.to_owned(),
            feed: feed.to_owned(),
            by_id: HashMap::new(),
            last: 0,
        }));
        Some(key)
    }

    /// The target for `entry_id` in `feed`, interned on first use.
    pub(super) fn target(&mut self, feed: FeedKey, entry_id: &str) -> Option<Target> {
        let entries = self.entries_mut(feed)?;
        if let Some(entry) = entries.by_id.get(entry_id) {
            return Some(Target::Plugin {
                feed,
                entry: *entry,
            });
        }
        let next = entries.last.checked_add(1)?;
        let entry = EntryKey::new(NonZeroU32::new(next)?);
        entries.last = next;
        entries.by_id.insert(entry_id.to_owned(), entry);
        Some(Target::Plugin { feed, entry })
    }

    /// The target `entry_id` already has, without interning a new one.
    pub(super) fn existing(&self, feed: FeedKey, entry_id: &str) -> Option<Target> {
        let entry = *self.entries(feed)?.by_id.get(entry_id)?;
        Some(Target::Plugin { feed, entry })
    }

    /// Forgets one entry's key, once its row has left the catalogue.
    pub(super) fn forget_entry(&mut self, feed: FeedKey, entry_id: &str) {
        if let Some(entries) = self.entries_mut(feed) {
            entries.by_id.remove(entry_id);
        }
    }

    /// Forgets every entry of `feed` and returns the targets they had.
    pub(super) fn forget_entries(&mut self, feed: FeedKey) -> Vec<Target> {
        let Some(entries) = self.entries_mut(feed) else {
            return Vec::new();
        };
        entries
            .by_id
            .drain()
            .map(|(_, entry)| Target::Plugin { feed, entry })
            .collect()
    }

    /// Forgets every feed of `plugin` and returns the targets their entries had.
    pub(super) fn forget_plugin(&mut self, plugin: &str) -> Vec<Target> {
        let mut forgotten = Vec::new();
        for (index, slot) in self.feeds.iter_mut().enumerate() {
            let owned = slot
                .as_ref()
                .is_some_and(|entries| entries.plugin == plugin);
            if !owned {
                continue;
            }
            let (Some(entries), Some(feed)) = (slot.take(), position_key(index)) else {
                continue;
            };
            let targets = entries.by_id.into_values();
            forgotten.extend(targets.map(|entry| Target::Plugin { feed, entry }));
        }
        forgotten
    }

    fn find(&self, plugin: &str, feed: &str) -> Option<FeedKey> {
        let index = self.feeds.iter().position(|slot| {
            slot.as_ref()
                .is_some_and(|entries| entries.plugin == plugin && entries.feed == feed)
        })?;
        position_key(index)
    }

    fn entries(&self, feed: FeedKey) -> Option<&FeedEntries> {
        self.feeds.get(slot_index(feed)?)?.as_ref()
    }

    fn entries_mut(&mut self, feed: FeedKey) -> Option<&mut FeedEntries> {
        self.feeds.get_mut(slot_index(feed)?)?.as_mut()
    }
}

/// A feed's key from its position in the list.
fn position_key(index: usize) -> Option<FeedKey> {
    let number = u32::try_from(index.checked_add(1)?).ok()?;
    Some(FeedKey::new(NonZeroU32::new(number)?))
}

/// A feed's position in the list from its key.
fn slot_index(feed: FeedKey) -> Option<usize> {
    usize::try_from(feed.get().get()).ok()?.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_stay_stable_and_a_forgotten_one_is_never_reused() {
        let mut keys = FeedKeys::default();
        let notes = keys.feed("notes", "notes.titles").expect("a feed key");
        assert_eq!(keys.feed("notes", "notes.titles"), Some(notes));
        let other = keys
            .feed("forge", "notes.titles")
            .expect("another plugin's key");
        assert_ne!(notes, other);
        let first = keys.target(notes, "made-up-1").expect("a target");
        assert_eq!(keys.target(notes, "made-up-1"), Some(first));
        assert_eq!(keys.existing(notes, "made-up-1"), Some(first));
        keys.forget_entry(notes, "made-up-1");
        assert_eq!(keys.existing(notes, "made-up-1"), None);
        // A new key, never the forgotten one, so a stale result cannot name another entry.
        let second = keys.target(notes, "made-up-1").expect("a new target");
        assert_ne!(second, first);
        let kept = keys.target(other, "kept").expect("another plugin's entry");

        // Removing one plugin forgets its feeds only, and a later feed gets a new key.
        assert_eq!(keys.forget_plugin("notes"), [second]);
        assert_eq!(keys.existing(notes, "made-up-1"), None);
        assert_eq!(keys.existing(other, "kept"), Some(kept));
        assert_ne!(keys.feed("notes", "notes.titles"), Some(notes));
    }
}

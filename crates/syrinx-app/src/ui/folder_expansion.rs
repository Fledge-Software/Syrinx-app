//! Remembers which saved folders are open across restarts, through the settings table (P1-21).
//!
//! Only expanded folders are stored, so a folder Syrinx has never shown open (a restored
//! import, or anything the setting does not name) starts collapsed, as P1-22 requires.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use syrinx_core::{SavedFolderId, TabModelSnapshot};

use super::BrowserView;
use crate::session::SessionCommand;

/// The settings key holding the expanded folders' IDs, space-separated in ascending order.
pub(super) const EXPANDED_FOLDERS_KEY: &str = "sidebar.expanded_folders";

impl BrowserView {
    /// Reads the stored expansion before the first state arrives; unreadable means none.
    pub(super) fn restore_folder_expansion(&mut self, settings: &BTreeMap<String, String>) {
        self.expanded_saved = settings
            .get(EXPANDED_FOLDERS_KEY)
            .and_then(|value| parse_expanded(value))
            .unwrap_or_default();
    }

    /// Collapses every restored folder the stored setting does not name as open.
    pub(super) fn collapse_restored_folders(&mut self, model: &TabModelSnapshot) {
        self.collapsed_folders = restored_collapsed(
            model.saved_folders.iter().map(|folder| folder.id),
            &self.expanded_saved,
        );
    }

    /// Expands or collapses one folder's rows and remembers the choice.
    pub(super) fn toggle_folder(&mut self, id: SavedFolderId) {
        if !self.collapsed_folders.remove(&id) {
            self.collapsed_folders.insert(id);
        }
        self.save_folder_expansion();
    }

    /// Opens a folder so a row just moved into it stays in view.
    pub(super) fn expand_folder(&mut self, id: SavedFolderId) {
        if self.collapsed_folders.remove(&id) {
            self.save_folder_expansion();
        }
    }

    /// Saves the expanded set when it differs from the stored one: after a toggle, and after
    /// each state, which is when created folders (open) and removed folders (gone) appear.
    pub(super) fn save_folder_expansion(&mut self) {
        let expanded = expanded_folders(
            self.model.saved_folders.iter().map(|folder| folder.id),
            &self.collapsed_folders,
        );
        if expanded == self.expanded_saved {
            return;
        }
        // Removed folders' IDs are never reused, so forgetting them here loses nothing.
        self.collapsed_folders.retain(|id| {
            self.model
                .saved_folders
                .iter()
                .any(|folder| folder.id == *id)
        });
        self.command(SessionCommand::SaveSettings(vec![expanded_setting(
            &expanded,
        )]));
        self.expanded_saved = expanded;
    }
}

/// The stored IDs, or `None` when the value is not a list of nonzero numbers.
pub(super) fn parse_expanded(value: &str) -> Option<BTreeSet<SavedFolderId>> {
    value
        .split_ascii_whitespace()
        .map(|part| {
            part.parse::<u64>()
                .ok()
                .and_then(|id| SavedFolderId::new(id).ok())
        })
        .collect()
}

/// The setting that records `expanded`.
pub(super) fn expanded_setting(expanded: &BTreeSet<SavedFolderId>) -> (String, String) {
    let ids: Vec<String> = expanded.iter().map(|id| id.get().to_string()).collect();
    (EXPANDED_FOLDERS_KEY.to_owned(), ids.join(" "))
}

/// Every folder except those stored as expanded starts collapsed.
pub(super) fn restored_collapsed(
    folders: impl Iterator<Item = SavedFolderId>,
    stored: &BTreeSet<SavedFolderId>,
) -> HashSet<SavedFolderId> {
    folders.filter(|id| !stored.contains(id)).collect()
}

/// The folders that exist and are not collapsed.
pub(super) fn expanded_folders(
    folders: impl Iterator<Item = SavedFolderId>,
    collapsed: &HashSet<SavedFolderId>,
) -> BTreeSet<SavedFolderId> {
    folders.filter(|id| !collapsed.contains(id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[u64]) -> Vec<SavedFolderId> {
        values
            .iter()
            .map(|&value| SavedFolderId::new(value).expect("synthetic folder id"))
            .collect()
    }

    #[test]
    fn the_setting_round_trips_in_ascending_order() {
        let expanded: BTreeSet<_> = ids(&[12, 3, 7]).into_iter().collect();
        let (key, value) = expanded_setting(&expanded);
        assert_eq!(key, EXPANDED_FOLDERS_KEY);
        assert_eq!(value, "3 7 12");
        assert_eq!(parse_expanded(&value), Some(expanded));
        assert_eq!(parse_expanded(""), Some(BTreeSet::new()));
    }

    #[test]
    fn unreadable_values_are_refused_whole() {
        for value in ["0", "-1", "3 x", "3,7", "18446744073709551616"] {
            assert_eq!(parse_expanded(value), None, "{value}");
        }
    }

    #[test]
    fn only_stored_folders_reopen_after_restart() {
        let stored: BTreeSet<_> = ids(&[2, 9]).into_iter().collect();
        let collapsed = restored_collapsed(ids(&[1, 2, 3]).into_iter(), &stored);
        assert_eq!(collapsed, ids(&[1, 3]).into_iter().collect());
        // What is saved next forgets the folder that no longer exists.
        assert_eq!(
            expanded_folders(ids(&[1, 2, 3]).into_iter(), &collapsed),
            ids(&[2]).into_iter().collect()
        );
    }

    #[test]
    fn a_new_folder_counts_as_open_until_collapsed() {
        let collapsed: HashSet<_> = ids(&[1]).into_iter().collect();
        assert_eq!(
            expanded_folders(ids(&[1, 4]).into_iter(), &collapsed),
            ids(&[4]).into_iter().collect()
        );
    }

    #[test]
    fn a_missing_setting_collapses_every_restored_folder() {
        let collapsed = restored_collapsed(ids(&[1, 2]).into_iter(), &BTreeSet::new());
        assert_eq!(collapsed.len(), 2);
    }
}

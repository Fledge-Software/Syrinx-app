//! Arranges saved pages into inline folders without a second bookmark model (P1-21, D43).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroU64;

use super::{SavedPage, SavedPageId, TabError, TabMetadata, TabModel};

/// A folder keeps its identity when it is renamed, moved or emptied.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SavedFolderId(NonZeroU64);

impl SavedFolderId {
    /// Validates a numeric identity without creating a folder.
    pub fn new(value: u64) -> Result<Self, TabError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(TabError::InvalidSavedFolderId)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// Either kind of row in the saved tree.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SavedEntryId {
    Page(SavedPageId),
    Folder(SavedFolderId),
}

/// One row of the saved tree. The layout lists rows in visible depth-first order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavedPlacement {
    pub entry: SavedEntryId,
    pub parent: Option<SavedFolderId>,
}

/// An inline folder of saved pages and further folders.
#[derive(Clone, Eq, PartialEq)]
pub struct SavedFolder {
    pub(super) id: SavedFolderId,
    pub(super) title: String,
}

impl SavedFolder {
    pub fn id(&self) -> SavedFolderId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

impl fmt::Debug for SavedFolder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Folder names are owner content, so diagnostics show only the identity.
        formatter
            .debug_struct("SavedFolder")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// What happens to a folder's contents when the folder itself is removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderRemoval {
    /// Children take the folder's place in its parent, in their existing order.
    MoveContentsUp,
    /// Every nested folder and saved page is removed; open pages stay open as ordinary tabs.
    RemoveContents,
}

/// The saved records a folder removal deleted, in visible order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RemovedSavedEntries {
    pub folders: Vec<SavedFolder>,
    pub pages: Vec<SavedPage>,
}

impl TabModel {
    /// Folders in visible depth-first order.
    pub fn saved_folders(&self) -> &[SavedFolder] {
        &self.folders
    }

    /// Every saved page and folder with its parent, in visible depth-first order.
    pub fn saved_layout(&self) -> &[SavedPlacement] {
        &self.layout
    }

    pub fn get_folder(&self, id: SavedFolderId) -> Option<&SavedFolder> {
        self.folders.iter().find(|folder| folder.id == id)
    }

    /// The folder that directly contains a saved row, or `None` at the top level.
    pub fn saved_parent(&self, entry: SavedEntryId) -> Result<Option<SavedFolderId>, TabError> {
        let index = self.placement_index(entry)?;
        Ok(self.layout[index].parent)
    }

    /// The direct children of a folder, or of the top level, in visible order.
    pub fn saved_children(
        &self,
        parent: Option<SavedFolderId>,
    ) -> Result<Vec<SavedEntryId>, TabError> {
        self.require_folder(parent)?;
        Ok(self
            .child_indexes(parent)
            .map(|index| self.layout[index].entry)
            .collect())
    }

    /// Creates an empty folder at the end of `parent`'s children.
    pub fn create_folder(
        &mut self,
        title: String,
        parent: Option<SavedFolderId>,
    ) -> Result<SavedFolderId, TabError> {
        // Validate and allocate before changing the tree so failures leave it untouched.
        self.require_folder(parent)?;
        let id = self.allocate_folder_id()?;
        let position = self.append_position(parent)?;
        self.layout.insert(
            position,
            SavedPlacement {
                entry: SavedEntryId::Folder(id),
                parent,
            },
        );
        self.folders.push(SavedFolder { id, title });
        self.sync_saved_order();
        Ok(id)
    }

    pub fn rename_folder(&mut self, id: SavedFolderId, title: String) -> Result<(), TabError> {
        let folder = self
            .folders
            .iter_mut()
            .find(|folder| folder.id == id)
            .ok_or(TabError::UnknownSavedFolder(id))?;
        folder.title = title;
        Ok(())
    }

    /// Adds an unloaded saved page at the end of `parent`'s children, as import needs.
    pub fn add_saved_page(
        &mut self,
        metadata: TabMetadata,
        parent: Option<SavedFolderId>,
    ) -> Result<SavedPageId, TabError> {
        // Allocate before changing the tree; no live page is opened.
        self.require_folder(parent)?;
        let id = self.allocate_saved_id()?;
        let position = self.append_position(parent)?;
        self.layout.insert(
            position,
            SavedPlacement {
                entry: SavedEntryId::Page(id),
                parent,
            },
        );
        self.saved.push(SavedPage { id, metadata });
        self.sync_saved_order();
        Ok(id)
    }

    /// Explicitly edits a saved destination; its open page, if any, keeps its own state.
    pub fn edit_saved(&mut self, id: SavedPageId, metadata: TabMetadata) -> Result<(), TabError> {
        let index = self.saved_index_of(id)?;
        self.saved[index].metadata = metadata;
        Ok(())
    }

    /// Moves a page or folder, with everything inside it, to a final position in `parent`.
    pub fn move_saved_entry(
        &mut self,
        entry: SavedEntryId,
        parent: Option<SavedFolderId>,
        index: usize,
    ) -> Result<(), TabError> {
        // Validate the source, the destination and the position before changing anything.
        let start = self.placement_index(entry)?;
        self.require_folder(parent)?;
        let end = self.subtree_end(start);
        Self::reject_cycle(&self.layout[start..end], entry, parent)?;
        let siblings = self
            .child_indexes(parent)
            .filter(|&child| child != start)
            .count();
        Self::validate_position(index, siblings + 1)?;
        let destination = self.child_position(parent, index, Some(start))?;

        // Lift the whole subtree out, reattach its root and insert it at the destination.
        let mut block: Vec<SavedPlacement> = self.layout.drain(start..end).collect();
        if let Some(root) = block.first_mut() {
            root.parent = parent;
        }
        let destination = if destination > start {
            destination - block.len()
        } else {
            destination
        };
        self.layout.splice(destination..destination, block);
        self.sync_saved_order();
        Ok(())
    }

    /// Removes a folder, either lifting its contents into its parent or removing them too.
    pub fn remove_folder(
        &mut self,
        id: SavedFolderId,
        removal: FolderRemoval,
    ) -> Result<RemovedSavedEntries, TabError> {
        let start = self.placement_index(SavedEntryId::Folder(id))?;
        match removal {
            FolderRemoval::MoveContentsUp => Ok(self.remove_folder_keeping_contents(start, id)),
            FolderRemoval::RemoveContents => Ok(self.remove_folder_with_contents(start)),
        }
    }

    fn remove_folder_keeping_contents(
        &mut self,
        start: usize,
        id: SavedFolderId,
    ) -> RemovedSavedEntries {
        // Direct children inherit the folder's parent; depth-first order is otherwise unchanged.
        let parent = self.layout[start].parent;
        for placement in &mut self.layout {
            if placement.parent == Some(id) {
                placement.parent = parent;
            }
        }
        self.layout.remove(start);
        let folders = self.take_folders(&BTreeSet::from([id]));
        self.sync_saved_order();
        RemovedSavedEntries {
            folders,
            pages: Vec::new(),
        }
    }

    fn remove_folder_with_contents(&mut self, start: usize) -> RemovedSavedEntries {
        // Unlink pages last-first so detached tabs keep their visible order at the ordinary start.
        let end = self.subtree_end(start);
        let block: Vec<SavedPlacement> = self.layout.drain(start..end).collect();
        let mut folder_ids = BTreeSet::new();
        let mut pages = Vec::new();
        for placement in block.iter().rev() {
            match placement.entry {
                SavedEntryId::Folder(folder) => {
                    folder_ids.insert(folder);
                }
                SavedEntryId::Page(page) => pages.extend(self.unlink_saved_page(page)),
            }
        }
        pages.reverse();
        let folders = self.take_folders(&folder_ids);
        self.sync_saved_order();
        RemovedSavedEntries { folders, pages }
    }

    fn take_folders(&mut self, ids: &BTreeSet<SavedFolderId>) -> Vec<SavedFolder> {
        // Folders are kept in depth-first order, so the removed ones are returned in that order.
        let (removed, kept) = std::mem::take(&mut self.folders)
            .into_iter()
            .partition(|folder| ids.contains(&folder.id));
        self.folders = kept;
        removed
    }

    pub(super) fn placement_index(&self, entry: SavedEntryId) -> Result<usize, TabError> {
        self.layout
            .iter()
            .position(|placement| placement.entry == entry)
            .ok_or(match entry {
                SavedEntryId::Page(id) => TabError::UnknownSavedPage(id),
                SavedEntryId::Folder(id) => TabError::UnknownSavedFolder(id),
            })
    }

    fn require_folder(&self, parent: Option<SavedFolderId>) -> Result<(), TabError> {
        if let Some(id) = parent {
            self.placement_index(SavedEntryId::Folder(id))?;
        }
        Ok(())
    }

    /// Layout indexes of `parent`'s direct children, in visible order.
    fn child_indexes(&self, parent: Option<SavedFolderId>) -> impl Iterator<Item = usize> + '_ {
        self.layout
            .iter()
            .enumerate()
            .filter(move |(_, placement)| placement.parent == parent)
            .map(|(index, _)| index)
    }

    /// The layout index just past the row at `start` and everything nested inside it.
    fn subtree_end(&self, start: usize) -> usize {
        // In depth-first order a subtree is contiguous: it ends at the first outside parent.
        let mut inside = BTreeSet::new();
        if let SavedEntryId::Folder(id) = self.layout[start].entry {
            inside.insert(id);
        }
        let mut end = start + 1;
        while let Some(placement) = self.layout.get(end) {
            if !placement
                .parent
                .is_some_and(|parent| inside.contains(&parent))
            {
                break;
            }
            if let SavedEntryId::Folder(id) = placement.entry {
                inside.insert(id);
            }
            end += 1;
        }
        end
    }

    /// The layout index at which a new last child of `parent` belongs.
    fn append_position(&self, parent: Option<SavedFolderId>) -> Result<usize, TabError> {
        let count = self.child_indexes(parent).count();
        self.child_position(parent, count, None)
    }

    /// The layout index for a child at `index` among `parent`'s children, skipping `moving`.
    fn child_position(
        &self,
        parent: Option<SavedFolderId>,
        index: usize,
        moving: Option<usize>,
    ) -> Result<usize, TabError> {
        if let Some(child) = self
            .child_indexes(parent)
            .filter(|&child| Some(child) != moving)
            .nth(index)
        {
            return Ok(child);
        }
        // Past the last child: after the parent's whole subtree, or at the very end.
        let Some(folder) = parent else {
            return Ok(self.layout.len());
        };
        let start = self.placement_index(SavedEntryId::Folder(folder))?;
        Ok(self.subtree_end(start))
    }

    fn reject_cycle(
        subtree: &[SavedPlacement],
        entry: SavedEntryId,
        parent: Option<SavedFolderId>,
    ) -> Result<(), TabError> {
        // A folder cannot move into itself or into any folder nested inside it.
        let (SavedEntryId::Folder(folder), Some(parent)) = (entry, parent) else {
            return Ok(());
        };
        if subtree
            .iter()
            .any(|placement| placement.entry == SavedEntryId::Folder(parent))
        {
            return Err(TabError::FolderCycle { folder, parent });
        }
        Ok(())
    }

    /// Reorders pages, folders and the open saved tabs to follow the layout.
    pub(super) fn sync_saved_order(&mut self) {
        // Stable sorts keep every live tab's identity, selection and association unchanged.
        let positions: BTreeMap<SavedEntryId, usize> = self
            .layout
            .iter()
            .enumerate()
            .map(|(index, placement)| (placement.entry, index))
            .collect();
        let page_position = |id: SavedPageId| positions.get(&SavedEntryId::Page(id)).copied();
        self.saved.sort_by_key(|page| page_position(page.id));
        self.folders
            .sort_by_key(|folder| positions.get(&SavedEntryId::Folder(folder.id)).copied());
        let boundary = self.saved_tab_count();
        self.tabs[..boundary].sort_by_key(|tab| tab.saved_page_id.and_then(page_position));
    }

    fn allocate_folder_id(&mut self) -> Result<SavedFolderId, TabError> {
        // A removed folder identity stays retired, like saved-page and live-tab identities.
        let next = self
            .last_folder_id
            .checked_add(1)
            .ok_or(TabError::SavedFolderIdExhausted)?;
        let id = SavedFolderId::new(next)?;
        self.last_folder_id = next;
        Ok(id)
    }
}

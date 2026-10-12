//! Appends a whole subtree of unloaded saved rows in one step, as bookmark import needs (P1-22).

use super::{SavedEntryId, SavedFolder, SavedFolderId, SavedPage, SavedPlacement};
use super::{TabError, TabMetadata, TabModel};

/// One row of a saved subtree to add; folders hold their rows in visible order.
#[derive(Clone, Eq, PartialEq)]
pub enum NewSavedItem {
    Folder {
        title: String,
        items: Vec<NewSavedItem>,
    },
    Page(TabMetadata),
}

/// What [`TabModel::append_saved_folder`] added.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendedSavedTree {
    /// The new top-level folder holding everything else.
    pub folder: SavedFolderId,
    /// Folders added inside it, not counting it.
    pub folders: usize,
    pub pages: usize,
    /// Every added folder, the new top-level one first, in depth-first order.
    pub folder_ids: Vec<SavedFolderId>,
}

impl TabModel {
    /// Adds a top-level folder at the end of the saved rows, holding `items` as unloaded saved
    /// pages and nested folders in their given order. Nothing is opened.
    ///
    /// Identities are checked for exhaustion first, so a failure leaves the model unchanged.
    pub fn append_saved_folder(
        &mut self,
        title: String,
        items: Vec<NewSavedItem>,
    ) -> Result<AppendedSavedTree, TabError> {
        let (folders, pages) = count_items(&items);
        self.reserve_identities(folders + 1, pages)?;
        let root = self.allocate_folder_id()?;
        self.push_folder(root, title, None);
        let mut folder_ids = vec![root];
        self.push_items(root, items, &mut folder_ids)?;
        self.sync_saved_order();
        Ok(AppendedSavedTree {
            folder: root,
            folders,
            pages,
            folder_ids,
        })
    }

    /// Fails unless this many new folder and page identities are still available.
    fn reserve_identities(&self, folders: usize, pages: usize) -> Result<(), TabError> {
        let available = |last: u64, wanted: usize| {
            u64::try_from(wanted)
                .ok()
                .and_then(|wanted| last.checked_add(wanted))
                .is_some()
        };
        if !available(self.last_folder_id, folders) {
            return Err(TabError::SavedFolderIdExhausted);
        }
        if !available(self.last_saved_id, pages) {
            return Err(TabError::SavedPageIdExhausted);
        }
        Ok(())
    }

    /// Appends rows depth-first without recursion, so nesting depth cannot exhaust the stack.
    fn push_items(
        &mut self,
        root: SavedFolderId,
        items: Vec<NewSavedItem>,
        folder_ids: &mut Vec<SavedFolderId>,
    ) -> Result<(), TabError> {
        let mut pending = vec![(root, items.into_iter())];
        while let Some((parent, rows)) = pending.last_mut() {
            let parent = *parent;
            let Some(item) = rows.next() else {
                pending.pop();
                continue;
            };
            match item {
                NewSavedItem::Page(metadata) => {
                    let id = self.allocate_saved_id()?;
                    self.layout.push(SavedPlacement {
                        entry: SavedEntryId::Page(id),
                        parent: Some(parent),
                    });
                    self.saved.push(SavedPage { id, metadata });
                }
                NewSavedItem::Folder { title, items } => {
                    let id = self.allocate_folder_id()?;
                    self.push_folder(id, title, Some(parent));
                    folder_ids.push(id);
                    pending.push((id, items.into_iter()));
                }
            }
        }
        Ok(())
    }

    fn push_folder(&mut self, id: SavedFolderId, title: String, parent: Option<SavedFolderId>) {
        self.layout.push(SavedPlacement {
            entry: SavedEntryId::Folder(id),
            parent,
        });
        self.folders.push(SavedFolder { id, title });
    }
}

/// Counts nested folders and pages without recursion.
fn count_items(items: &[NewSavedItem]) -> (usize, usize) {
    let (mut folders, mut pages) = (0, 0);
    let mut pending: Vec<&[NewSavedItem]> = vec![items];
    while let Some(rows) = pending.pop() {
        for item in rows {
            match item {
                NewSavedItem::Page(_) => pages += 1,
                NewSavedItem::Folder { items, .. } => {
                    folders += 1;
                    pending.push(items);
                }
            }
        }
    }
    (folders, pages)
}

// Titles and addresses are the owner's data, so diagnostics show only the shape.
impl std::fmt::Debug for NewSavedItem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Folder { items, .. } => formatter
                .debug_struct("Folder")
                .field("items", items)
                .finish_non_exhaustive(),
            Self::Page(_) => formatter.write_str("Page { .. }"),
        }
    }
}

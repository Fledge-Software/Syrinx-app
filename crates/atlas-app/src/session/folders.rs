//! Carries the sidebar's folder edits to the one tab model on the session worker (P1-21).

use atlas_core::{FolderContents, FolderRemoval, SavedEntryId, SavedFolderId, TabError, TabModel};

/// One owner edit of the saved tree. Each is applied and persisted like any other tab command.
#[derive(Clone, Eq, PartialEq)]
pub enum FolderCommand {
    /// Adds an empty folder at the end of `parent`; a blank name is refused.
    Create {
        title: String,
        parent: Option<SavedFolderId>,
    },
    Rename {
        id: SavedFolderId,
        title: String,
    },
    /// Removes a folder and lifts its contents one level up: the default, which loses nothing.
    Remove {
        id: SavedFolderId,
    },
    /// Deletes a folder with everything in it, sent only after the owner confirmed `expected`;
    /// the model refuses it if the folder holds anything else by then.
    DeleteWithContents {
        id: SavedFolderId,
        expected: FolderContents,
    },
    /// Moves a saved page or a whole folder to a final position among `parent`'s children.
    Move {
        entry: SavedEntryId,
        parent: Option<SavedFolderId>,
        index: usize,
    },
}

impl FolderCommand {
    /// Applies the edit; a refused edit leaves the model unchanged.
    pub(super) fn apply(self, model: &mut TabModel) -> Result<(), TabError> {
        match self {
            Self::Create { title, parent } => model.create_folder(title, parent).map(|_| ()),
            Self::Rename { id, title } => model.rename_folder(id, title),
            Self::Remove { id } => model
                .remove_folder(id, FolderRemoval::MoveContentsUp)
                .map(|_| ()),
            Self::DeleteWithContents { id, expected } => {
                model.delete_folder_confirmed(id, &expected).map(|_| ())
            }
            Self::Move {
                entry,
                parent,
                index,
            } => model.move_saved_entry(entry, parent, index),
        }
    }
}

// Folder names are the owner's content, so diagnostics show only the kind of edit.
impl std::fmt::Debug for FolderCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Create { .. } => "Create",
            Self::Rename { .. } => "Rename",
            Self::Remove { .. } => "Remove",
            Self::DeleteWithContents { .. } => "DeleteWithContents",
            Self::Move { .. } => "Move",
        };
        write!(formatter, "FolderCommand::{kind} {{ .. }}")
    }
}

#[cfg(test)]
mod tests {
    use atlas_core::{Activation, TabKind, TabMetadata};

    use super::*;

    fn saved_page(model: &mut TabModel, number: usize) -> Result<SavedEntryId, TabError> {
        let id = model.open(
            TabKind::Web,
            TabMetadata {
                title: format!("Synthetic page {number}"),
                address: format!("https://folder-command-{number}.example.test/"),
                ..TabMetadata::default()
            },
            Activation::Foreground,
        )?;
        Ok(SavedEntryId::Page(model.save_page(id)?))
    }

    fn create(model: &mut TabModel, title: &str) -> Result<SavedFolderId, TabError> {
        FolderCommand::Create {
            title: title.to_owned(),
            parent: None,
        }
        .apply(model)?;
        model
            .saved_folders()
            .last()
            .map(|folder| folder.id())
            .ok_or(TabError::InvalidSavedFolderId)
    }

    #[test]
    fn each_edit_reaches_the_model() -> Result<(), TabError> {
        let mut model = TabModel::new();
        let page = saved_page(&mut model, 1)?;
        let folder = create(&mut model, "Synthetic work")?;
        FolderCommand::Rename {
            id: folder,
            title: "Synthetic renamed".to_owned(),
        }
        .apply(&mut model)?;
        assert_eq!(model.saved_folders()[0].title(), "Synthetic renamed");

        FolderCommand::Move {
            entry: page,
            parent: Some(folder),
            index: 0,
        }
        .apply(&mut model)?;
        assert_eq!(model.saved_parent(page)?, Some(folder));

        // The default removal keeps the page, which takes the folder's place.
        FolderCommand::Remove { id: folder }.apply(&mut model)?;
        assert!(model.saved_folders().is_empty());
        assert_eq!(model.saved_parent(page)?, None);
        assert_eq!(model.saved_pages().len(), 1);
        Ok(())
    }

    #[test]
    fn deleting_contents_keeps_open_pages_as_ordinary_tabs() -> Result<(), TabError> {
        let mut model = TabModel::new();
        let page = saved_page(&mut model, 1)?;
        let folder = create(&mut model, "Synthetic doomed")?;
        FolderCommand::Move {
            entry: page,
            parent: Some(folder),
            index: 0,
        }
        .apply(&mut model)?;
        let live = model.tabs()[0].id();
        // A count taken before another page arrived no longer matches, so nothing is deleted.
        let stale = FolderContents::default();
        let before = model.snapshot();
        assert_eq!(
            FolderCommand::DeleteWithContents {
                id: folder,
                expected: stale,
            }
            .apply(&mut model),
            Err(TabError::FolderContentsChanged(folder))
        );
        assert_eq!(model.snapshot(), before);
        FolderCommand::DeleteWithContents {
            id: folder,
            expected: [page].into_iter().collect(),
        }
        .apply(&mut model)?;
        assert!(model.saved_pages().is_empty());
        assert_eq!(model.tabs()[0].id(), live);
        assert_eq!(model.tabs()[0].saved_page_id(), None);
        Ok(())
    }

    #[test]
    fn refused_edits_leave_the_model_unchanged() -> Result<(), TabError> {
        let mut model = TabModel::new();
        let outer = create(&mut model, "Synthetic outer")?;
        FolderCommand::Create {
            title: "Synthetic inner".to_owned(),
            parent: Some(outer),
        }
        .apply(&mut model)?;
        let inner = model.saved_folders()[1].id();
        let before = model.snapshot();
        let refused = [
            FolderCommand::Create {
                title: "  ".to_owned(),
                parent: None,
            },
            FolderCommand::Rename {
                id: outer,
                title: String::new(),
            },
            FolderCommand::Move {
                entry: SavedEntryId::Folder(outer),
                parent: Some(inner),
                index: 0,
            },
        ];
        for command in refused {
            assert!(command.apply(&mut model).is_err());
        }
        assert_eq!(model.snapshot(), before);
        Ok(())
    }

    #[test]
    fn diagnostics_never_show_folder_names() {
        let command = FolderCommand::Rename {
            id: SavedFolderId::new(1).expect("synthetic folder id"),
            title: "Synthetic secret name".to_owned(),
        };
        assert_eq!(format!("{command:?}"), "FolderCommand::Rename { .. }");
    }
}

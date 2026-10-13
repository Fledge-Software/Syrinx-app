//! Turns the saved tree into the rows the sidebar shows, honouring collapsed folders (P1-09).

use std::collections::{HashMap, HashSet};

use atlas_core::{SavedEntryId, SavedFolderId, SavedPageId, SavedPlacement};

/// One visible saved row, with its nesting depth for indentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SavedRow {
    Folder {
        id: SavedFolderId,
        depth: usize,
        expanded: bool,
    },
    Page {
        id: SavedPageId,
        depth: usize,
    },
}

/// The saved rows in layout order, skipping everything inside a collapsed folder.
///
/// The layout is depth-first with each parent before its children (P1-21), so one pass
/// can carry each folder's depth and visibility down to its contents.
pub(super) fn visible_saved_rows(
    layout: &[SavedPlacement],
    collapsed: &HashSet<SavedFolderId>,
) -> Vec<SavedRow> {
    let mut folders: HashMap<SavedFolderId, FolderView> = HashMap::new();
    let mut rows = Vec::with_capacity(layout.len());
    for placement in layout {
        let (depth, hidden) = position_under(placement.parent, &folders);
        match placement.entry {
            SavedEntryId::Folder(id) => {
                let expanded = !collapsed.contains(&id);
                let conceals = hidden || !expanded;
                folders.insert(id, FolderView { depth, conceals });
                if !hidden {
                    rows.push(SavedRow::Folder {
                        id,
                        depth,
                        expanded,
                    });
                }
            }
            SavedEntryId::Page(id) => {
                if !hidden {
                    rows.push(SavedRow::Page { id, depth });
                }
            }
        }
    }
    rows
}

/// What a folder passes down to its contents.
#[derive(Clone, Copy)]
struct FolderView {
    depth: usize,
    conceals: bool,
}

/// A row's depth and whether a collapsed ancestor hides it.
fn position_under(
    parent: Option<SavedFolderId>,
    folders: &HashMap<SavedFolderId, FolderView>,
) -> (usize, bool) {
    let Some(parent) = parent else {
        return (0, false);
    };
    // A validated snapshot always places a parent first; an unknown one shows at the top.
    match folders.get(&parent) {
        Some(folder) => (folder.depth + 1, folder.conceals),
        None => (0, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: u64) -> SavedFolderId {
        SavedFolderId::new(id).expect("synthetic folder id")
    }

    fn page(id: u64) -> SavedPageId {
        SavedPageId::new(id).expect("synthetic page id")
    }

    fn place(entry: SavedEntryId, parent: Option<u64>) -> SavedPlacement {
        SavedPlacement {
            entry,
            parent: parent.map(folder),
        }
    }

    /// Top: page 1, folder 1 { page 2, folder 2 { page 3 } }, page 4.
    fn tree() -> Vec<SavedPlacement> {
        vec![
            place(SavedEntryId::Page(page(1)), None),
            place(SavedEntryId::Folder(folder(1)), None),
            place(SavedEntryId::Page(page(2)), Some(1)),
            place(SavedEntryId::Folder(folder(2)), Some(1)),
            place(SavedEntryId::Page(page(3)), Some(2)),
            place(SavedEntryId::Page(page(4)), None),
        ]
    }

    #[test]
    fn expanded_folders_show_every_row_indented_by_depth() {
        let rows = visible_saved_rows(&tree(), &HashSet::new());
        assert_eq!(
            rows,
            vec![
                SavedRow::Page {
                    id: page(1),
                    depth: 0
                },
                SavedRow::Folder {
                    id: folder(1),
                    depth: 0,
                    expanded: true
                },
                SavedRow::Page {
                    id: page(2),
                    depth: 1
                },
                SavedRow::Folder {
                    id: folder(2),
                    depth: 1,
                    expanded: true
                },
                SavedRow::Page {
                    id: page(3),
                    depth: 2
                },
                SavedRow::Page {
                    id: page(4),
                    depth: 0
                },
            ]
        );
    }

    #[test]
    fn collapsing_a_folder_hides_its_whole_subtree_but_not_its_row() {
        let rows = visible_saved_rows(&tree(), &HashSet::from([folder(1)]));
        assert_eq!(
            rows,
            vec![
                SavedRow::Page {
                    id: page(1),
                    depth: 0
                },
                SavedRow::Folder {
                    id: folder(1),
                    depth: 0,
                    expanded: false
                },
                SavedRow::Page {
                    id: page(4),
                    depth: 0
                },
            ]
        );
    }

    #[test]
    fn collapsing_an_inner_folder_keeps_its_siblings() {
        let rows = visible_saved_rows(&tree(), &HashSet::from([folder(2)]));
        assert!(rows.contains(&SavedRow::Page {
            id: page(2),
            depth: 1
        }));
        assert!(rows.contains(&SavedRow::Folder {
            id: folder(2),
            depth: 1,
            expanded: false
        }));
        assert!(!rows.iter().any(|row| matches!(
            row,
            SavedRow::Page { id, .. } if *id == page(3)
        )));
    }

    #[test]
    fn reexpanding_restores_the_same_rows() {
        let layout = tree();
        let mut collapsed = HashSet::from([folder(1), folder(2)]);
        let before = visible_saved_rows(&layout, &HashSet::new());
        collapsed.clear();
        assert_eq!(visible_saved_rows(&layout, &collapsed), before);
    }

    #[test]
    fn a_flat_saved_list_has_no_folder_rows() {
        let layout = vec![
            place(SavedEntryId::Page(page(1)), None),
            place(SavedEntryId::Page(page(2)), None),
        ];
        let rows = visible_saved_rows(&layout, &HashSet::new());
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|row| matches!(row, SavedRow::Page { depth: 0, .. }))
        );
    }

    #[test]
    fn an_unknown_parent_shows_the_row_at_the_top_level() {
        let layout = vec![place(SavedEntryId::Page(page(1)), Some(9))];
        assert_eq!(
            visible_saved_rows(&layout, &HashSet::new()),
            vec![SavedRow::Page {
                id: page(1),
                depth: 0
            }]
        );
    }
}

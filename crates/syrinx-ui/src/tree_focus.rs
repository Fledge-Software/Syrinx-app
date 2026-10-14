//! The sidebar tree's roving focus: one Tab stop, and where each arrow key moves it (DW-61).
//!
//! Window-independent so the docked sidebar and the peek popup share one tested rule set.
//!
//! Wiring cautions: build the `TreeRow`s from the same list that draws the rows (`syrinx-app`'s
//! `ui/sidebar_tree.rs`, `sidebar_entries`, drops a row whose lookup fails), and after every row
//! change reset the focused id to `tab_stop(...)` (or `focus_after_collapse` for a collapse).

/// What kind of row a tree row is. Only folders react to Left and Right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// A saved folder, showing its contents when expanded.
    Folder { expanded: bool },
    /// A saved page, open or not.
    SavedPage,
    /// An open tab that is not saved.
    OpenTab,
}

/// One visible row, in layout order: each folder comes before its contents, one level deeper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRow<Id> {
    /// A stable identity that survives reordering, such as the saved page or tab id.
    pub id: Id,
    /// Nesting level; top-level rows are 0.
    pub depth: usize,
    pub kind: RowKind,
}

/// The keys the tree handles, following the ARIA tree pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
}

/// A folder change a key asks the sidebar to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderRequest<Id> {
    Expand(Id),
    Collapse(Id),
}

/// What a key does: the row to focus (none only in an empty tree) and any folder change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeMove<Id> {
    pub focus: Option<Id>,
    pub folder: Option<FolderRequest<Id>>,
}

/// The row that owns the tree's single Tab stop: the focused row while it is still visible,
/// else the active row, else the first row. `None` only when the tree is empty.
pub fn tab_stop<Id: Copy + Eq>(
    rows: &[TreeRow<Id>],
    focused: Option<Id>,
    active: Option<Id>,
) -> Option<Id> {
    stop_index(rows, focused, active).map(|index| rows[index].id)
}

/// Where `key` moves focus from the current Tab stop, and any folder it expands or collapses.
///
/// A focused row that has disappeared (collapsed away or removed) counts as the Tab stop
/// `tab_stop` falls back to, so a key never acts on a row the owner cannot see.
pub fn tree_key<Id: Copy + Eq>(
    rows: &[TreeRow<Id>],
    focused: Option<Id>,
    active: Option<Id>,
    key: TreeKey,
) -> TreeMove<Id> {
    let Some(current) = stop_index(rows, focused, active) else {
        return TreeMove {
            focus: None,
            folder: None,
        };
    };
    let (target, folder) = match key {
        TreeKey::Up => (current.saturating_sub(1), None),
        TreeKey::Down => ((current + 1).min(rows.len() - 1), None),
        TreeKey::Home => (0, None),
        TreeKey::End => (rows.len() - 1, None),
        TreeKey::Left => left(rows, current),
        TreeKey::Right => right(rows, current),
    };
    TreeMove {
        focus: Some(rows[target].id),
        folder,
    }
}

/// Where focus goes when `folder` collapses while `focused` may lie inside it: the folder
/// itself when `focused` is in its subtree in the rows before the collapse, as Windows
/// TreeView does. `None` means focus was elsewhere; the caller then uses `tab_stop`.
pub fn focus_after_collapse<Id: Copy + Eq>(
    rows_before: &[TreeRow<Id>],
    focused: Option<Id>,
    folder: Id,
) -> Option<Id> {
    let focused = focused?;
    let start = rows_before.iter().position(|row| row.id == folder)?;
    let depth = rows_before[start].depth;
    rows_before[start + 1..]
        .iter()
        .take_while(|row| row.depth > depth)
        .any(|row| row.id == focused)
        .then_some(folder)
}

/// The index of the Tab stop row, following `tab_stop`'s fallback order.
fn stop_index<Id: Copy + Eq>(
    rows: &[TreeRow<Id>],
    focused: Option<Id>,
    active: Option<Id>,
) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let position = |id: Option<Id>| id.and_then(|id| rows.iter().position(|row| row.id == id));
    position(focused).or_else(|| position(active)).or(Some(0))
}

/// Left collapses an expanded folder in place; on any other row it moves to the parent folder.
fn left<Id: Copy>(rows: &[TreeRow<Id>], current: usize) -> (usize, Option<FolderRequest<Id>>) {
    let row = rows[current];
    if row.kind == (RowKind::Folder { expanded: true }) {
        return (current, Some(FolderRequest::Collapse(row.id)));
    }
    (parent_index(rows, current).unwrap_or(current), None)
}

/// Right expands a collapsed folder in place, and moves into an expanded one's first child.
fn right<Id: Copy>(rows: &[TreeRow<Id>], current: usize) -> (usize, Option<FolderRequest<Id>>) {
    let row = rows[current];
    match row.kind {
        RowKind::Folder { expanded: false } => (current, Some(FolderRequest::Expand(row.id))),
        RowKind::Folder { expanded: true } => {
            (first_child_index(rows, current).unwrap_or(current), None)
        }
        RowKind::SavedPage | RowKind::OpenTab => (current, None),
    }
}

/// The nearest earlier row one level up: the parent, since a folder precedes its contents.
fn parent_index<Id>(rows: &[TreeRow<Id>], current: usize) -> Option<usize> {
    let depth = rows[current].depth;
    rows[..current].iter().rposition(|row| row.depth < depth)
}

/// The row right after a folder, when it is nested inside it; an empty folder has none.
fn first_child_index<Id>(rows: &[TreeRow<Id>], folder: usize) -> Option<usize> {
    let next = folder + 1;
    rows.get(next)
        .filter(|row| row.depth > rows[folder].depth)
        .map(|_| next)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic identity; the sidebar will use its saved page, folder and tab ids.
    type Id = u32;

    fn folder(id: Id, depth: usize, expanded: bool) -> TreeRow<Id> {
        TreeRow {
            id,
            depth,
            kind: RowKind::Folder { expanded },
        }
    }

    fn saved(id: Id, depth: usize) -> TreeRow<Id> {
        TreeRow {
            id,
            depth,
            kind: RowKind::SavedPage,
        }
    }

    fn tab(id: Id) -> TreeRow<Id> {
        TreeRow {
            id,
            depth: 0,
            kind: RowKind::OpenTab,
        }
    }

    /// Page 1; folder 10 { page 2; folder 20 { page 3; folder 30 (collapsed) } }; page 4;
    /// empty folder 40; open tab 100.
    fn tree() -> Vec<TreeRow<Id>> {
        vec![
            saved(1, 0),
            folder(10, 0, true),
            saved(2, 1),
            folder(20, 1, true),
            saved(3, 2),
            folder(30, 2, false),
            saved(4, 0),
            folder(40, 0, true),
            tab(100),
        ]
    }

    fn press(rows: &[TreeRow<Id>], focused: Id, key: TreeKey) -> TreeMove<Id> {
        tree_key(rows, Some(focused), None, key)
    }

    fn moved(focus: Id) -> TreeMove<Id> {
        TreeMove {
            focus: Some(focus),
            folder: None,
        }
    }

    fn requested(focus: Id, folder: FolderRequest<Id>) -> TreeMove<Id> {
        TreeMove {
            focus: Some(focus),
            folder: Some(folder),
        }
    }

    #[test]
    fn down_and_up_walk_every_visible_row_in_layout_order() {
        let rows = tree();
        let order: Vec<Id> = rows.iter().map(|row| row.id).collect();
        for pair in order.windows(2) {
            assert_eq!(press(&rows, pair[0], TreeKey::Down), moved(pair[1]));
            assert_eq!(press(&rows, pair[1], TreeKey::Up), moved(pair[0]));
        }
    }

    #[test]
    fn up_and_down_stop_at_the_ends_without_wrapping() {
        let rows = tree();
        assert_eq!(press(&rows, 1, TreeKey::Up), moved(1));
        assert_eq!(press(&rows, 100, TreeKey::Down), moved(100));
    }

    #[test]
    fn home_and_end_go_to_the_first_and_last_rows() {
        let rows = tree();
        for row in &rows {
            assert_eq!(press(&rows, row.id, TreeKey::Home), moved(1));
            assert_eq!(press(&rows, row.id, TreeKey::End), moved(100));
        }
    }

    #[test]
    fn left_collapses_an_expanded_folder_and_keeps_focus_on_it() {
        let rows = tree();
        assert_eq!(
            press(&rows, 10, TreeKey::Left),
            requested(10, FolderRequest::Collapse(10))
        );
        assert_eq!(
            press(&rows, 20, TreeKey::Left),
            requested(20, FolderRequest::Collapse(20))
        );
    }

    #[test]
    fn left_on_a_child_moves_to_its_parent_folder_at_any_depth() {
        let rows = tree();
        assert_eq!(press(&rows, 2, TreeKey::Left), moved(10));
        assert_eq!(press(&rows, 20, TreeKey::Right), moved(3));
        assert_eq!(press(&rows, 3, TreeKey::Left), moved(20));
        // A collapsed folder has nothing to collapse, so Left moves to its parent.
        assert_eq!(press(&rows, 30, TreeKey::Left), moved(20));
    }

    #[test]
    fn left_on_a_top_level_row_stays_put() {
        let rows = tree();
        for id in [1, 4, 100] {
            assert_eq!(press(&rows, id, TreeKey::Left), moved(id));
        }
        let collapsed = [folder(10, 0, false)];
        assert_eq!(press(&collapsed, 10, TreeKey::Left), moved(10));
    }

    #[test]
    fn right_expands_a_collapsed_folder_and_keeps_focus_on_it() {
        let rows = tree();
        assert_eq!(
            press(&rows, 30, TreeKey::Right),
            requested(30, FolderRequest::Expand(30))
        );
    }

    #[test]
    fn right_on_an_expanded_folder_moves_to_its_first_child() {
        let rows = tree();
        assert_eq!(press(&rows, 10, TreeKey::Right), moved(2));
        assert_eq!(press(&rows, 20, TreeKey::Right), moved(3));
    }

    #[test]
    fn right_on_an_empty_expanded_folder_or_a_page_stays_put() {
        let rows = tree();
        for id in [40, 1, 3, 100] {
            assert_eq!(press(&rows, id, TreeKey::Right), moved(id));
        }
    }

    #[test]
    fn the_tab_stop_is_the_focused_row_then_the_active_row_then_the_first() {
        let rows = tree();
        assert_eq!(tab_stop(&rows, Some(3), Some(4)), Some(3));
        assert_eq!(tab_stop(&rows, None, Some(4)), Some(4));
        assert_eq!(tab_stop(&rows, Some(999), Some(4)), Some(4));
        assert_eq!(tab_stop(&rows, Some(999), Some(998)), Some(1));
        assert_eq!(tab_stop(&rows, None, None), Some(1));
    }

    #[test]
    fn an_empty_tree_has_no_tab_stop_and_ignores_every_key() {
        let rows: Vec<TreeRow<Id>> = Vec::new();
        assert_eq!(tab_stop(&rows, Some(1), Some(1)), None);
        for key in [
            TreeKey::Up,
            TreeKey::Down,
            TreeKey::Left,
            TreeKey::Right,
            TreeKey::Home,
            TreeKey::End,
        ] {
            assert_eq!(
                tree_key(&rows, Some(1), Some(1), key),
                TreeMove {
                    focus: None,
                    folder: None
                }
            );
        }
    }

    #[test]
    fn a_row_hidden_by_collapsing_its_folder_hands_the_stop_to_the_active_row() {
        // Folder 10 collapsed by a click while page 3, inside it, held focus.
        let rows = vec![saved(1, 0), folder(10, 0, false), saved(4, 0), tab(100)];
        assert_eq!(tab_stop(&rows, Some(3), Some(100)), Some(100));
        // The next key acts from that stop, not from the hidden row.
        assert_eq!(tree_key(&rows, Some(3), Some(100), TreeKey::Up), moved(4));
        assert_eq!(tree_key(&rows, Some(3), None, TreeKey::Down), moved(10));
    }

    #[test]
    fn a_removed_row_with_no_active_row_hands_the_stop_to_the_first() {
        let mut rows = tree();
        rows.retain(|row| row.id != 4);
        assert_eq!(tab_stop(&rows, Some(4), None), Some(1));
        assert_eq!(tree_key(&rows, Some(4), None, TreeKey::End), moved(100));
    }

    #[test]
    fn focus_follows_the_identity_when_rows_reorder() {
        let mut rows = tree();
        // Page 4 moves to the top and the open tab moves above folder 40.
        let page = rows.remove(6);
        rows.insert(0, page);
        let open = rows.remove(8);
        rows.insert(7, open);
        assert_eq!(tab_stop(&rows, Some(4), Some(1)), Some(4));
        assert_eq!(press(&rows, 4, TreeKey::Down), moved(1));
        assert_eq!(press(&rows, 100, TreeKey::Down), moved(40));
        assert_eq!(press(&rows, 2, TreeKey::Left), moved(10));
    }

    #[test]
    fn keys_from_no_focus_start_at_the_active_row() {
        let rows = tree();
        assert_eq!(tree_key(&rows, None, Some(2), TreeKey::Down), moved(20));
        assert_eq!(
            tree_key(&rows, None, Some(10), TreeKey::Left),
            requested(10, FolderRequest::Collapse(10))
        );
    }

    #[test]
    fn collapsing_a_folder_brings_focus_from_its_child_or_grandchild_to_it() {
        let rows = tree();
        assert_eq!(focus_after_collapse(&rows, Some(2), 10), Some(10));
        assert_eq!(focus_after_collapse(&rows, Some(3), 10), Some(10));
        assert_eq!(focus_after_collapse(&rows, Some(30), 10), Some(10));
        assert_eq!(focus_after_collapse(&rows, Some(3), 20), Some(20));
    }

    #[test]
    fn collapsing_a_folder_leaves_focus_outside_it_to_the_tab_stop() {
        let rows = tree();
        // A later sibling, an earlier row, the folder itself and an open tab.
        for focused in [4, 1, 10, 100] {
            assert_eq!(focus_after_collapse(&rows, Some(focused), 10), None);
        }
        // A sibling of the collapsing folder is not inside it.
        assert_eq!(focus_after_collapse(&rows, Some(2), 20), None);
        assert_eq!(focus_after_collapse(&rows, None, 10), None);
    }

    #[test]
    fn collapsing_an_unknown_folder_changes_nothing() {
        assert_eq!(focus_after_collapse(&tree(), Some(2), 999), None);
        assert_eq!(focus_after_collapse(&[], Some(2), 10), None);
    }

    #[test]
    fn deep_nesting_descends_with_right_and_climbs_with_left() {
        // Folders 0 to 5, each inside the last, page 99 at the bottom, then page 50 at depth 3.
        let rows: Vec<TreeRow<Id>> = (0..6)
            .map(|level| folder(level, level as usize, true))
            .chain([saved(99, 6), saved(50, 3)])
            .collect();
        let mut focus = 0;
        for expected in (1..6).chain([99]) {
            focus = press(&rows, focus, TreeKey::Right)
                .focus
                .expect("a child row");
            assert_eq!(focus, expected);
        }
        assert_eq!(press(&rows, 99, TreeKey::Left), moved(5));
        // A shallower row after a deep subtree skips back past it to its own parent.
        assert_eq!(press(&rows, 50, TreeKey::Left), moved(2));
    }
}

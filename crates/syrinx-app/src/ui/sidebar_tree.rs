//! Wires the sidebar tree's roving focus (DW-61) into the docked sidebar and the peek popup.
//!
//! The rules are `syrinx_ui::tree_focus`; this file keeps each window's row focus handles.

use std::collections::{HashMap, HashSet};

use gpui::{App, Context, FocusHandle, KeyDownEvent, ScrollHandle, Window};
use syrinx_core::{
    SavedFolderId, SavedFolderSnapshot, SavedPageId, SavedPageSnapshot, TabId, TabModelSnapshot,
    TabSnapshot,
};
use syrinx_ui::tree_focus::{
    FolderRequest, RowKind, TreeKey, TreeRow, focus_after_collapse, tab_stop, tree_key,
};

use super::{
    BrowserView,
    sidebar_rows::{SavedRow, visible_saved_rows},
};

/// A sidebar row's stable identity for focus: its folder, saved page or open tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum TreeId {
    Folder(SavedFolderId),
    Page(SavedPageId),
    Tab(TabId),
}

/// One row the sidebar draws, already looked up in the model.
pub(super) enum SidebarEntry<'a> {
    Folder {
        folder: &'a SavedFolderSnapshot,
        depth: usize,
        expanded: bool,
    },
    Page {
        page: &'a SavedPageSnapshot,
        depth: usize,
    },
    Tab(&'a TabSnapshot),
}

impl SidebarEntry<'_> {
    /// The row as the focus rules see it.
    pub(super) fn tree_row(&self) -> TreeRow<TreeId> {
        match self {
            SidebarEntry::Folder {
                folder,
                depth,
                expanded,
            } => TreeRow {
                id: TreeId::Folder(folder.id),
                depth: *depth,
                kind: RowKind::Folder {
                    expanded: *expanded,
                },
            },
            SidebarEntry::Page { page, depth } => TreeRow {
                id: TreeId::Page(page.id),
                depth: *depth,
                kind: RowKind::SavedPage,
            },
            SidebarEntry::Tab(tab) => TreeRow {
                id: TreeId::Tab(tab.id),
                depth: 0,
                kind: RowKind::OpenTab,
            },
        }
    }
}

/// The rows the sidebar draws, in layout order: saved rows (skipping collapsed folders'
/// contents and any row whose lookup fails), then the open tabs that are not saved. The
/// focus rows are built from this same list, so they always match what is drawn.
pub(super) fn sidebar_entries<'a>(
    model: &'a TabModelSnapshot,
    collapsed: &HashSet<SavedFolderId>,
) -> Vec<SidebarEntry<'a>> {
    // Index only the visible rows: a large collapsed import must not cost a map per frame.
    let visible = visible_saved_rows(&model.saved_layout, collapsed);
    let visible_pages: HashSet<SavedPageId> = visible
        .iter()
        .filter_map(|row| match row {
            SavedRow::Page { id, .. } => Some(*id),
            SavedRow::Folder { .. } => None,
        })
        .collect();
    let pages: HashMap<SavedPageId, &SavedPageSnapshot> = model
        .saved_pages
        .iter()
        .filter(|page| visible_pages.contains(&page.id))
        .map(|page| (page.id, page))
        .collect();
    let folders: HashMap<SavedFolderId, &SavedFolderSnapshot> = model
        .saved_folders
        .iter()
        .map(|folder| (folder.id, folder))
        .collect();
    let saved = visible.into_iter().filter_map(|row| match row {
        SavedRow::Folder {
            id,
            depth,
            expanded,
        } => folders.get(&id).map(|folder| SidebarEntry::Folder {
            folder,
            depth,
            expanded,
        }),
        SavedRow::Page { id, depth } => pages
            .get(&id)
            .map(|page| SidebarEntry::Page { page, depth }),
    });
    let open = model
        .tabs
        .iter()
        .filter(|tab| tab.saved_page_id.is_none())
        .map(SidebarEntry::Tab);
    saved.chain(open).collect()
}

/// The row the active tab shows as: its saved page when it has one, else the tab itself.
pub(super) fn active_tree_id(model: &TabModelSnapshot) -> Option<TreeId> {
    let active = model.active_id?;
    let tab = model.tabs.iter().find(|tab| tab.id == active)?;
    Some(match tab.saved_page_id {
        Some(saved) => TreeId::Page(saved),
        None => TreeId::Tab(tab.id),
    })
}

/// The tree key an unmodified key press stands for. Alt arrows move rows, so any modifier
/// leaves the key to the row's other bindings.
pub(super) fn tree_key_for(key: &str, modified: bool) -> Option<TreeKey> {
    if modified {
        return None;
    }
    match key {
        "up" => Some(TreeKey::Up),
        "down" => Some(TreeKey::Down),
        "left" => Some(TreeKey::Left),
        "right" => Some(TreeKey::Right),
        "home" => Some(TreeKey::Home),
        "end" => Some(TreeKey::End),
        _ => None,
    }
}

/// Where focus goes after a redraw that removed the focused row (collapsed away, closed or
/// deleted): the new Tab stop. `None` when the focused row is still drawn or none was focused.
pub(super) fn refocus_after_redraw(
    rows: &[TreeRow<TreeId>],
    focused: Option<TreeId>,
    active: Option<TreeId>,
) -> Option<TreeId> {
    let focused = focused?;
    if rows.iter().any(|row| row.id == focused) {
        return None;
    }
    tab_stop(rows, Some(focused), active)
}

/// One window's tree: a focus handle per drawn row, the rows as last drawn, and the scroll
/// position that keeps a focused row in view.
#[derive(Default)]
pub(super) struct WindowTree {
    handles: HashMap<TreeId, FocusHandle>,
    rows: Vec<TreeRow<TreeId>>,
    scroll: ScrollHandle,
}

/// What drawing one frame of rows needs: each row's focus handle, in row order, and a row to
/// move focus to because the focused one has gone.
pub(super) struct PreparedRows {
    pub(super) handles: Vec<FocusHandle>,
    pub(super) refocus: Option<(TreeId, TreeId)>,
    pub(super) scroll: ScrollHandle,
}

impl WindowTree {
    /// Records the rows about to be drawn and hands out their focus handles, with only the
    /// Tab stop row reachable by Tab.
    fn prepare(
        &mut self,
        rows: Vec<TreeRow<TreeId>>,
        active: Option<TreeId>,
        window: &Window,
        cx: &mut App,
    ) -> PreparedRows {
        let focused = self.focused(window);
        let stop = tab_stop(&rows, focused, active);
        let refocus = refocus_after_redraw(&rows, focused, active)
            .zip(focused)
            .map(|(target, lost)| (lost, target));
        // Keep a vanished focused row's handle until the refocus has run, so the window
        // never holds a released focus id.
        let drawn: HashSet<TreeId> = rows.iter().map(|row| row.id).chain(focused).collect();
        self.handles.retain(|id, _| drawn.contains(id));
        let handles = rows
            .iter()
            .map(|row| {
                let handle = self
                    .handles
                    .entry(row.id)
                    .or_insert_with(|| cx.focus_handle());
                // A tracked handle decides its own Tab stop; `tab_index` would not.
                *handle = handle.clone().tab_stop(Some(row.id) == stop);
                handle.clone()
            })
            .collect();
        self.rows = rows;
        PreparedRows {
            handles,
            refocus,
            scroll: self.scroll.clone(),
        }
    }

    /// The row whose handle holds this window's focus.
    fn focused(&self, window: &Window) -> Option<TreeId> {
        self.handles
            .iter()
            .find(|(_, handle)| handle.is_focused(window))
            .map(|(id, _)| *id)
    }

    /// Focuses a drawn row and scrolls it into view; false when the row is not drawn.
    fn focus(&self, id: TreeId, window: &mut Window, cx: &mut App) -> bool {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            return false;
        };
        let Some(handle) = self.handles.get(&id) else {
            return false;
        };
        window.focus(handle, cx);
        self.scroll.scroll_to_item(index);
        true
    }
}

/// The docked sidebar's tree and the peek's, kept apart so no focus handle is ever tracked
/// in two windows (the accessibility tree panics on a doubled focus claim).
#[derive(Default)]
pub(super) struct SidebarTrees {
    main: WindowTree,
    peek: WindowTree,
}

impl SidebarTrees {
    fn window(&self, peek: bool) -> &WindowTree {
        if peek { &self.peek } else { &self.main }
    }

    fn window_mut(&mut self, peek: bool) -> &mut WindowTree {
        if peek { &mut self.peek } else { &mut self.main }
    }

    /// Prepares one window's tree for the rows about to be drawn, and moves focus off a
    /// focused row that is no longer among them once the frame is done.
    pub(super) fn prepare(
        &mut self,
        peek: bool,
        rows: Vec<TreeRow<TreeId>>,
        active: Option<TreeId>,
        window: &Window,
        cx: &mut Context<BrowserView>,
    ) -> PreparedRows {
        let prepared = self.window_mut(peek).prepare(rows, active, window, cx);
        if let Some((lost, target)) = prepared.refocus {
            schedule_refocus(lost, target, window, cx);
        }
        prepared
    }
}

impl BrowserView {
    /// Whether `window` draws the peek's tree: rows draw only there or in the main window,
    /// and the peek's first frame comes before `self.peek` records its handle.
    pub(super) fn draws_peek_tree(&self, window: &Window) -> bool {
        Window::window_handle(window) != self.main_window
    }

    /// Up, Down, Home, End, Left and Right on a focused row: move focus and expand or collapse
    /// a folder as `syrinx_ui::tree_focus` decides. Other keys go on to the row's bindings.
    pub(super) fn tree_key_down(
        &mut self,
        row: TreeId,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let Some(key) = tree_key_for(&keystroke.key, keystroke.modifiers.modified()) else {
            return;
        };
        cx.stop_propagation();
        let peek = self.draws_peek_tree(window);
        let rows = &self.sidebar_trees.window(peek).rows;
        let step = tree_key(rows, Some(row), active_tree_id(&self.model), key);
        // A collapse brings focus from inside the folder to the folder itself.
        let target = match step.folder {
            Some(FolderRequest::Collapse(folder)) => {
                focus_after_collapse(rows, Some(row), folder).or(step.focus)
            }
            _ => step.focus,
        };
        match step.folder {
            Some(FolderRequest::Collapse(TreeId::Folder(folder))) => self.collapse_folder(folder),
            Some(FolderRequest::Expand(TreeId::Folder(folder))) => self.expand_folder(folder),
            _ => {}
        }
        if let Some(target) = target {
            self.sidebar_trees.window(peek).focus(target, window, cx);
        }
        cx.notify();
    }

    /// F6 into the docked sidebar lands on the active row (else the first); false when the
    /// sidebar has no rows, so F6 falls back to its first control.
    pub(super) fn focus_active_row(&self, window: &mut Window, cx: &mut App) -> bool {
        let tree = self.sidebar_trees.window(false);
        let Some(target) = tab_stop(&tree.rows, None, active_tree_id(&self.model)) else {
            return false;
        };
        tree.focus(target, window, cx)
    }

    /// Moves focus from a row that vanished to the new Tab stop, unless focus has moved on.
    fn refocus_lost_row(
        &mut self,
        lost: TreeId,
        target: TreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let peek = self.draws_peek_tree(window);
        let tree = self.sidebar_trees.window(peek);
        if tree.focused(window) != Some(lost) {
            return;
        }
        if !tree.focus(target, window, cx) {
            self.release_row_focus(window, cx);
        }
        cx.notify();
    }
}

/// Runs the refocus after the current frame, in the window that drew it: focus cannot move
/// during a draw, and the peek's rows belong to the peek's window, not the browser's.
fn schedule_refocus(lost: TreeId, target: TreeId, window: &Window, cx: &mut Context<BrowserView>) {
    let view = cx.weak_entity();
    window.defer(cx, move |window, cx| {
        let updated = view.update(cx, |browser, cx| {
            browser.refocus_lost_row(lost, target, window, cx);
        });
        // A browser already gone (Syrinx closing) leaves nothing to keep focused.
        if updated.is_err() {
            window.blur(cx);
        }
    });
}

#[cfg(test)]
mod tests {
    use syrinx_core::{SavedEntryId, SavedPlacement, TabKind, TabMetadata};

    use super::*;

    fn page_id(id: u64) -> SavedPageId {
        SavedPageId::new(id).expect("synthetic saved page id")
    }

    fn folder_id(id: u64) -> SavedFolderId {
        SavedFolderId::new(id).expect("synthetic folder id")
    }

    fn tab_id(id: u64) -> TabId {
        TabId::new(id).expect("synthetic tab id")
    }

    fn metadata(title: &str) -> TabMetadata {
        TabMetadata {
            title: title.to_owned(),
            address: format!("https://{title}.example.test/"),
            favicon: None,
            badge: None,
        }
    }

    fn tab(id: u64, saved: Option<u64>) -> TabSnapshot {
        TabSnapshot {
            id: tab_id(id),
            kind: TabKind::Web,
            metadata: metadata("tab"),
            saved_page_id: saved.map(page_id),
        }
    }

    fn placed(entry: SavedEntryId, parent: Option<u64>) -> SavedPlacement {
        SavedPlacement {
            entry,
            parent: parent.map(folder_id),
        }
    }

    /// Page 1; folder 10 { page 2 }; open tab 100 showing page 1; ordinary tab 200.
    fn model() -> TabModelSnapshot {
        TabModelSnapshot {
            tabs: vec![tab(100, Some(1)), tab(200, None)],
            saved_pages: vec![
                SavedPageSnapshot {
                    id: page_id(1),
                    metadata: metadata("one"),
                },
                SavedPageSnapshot {
                    id: page_id(2),
                    metadata: metadata("two"),
                },
            ],
            saved_folders: vec![SavedFolderSnapshot {
                id: folder_id(10),
                title: "Folder".to_owned(),
            }],
            saved_layout: vec![
                placed(SavedEntryId::Page(page_id(1)), None),
                placed(SavedEntryId::Folder(folder_id(10)), None),
                placed(SavedEntryId::Page(page_id(2)), Some(10)),
            ],
            active_id: Some(tab_id(100)),
            ..TabModelSnapshot::default()
        }
    }

    fn tree(model: &TabModelSnapshot, collapsed: &HashSet<SavedFolderId>) -> Vec<TreeRow<TreeId>> {
        sidebar_entries(model, collapsed)
            .iter()
            .map(SidebarEntry::tree_row)
            .collect()
    }

    #[test]
    fn focus_rows_follow_the_drawn_rows_in_layout_order() {
        let rows = tree(&model(), &HashSet::new());
        let expected = [
            (TreeId::Page(page_id(1)), 0, RowKind::SavedPage),
            (
                TreeId::Folder(folder_id(10)),
                0,
                RowKind::Folder { expanded: true },
            ),
            (TreeId::Page(page_id(2)), 1, RowKind::SavedPage),
            (TreeId::Tab(tab_id(200)), 0, RowKind::OpenTab),
        ];
        let actual: Vec<_> = rows
            .iter()
            .map(|row| (row.id, row.depth, row.kind))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn a_collapsed_folder_hides_its_contents_from_focus() {
        let collapsed = HashSet::from([folder_id(10)]);
        let rows = tree(&model(), &collapsed);
        let ids: Vec<TreeId> = rows.iter().map(|row| row.id).collect();
        assert_eq!(
            ids,
            [
                TreeId::Page(page_id(1)),
                TreeId::Folder(folder_id(10)),
                TreeId::Tab(tab_id(200)),
            ]
        );
        assert_eq!(rows[1].kind, RowKind::Folder { expanded: false });
    }

    #[test]
    fn a_row_whose_lookup_fails_is_neither_drawn_nor_focusable() {
        let mut model = model();
        model.saved_pages.retain(|page| page.id != page_id(2));
        model.saved_folders.clear();
        let ids: Vec<TreeId> = tree(&model, &HashSet::new())
            .iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(ids, [TreeId::Page(page_id(1)), TreeId::Tab(tab_id(200))]);
    }

    #[test]
    fn the_active_row_is_the_saved_page_a_live_tab_shows_or_the_tab_itself() {
        let mut model = model();
        assert_eq!(active_tree_id(&model), Some(TreeId::Page(page_id(1))));
        model.active_id = Some(tab_id(200));
        assert_eq!(active_tree_id(&model), Some(TreeId::Tab(tab_id(200))));
        model.active_id = None;
        assert_eq!(active_tree_id(&model), None);
        model.active_id = Some(tab_id(999));
        assert_eq!(active_tree_id(&model), None);
    }

    #[test]
    fn f6_lands_on_the_active_row_rather_than_the_first() {
        let model = model();
        let rows = tree(&model, &HashSet::new());
        assert_eq!(
            tab_stop(&rows, None, active_tree_id(&model)),
            Some(TreeId::Page(page_id(1)))
        );
        let mut later = model.clone();
        later.active_id = Some(tab_id(200));
        assert_eq!(
            tab_stop(&rows, None, active_tree_id(&later)),
            Some(TreeId::Tab(tab_id(200)))
        );
    }

    #[test]
    fn only_unmodified_navigation_keys_drive_the_tree() {
        let keys = [
            ("up", TreeKey::Up),
            ("down", TreeKey::Down),
            ("left", TreeKey::Left),
            ("right", TreeKey::Right),
            ("home", TreeKey::Home),
            ("end", TreeKey::End),
        ];
        for (name, key) in keys {
            assert_eq!(tree_key_for(name, false), Some(key), "{name}");
            // Alt arrows move rows and Shift or Ctrl keep their own meanings.
            assert_eq!(tree_key_for(name, true), None, "{name}");
        }
        for other in ["enter", "space", "tab", "delete", "f2", "a"] {
            assert_eq!(tree_key_for(other, false), None, "{other}");
        }
    }

    #[test]
    fn a_focused_row_that_vanishes_hands_focus_to_the_new_tab_stop() {
        let model = model();
        let active = active_tree_id(&model);
        let collapsed = HashSet::from([folder_id(10)]);
        let rows = tree(&model, &collapsed);
        // Page 2 held focus when its folder collapsed: focus goes to the active row.
        assert_eq!(
            refocus_after_redraw(&rows, Some(TreeId::Page(page_id(2))), active),
            Some(TreeId::Page(page_id(1)))
        );
        // A closed tab with no active row hands focus to the first row.
        assert_eq!(
            refocus_after_redraw(&rows, Some(TreeId::Tab(tab_id(300))), None),
            Some(TreeId::Page(page_id(1)))
        );
    }

    #[test]
    fn a_focused_row_that_is_still_drawn_or_no_focus_needs_no_refocus() {
        let model = model();
        let rows = tree(&model, &HashSet::new());
        let active = active_tree_id(&model);
        assert_eq!(
            refocus_after_redraw(&rows, Some(TreeId::Page(page_id(2))), active),
            None
        );
        assert_eq!(refocus_after_redraw(&rows, None, active), None);
        assert_eq!(
            refocus_after_redraw(&[], Some(TreeId::Page(page_id(2))), active),
            None
        );
    }

    #[test]
    fn collapsing_a_folder_from_inside_it_by_key_focuses_the_folder() {
        let model = model();
        let rows = tree(&model, &HashSet::new());
        // Left on page 2 moves to folder 10; Left again collapses it in place.
        let step = tree_key(&rows, Some(TreeId::Page(page_id(2))), None, TreeKey::Left);
        assert_eq!(step.focus, Some(TreeId::Folder(folder_id(10))));
        let step = tree_key(
            &rows,
            Some(TreeId::Folder(folder_id(10))),
            None,
            TreeKey::Left,
        );
        assert_eq!(
            step.folder,
            Some(FolderRequest::Collapse(TreeId::Folder(folder_id(10))))
        );
        assert_eq!(
            focus_after_collapse(
                &rows,
                Some(TreeId::Page(page_id(2))),
                TreeId::Folder(folder_id(10))
            ),
            Some(TreeId::Folder(folder_id(10)))
        );
    }
}

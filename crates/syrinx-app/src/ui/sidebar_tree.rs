//! Wires the sidebar tree's roving focus (DW-61) into the docked sidebar and the peek popup.
//!
//! The rules are `syrinx_ui::tree_focus`; this file keeps each window's row focus handles.

use std::collections::{HashMap, HashSet};

use gpui::{App, Context, FocusHandle, KeyDownEvent, Pixels, Point, ScrollHandle, Window};
use syrinx_core::{
    SavedFolderId, SavedFolderSnapshot, SavedPageId, SavedPageSnapshot, TabId, TabModelSnapshot,
    TabSnapshot,
};
use syrinx_ui::tree_focus::{FolderRequest, RowKind, TreeKey, TreeRow, tab_stop, tree_key};

use super::{
    BrowserView,
    row_menu::TabAction,
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

/// The open tab showing a saved page, if the page is live.
pub(super) fn live_tab(model: &TabModelSnapshot, page: SavedPageId) -> Option<TabId> {
    model
        .tabs
        .iter()
        .find(|tab| tab.saved_page_id == Some(page))
        .map(|tab| tab.id)
}

/// The live tab Ctrl W closes from a focused row (DW-63): a saved page's open tab, which
/// unloads the page while its saved row stays, or the open tab itself. A folder, or a saved
/// page with no live tab, closes nothing, and the key then does nothing rather than fall
/// through to the window's Ctrl W: the active page may belong to a different row.
pub(super) fn row_close_target(model: &TabModelSnapshot, row: TreeId) -> Option<TabId> {
    match row {
        TreeId::Folder(_) => None,
        TreeId::Page(page) => live_tab(model, page),
        TreeId::Tab(id) => model.tabs.iter().any(|tab| tab.id == id).then_some(id),
    }
}

/// The close control a row offers, which the wide row's × and the compact row's corner badge
/// (#128) both draw. It is built on `row_close_target`, so the control and Ctrl W never
/// disagree about what a row closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowClose {
    /// Close an open tab; its row goes with it.
    Tab(TabId),
    /// Unload a saved page's live tab; the saved row stays.
    Unload(TabId),
}

impl RowClose {
    /// The tab `SessionCommand::CloseTab` takes.
    pub(super) fn tab(self) -> TabId {
        match self {
            RowClose::Tab(tab) | RowClose::Unload(tab) => tab,
        }
    }

    /// The control's accessible name.
    pub(super) fn name(self) -> &'static str {
        match self {
            RowClose::Tab(_) => "Close tab",
            RowClose::Unload(_) => "Unload page",
        }
    }

    /// The row keys that do the same, in `aria-keyshortcuts` form. Delete closes only an
    /// ordinary tab's row: on a saved row it removes the saved page itself.
    pub(super) fn keys(self) -> &'static str {
        match self {
            RowClose::Tab(_) => "Control+W Delete",
            RowClose::Unload(_) => "Control+W",
        }
    }

    /// The row action the control and Ctrl W run.
    pub(super) fn action(self) -> TabAction {
        match self {
            RowClose::Tab(tab) => TabAction::Close(tab),
            RowClose::Unload(tab) => TabAction::Unload(tab),
        }
    }

    /// Whether the row is still drawn after the close, so a click must let go of row focus
    /// rather than leave the row's keys (Alt arrows, Delete) armed.
    pub(super) fn row_stays(self) -> bool {
        matches!(self, RowClose::Unload(_))
    }
}

/// The close control for `row`: none for a folder, an unopened saved page or a tab that has
/// already gone.
pub(super) fn row_close(model: &TabModelSnapshot, row: TreeId) -> Option<RowClose> {
    let tab = row_close_target(model, row)?;
    match row {
        TreeId::Tab(_) => Some(RowClose::Tab(tab)),
        TreeId::Page(_) => Some(RowClose::Unload(tab)),
        // A folder has no live page, so `row_close_target` has already said no.
        TreeId::Folder(_) => None,
    }
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

/// A focused row that a redraw no longer contains (its tab closed, its saved page deleted,
/// or its tab saved, which turns a tab row into a saved row), and where focus goes instead:
/// the new Tab stop, or nowhere when the tree is now empty, so the row lets go of focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Refocus {
    pub(super) lost: TreeId,
    pub(super) target: Option<TreeId>,
}

/// What one frame of rows does with focus.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct FramePlan {
    /// The one row reachable by Tab.
    pub(super) stop: Option<TreeId>,
    pub(super) refocus: Option<Refocus>,
    /// The rows whose focus handles survive the frame: the drawn rows, and a vanished
    /// focused row until its refocus has moved focus off it, so the window never holds a
    /// released focus id.
    pub(super) keep: HashSet<TreeId>,
}

/// Decides one frame: its Tab stop, any refocus, and which focus handles to keep.
pub(super) fn plan_frame(
    rows: &[TreeRow<TreeId>],
    focused: Option<TreeId>,
    active: Option<TreeId>,
) -> FramePlan {
    let stop = tab_stop(rows, focused, active);
    let refocus = focused
        .filter(|focused| !rows.iter().any(|row| row.id == *focused))
        .map(|lost| Refocus { lost, target: stop });
    let keep = rows
        .iter()
        .map(|row| row.id)
        .chain(refocus.map(|refocus| refocus.lost))
        .collect();
    FramePlan {
        stop,
        refocus,
        keep,
    }
}

/// A folder change a tree key asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FolderChange {
    Expand(SavedFolderId),
    Collapse(SavedFolderId),
}

/// What a tree key on a focused row does: the row to focus and any folder change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct KeyStep {
    pub(super) focus: Option<TreeId>,
    pub(super) folder: Option<FolderChange>,
}

/// The key's step from `row`. `tree_key` only asks to collapse the focused row itself (an
/// open folder), so focus never sits inside a folder this collapses and
/// `focus_after_collapse` has nothing to do here.
pub(super) fn key_step(
    rows: &[TreeRow<TreeId>],
    row: TreeId,
    active: Option<TreeId>,
    key: TreeKey,
) -> KeyStep {
    let step = tree_key(rows, Some(row), active, key);
    // Only folder rows ask for a change, so any other request cannot arise.
    let folder = match step.folder {
        Some(FolderRequest::Expand(TreeId::Folder(id))) => Some(FolderChange::Expand(id)),
        Some(FolderRequest::Collapse(TreeId::Folder(id))) => Some(FolderChange::Collapse(id)),
        _ => None,
    };
    KeyStep {
        focus: step.focus,
        folder,
    }
}

/// Where F6 lands in the sidebar: the active row, else the first. The row focused last does
/// not count, because F6 always arrives from outside the tree.
pub(super) fn f6_target(rows: &[TreeRow<TreeId>], active: Option<TreeId>) -> Option<TreeId> {
    tab_stop(rows, None, active)
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
    pub(super) refocus: Option<Refocus>,
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
        let plan = plan_frame(&rows, self.focused(window), active);
        self.handles.retain(|id, _| plan.keep.contains(id));
        let handles = rows
            .iter()
            .map(|row| {
                let handle = self
                    .handles
                    .entry(row.id)
                    .or_insert_with(|| cx.focus_handle());
                // A tracked handle decides its own Tab stop; `tab_index` would not.
                *handle = handle.clone().tab_stop(Some(row.id) == plan.stop);
                handle.clone()
            })
            .collect();
        self.rows = rows;
        PreparedRows {
            handles,
            refocus: plan.refocus,
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
        if let Some(refocus) = prepared.refocus {
            schedule_refocus(refocus, window, cx);
        }
        prepared
    }

    /// The middle of `id`'s row as the peek last laid it out, in the popup's coordinates;
    /// the palette acceptance run clicks there.
    pub(super) fn peek_row_center(&self, id: TreeId) -> Option<Point<Pixels>> {
        let index = self.peek.rows.iter().position(|row| row.id == id)?;
        let bounds = self.peek.scroll.bounds_for_item(index)?;
        Some(bounds.center() + self.peek.scroll.offset())
    }

    /// The bottom-left corner of `id`'s row as the docked sidebar last laid it out, where a
    /// row menu opened by a key hangs (#95).
    pub(super) fn main_row_bottom_left(&self, id: TreeId) -> Option<Point<Pixels>> {
        let index = self.main.rows.iter().position(|row| row.id == id)?;
        let bounds = self.main.scroll.bounds_for_item(index)?;
        Some(bounds.bottom_left() + self.main.scroll.offset())
    }

    /// Focuses a row of the docked sidebar; false when it is not drawn there.
    pub(super) fn focus_main_row(&self, id: TreeId, window: &mut Window, cx: &mut App) -> bool {
        self.main.focus(id, window, cx)
    }
}

impl BrowserView {
    /// Whether `window` draws the peek's tree. Only the main window and the peek draw the
    /// sidebar, and the peek's first frame comes before `self.peek` records its handle.
    pub(super) fn draws_peek_tree(&self, window: &Window) -> bool {
        !self.is_main_window(window)
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
        let step = key_step(rows, row, active_tree_id(&self.model), key);
        match step.folder {
            Some(FolderChange::Collapse(folder)) => self.collapse_folder(folder),
            Some(FolderChange::Expand(folder)) => self.expand_folder(folder),
            None => {}
        }
        if let Some(target) = step.focus {
            self.sidebar_trees.window(peek).focus(target, window, cx);
        }
        cx.notify();
    }

    /// Ctrl W on a focused row closes the row's live page, if it has one (DW-63). An unloaded
    /// saved row stays drawn and keeps focus; a closed tab's row vanishes, and the next frame's
    /// `plan_frame` moves focus to the new Tab stop, as after Delete.
    pub(super) fn close_row_page(
        &mut self,
        row: TreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(close) = row_close(&self.model, row) else {
            return;
        };
        self.run_tab_action(close.action(), None, window, cx);
    }

    /// F6 into the docked sidebar lands on the active row (else the first); false when the
    /// sidebar has no rows, so F6 falls back to its first control.
    pub(super) fn focus_active_row(&self, window: &mut Window, cx: &mut App) -> bool {
        let tree = self.sidebar_trees.window(false);
        let Some(target) = f6_target(&tree.rows, active_tree_id(&self.model)) else {
            return false;
        };
        tree.focus(target, window, cx)
    }

    /// Moves focus from a row that vanished to the new Tab stop, unless focus has moved on.
    /// With no row left (or the target gone too), the row lets go of focus to the window
    /// root, so the window's own keys (Ctrl T, Ctrl K, Tab, F6) keep working.
    fn refocus_lost_row(&mut self, refocus: Refocus, window: &mut Window, cx: &mut Context<Self>) {
        let peek = self.draws_peek_tree(window);
        let tree = self.sidebar_trees.window(peek);
        if tree.focused(window) != Some(refocus.lost) {
            return;
        }
        let moved = refocus
            .target
            .is_some_and(|target| tree.focus(target, window, cx));
        if !moved {
            self.release_row_focus(window, cx);
        }
        cx.notify();
    }
}

/// Runs the refocus after the current frame, in the window that drew it: focus cannot move
/// during a draw, and the peek's rows belong to the peek's window, not the browser's.
fn schedule_refocus(refocus: Refocus, window: &Window, cx: &mut Context<BrowserView>) {
    let view = cx.weak_entity();
    window.defer(cx, move |window, cx| {
        let updated = view.update(cx, |browser, cx| {
            browser.refocus_lost_row(refocus, window, cx);
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

    fn ids(rows: &[TreeRow<TreeId>]) -> HashSet<TreeId> {
        rows.iter().map(|row| row.id).collect()
    }

    #[test]
    fn f6_lands_on_the_active_row_else_the_first_else_nowhere() {
        let model = model();
        let rows = tree(&model, &HashSet::new());
        assert_eq!(
            f6_target(&rows, active_tree_id(&model)),
            Some(TreeId::Page(page_id(1)))
        );
        let mut later = model.clone();
        later.active_id = Some(tab_id(200));
        assert_eq!(
            f6_target(&rows, active_tree_id(&later)),
            Some(TreeId::Tab(tab_id(200)))
        );
        // The active page hidden inside a collapsed folder: the first row instead.
        let collapsed = tree(&model, &HashSet::from([folder_id(10)]));
        assert_eq!(
            f6_target(&collapsed, Some(TreeId::Page(page_id(2)))),
            Some(TreeId::Page(page_id(1)))
        );
        // An empty tree: F6 falls back to the sidebar's first control.
        assert_eq!(f6_target(&[], active_tree_id(&model)), None);
    }

    #[test]
    fn every_frame_has_exactly_one_tab_stop_among_its_drawn_rows() {
        let model = model();
        let rows = tree(&model, &HashSet::new());
        let active = active_tree_id(&model);
        let vanished = Some(TreeId::Tab(tab_id(300)));
        let focuses = rows.iter().map(|row| Some(row.id)).chain([None, vanished]);
        for focused in focuses {
            let plan = plan_frame(&rows, focused, active);
            let stops = rows.iter().filter(|row| Some(row.id) == plan.stop).count();
            assert_eq!(stops, 1, "{focused:?}");
        }
    }

    #[test]
    fn a_focused_row_still_drawn_keeps_the_stop_and_needs_no_refocus() {
        let model = model();
        let rows = tree(&model, &HashSet::new());
        let plan = plan_frame(
            &rows,
            Some(TreeId::Page(page_id(2))),
            active_tree_id(&model),
        );
        assert_eq!(plan.stop, Some(TreeId::Page(page_id(2))));
        assert_eq!(plan.refocus, None);
        assert_eq!(plan.keep, ids(&rows));
        // With nothing focused the active row is the stop.
        let plan = plan_frame(&rows, None, active_tree_id(&model));
        assert_eq!(plan.stop, Some(TreeId::Page(page_id(1))));
        assert_eq!(plan.refocus, None);
    }

    #[test]
    fn a_vanished_focused_row_keeps_its_handle_only_until_focus_moves_off_it() {
        let model = model();
        let active = active_tree_id(&model);
        let rows = tree(&model, &HashSet::from([folder_id(10)]));
        let lost = TreeId::Page(page_id(2));
        let plan = plan_frame(&rows, Some(lost), active);
        assert_eq!(
            plan.refocus,
            Some(Refocus {
                lost,
                target: Some(TreeId::Page(page_id(1)))
            })
        );
        assert!(plan.keep.contains(&lost));
        // Once the refocus has run, the next frame drops the vanished row's handle.
        let next = plan_frame(&rows, Some(TreeId::Page(page_id(1))), active);
        assert_eq!(next.refocus, None);
        assert_eq!(next.keep, ids(&rows));
    }

    #[test]
    fn rows_no_longer_drawn_lose_their_handles() {
        let model = model();
        let rows = tree(&model, &HashSet::from([folder_id(10)]));
        let plan = plan_frame(
            &rows,
            Some(TreeId::Tab(tab_id(200))),
            active_tree_id(&model),
        );
        assert!(!plan.keep.contains(&TreeId::Page(page_id(2))));
        assert_eq!(plan.keep, ids(&rows));
    }

    #[test]
    fn an_emptied_tree_releases_focus_instead_of_keeping_the_vanished_row() {
        // The only open tab was focused and closed: no row is left to take focus.
        let lost = TreeId::Tab(tab_id(200));
        let plan = plan_frame(&[], Some(lost), None);
        assert_eq!(plan.stop, None);
        assert_eq!(plan.refocus, Some(Refocus { lost, target: None }));
        assert_eq!(plan.keep, HashSet::from([lost]));
        // After the release nothing is focused, and the handle goes.
        let next = plan_frame(&[], None, None);
        assert_eq!(next.refocus, None);
        assert!(next.keep.is_empty());
    }

    #[test]
    fn ctrl_w_on_a_live_saved_row_unloads_its_tab() {
        // Page 1 is live in tab 100 and not the only row, so the saved row stays.
        assert_eq!(
            row_close_target(&model(), TreeId::Page(page_id(1))),
            Some(tab_id(100))
        );
    }

    #[test]
    fn ctrl_w_on_an_open_tab_row_closes_that_tab() {
        assert_eq!(
            row_close_target(&model(), TreeId::Tab(tab_id(200))),
            Some(tab_id(200))
        );
        // A tab already closed by the time the key arrives closes nothing else.
        assert_eq!(row_close_target(&model(), TreeId::Tab(tab_id(300))), None);
    }

    #[test]
    fn ctrl_w_on_an_unloaded_saved_row_or_a_folder_closes_nothing() {
        let model = model();
        // Page 2 has no live tab, though page 1's tab is active: the active page stays open.
        assert_eq!(model.active_id, Some(tab_id(100)));
        assert_eq!(row_close_target(&model, TreeId::Page(page_id(2))), None);
        assert_eq!(
            row_close_target(&model, TreeId::Folder(folder_id(10))),
            None
        );
    }

    #[test]
    fn the_close_control_closes_an_open_tab_row() {
        let close = row_close(&model(), TreeId::Tab(tab_id(200)))
            .expect("an open tab's row offers Close tab");
        assert_eq!(close, RowClose::Tab(tab_id(200)));
        assert_eq!(close.tab(), tab_id(200));
        assert_eq!(close.name(), "Close tab");
        assert_eq!(close.keys(), "Control+W Delete");
        assert!(!close.row_stays());
    }

    #[test]
    fn the_close_control_unloads_a_live_saved_row() {
        let close = row_close(&model(), TreeId::Page(page_id(1)))
            .expect("a live saved page's row offers Unload page");
        assert_eq!(close, RowClose::Unload(tab_id(100)));
        assert_eq!(close.tab(), tab_id(100));
        assert_eq!(close.name(), "Unload page");
        assert_eq!(close.keys(), "Control+W");
        // The saved row stays after the unload, so a click must let go of row focus.
        assert!(close.row_stays());
    }

    #[test]
    fn an_unopened_saved_row_a_folder_and_a_closed_tab_offer_no_close_control() {
        let model = model();
        assert_eq!(row_close(&model, TreeId::Page(page_id(2))), None);
        assert_eq!(row_close(&model, TreeId::Folder(folder_id(10))), None);
        assert_eq!(row_close(&model, TreeId::Tab(tab_id(300))), None);
    }

    #[test]
    fn the_close_control_and_ctrl_w_agree_on_every_row() {
        let model = model();
        // Every row the sidebar draws, and a tab that has already gone.
        let rows = tree(&model, &HashSet::new())
            .into_iter()
            .map(|row| row.id)
            .chain([TreeId::Tab(tab_id(300))]);
        for row in rows {
            assert_eq!(
                row_close(&model, row).map(RowClose::tab),
                row_close_target(&model, row),
                "{row:?}"
            );
        }
    }

    #[test]
    fn left_and_right_open_close_and_cross_folders() {
        let model = model();
        let active = active_tree_id(&model);
        let rows = tree(&model, &HashSet::new());
        let folder = TreeId::Folder(folder_id(10));
        let inside = TreeId::Page(page_id(2));
        let step = |rows: &[TreeRow<TreeId>], row, key| key_step(rows, row, active, key);
        assert_eq!(
            step(&rows, inside, TreeKey::Left),
            KeyStep {
                focus: Some(folder),
                folder: None
            }
        );
        assert_eq!(
            step(&rows, folder, TreeKey::Left),
            KeyStep {
                focus: Some(folder),
                folder: Some(FolderChange::Collapse(folder_id(10)))
            }
        );
        assert_eq!(
            step(&rows, folder, TreeKey::Right),
            KeyStep {
                focus: Some(inside),
                folder: None
            }
        );
        let collapsed = tree(&model, &HashSet::from([folder_id(10)]));
        assert_eq!(
            step(&collapsed, folder, TreeKey::Right),
            KeyStep {
                focus: Some(folder),
                folder: Some(FolderChange::Expand(folder_id(10)))
            }
        );
    }

    #[test]
    fn keys_on_pages_and_tabs_move_focus_without_folder_changes() {
        let model = model();
        let active = active_tree_id(&model);
        let rows = tree(&model, &HashSet::new());
        let last = TreeId::Tab(tab_id(200));
        let keys = [
            (TreeKey::Up, TreeId::Page(page_id(2))),
            (TreeKey::Down, last),
            (TreeKey::Home, TreeId::Page(page_id(1))),
            (TreeKey::End, last),
            (TreeKey::Left, last),
            (TreeKey::Right, last),
        ];
        for (key, focus) in keys {
            assert_eq!(
                key_step(&rows, last, active, key),
                KeyStep {
                    focus: Some(focus),
                    folder: None
                },
                "{key:?}"
            );
        }
    }
}

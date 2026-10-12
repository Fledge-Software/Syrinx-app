//! Draws the sidebar at its current D44 width and lets the owner drag its edge (P1-08, P1-09),
//! with the saved rows' folder controls (P1-21).

use std::collections::{BTreeMap, HashMap, HashSet};

use atlas_core::{
    ActivationTarget, SavedEntryId, SavedFolderId, SavedFolderSnapshot, SavedPageId,
    SavedPageSnapshot, TabId, TabSnapshot,
};
use atlas_ui::{
    SIDEBAR_WIDTH_STATE_KEY, SidebarWidthState,
    metrics::chrome::ICON_BUTTON,
    parse_sidebar_setting,
    sidebar::{SIDEBAR_PADDING_LEFT, SIDEBAR_PADDING_RIGHT, draws_content},
    sidebar_setting,
};
use gpui::{
    AppContext, Context, Div, ElementId, InteractiveElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, Role, Stateful, StatefulInteractiveElement,
    Styled, Window, div, prelude::FluentBuilder, px,
};

use super::{
    BrowserView, button,
    folder_editor::{
        DeleteFolderContents, DraggedRow, MoveRowDown, MoveRowIn, MoveRowOut, MoveRowUp, OpenRow,
        PressButton, ROW_CONTEXT, RemoveRow, RenameRow, keyboard_button,
    },
    page_card::edge_strip,
    saved_moves::{DropTarget, Step},
    sidebar_rows::{SavedRow, visible_saved_rows},
    tiles::monogram,
};
use crate::{session::SessionCommand, theme};

const ROW_HEIGHT: f32 = 36.;
/// How far each folder level indents its contents in the wide sidebar.
const FOLDER_INDENT: f32 = 12.;
const MONOGRAM_SIZE: f32 = 20.;
const PLUGINS: [&str; 3] = ["Forge", "Notes", "Fledge"];
const LAST_VISIBLE_KEY: &str = "sidebar.last_visible_state";

/// What a row shows: its icon letter, title and selection, and how deep it is nested.
struct RowLook {
    icon: String,
    title: String,
    active: bool,
    depth: usize,
    saved: bool,
}

impl BrowserView {
    /// The sidebar at the width the sizing reserves. Narrower than favicon-only (hidden, or
    /// mid-drag) it draws only its unpadded background, so it is exactly that wide.
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        self.sidebar_at(
            self.sidebar_sizing.width(),
            self.sidebar_sizing.labels_visible(),
            cx,
        )
    }

    /// The popup and persistent sidebar share all content and listeners.
    pub(super) fn sidebar_at(&self, width: f32, labels: bool, cx: &mut Context<Self>) -> Div {
        let frame = div()
            .w(px(width))
            .flex_none()
            .h_full()
            .overflow_hidden()
            .bg(theme::ground());
        if !draws_content(width) {
            return frame;
        }
        frame
            .pt_3()
            .pb_3()
            .pl(px(SIDEBAR_PADDING_LEFT))
            .pr(px(SIDEBAR_PADDING_RIGHT))
            .flex()
            .flex_col()
            .gap_3()
            .children(labels.then(|| {
                div()
                    .px_2()
                    .text_lg()
                    .text_color(theme::accent())
                    .child("Atlas")
            }))
            .child(search_button(labels, cx))
            .child(self.favorites(labels, cx))
            .child(plugin_section(labels))
            .child(div().h(px(1.)).bg(theme::line()))
            .child(tabs_header(labels, cx))
            .children(labels.then(|| self.folder_editor_view(cx)).flatten())
            .child(
                div()
                    .id("tab-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(self.rows(labels, cx)),
            )
    }

    /// The strip between the sidebar and the page card that the owner drags. It also takes
    /// keyboard focus: Left and Right step between the three states.
    pub(super) fn sidebar_edge(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let dragging = self.sidebar_sizing.is_dragging();
        let (start, end) = edge_strip(self.sidebar_sizing.width());
        div()
            .id("sidebar-edge")
            .role(Role::Splitter)
            .aria_label("Resize sidebar")
            .aria_description(self.sidebar_sizing.state().name())
            .track_focus(&self.sidebar_edge_focus)
            .tab_index(0)
            .focus(|style| style.bg(theme::line()))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.sidebar_edge_key(event, window, cx);
            }))
            .w(px(end - start))
            .flex_none()
            .h_full()
            .cursor_col_resize()
            .on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                if *hovered {
                    this.watch_peek(window, cx);
                }
            }))
            .when(dragging, |edge| edge.bg(theme::accent()))
            .hover(|style| style.bg(theme::line()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.retire_peek(cx);
                    this.sidebar_sizing.begin_drag(f32::from(event.position.x));
                    cx.notify();
                }),
            )
    }

    /// Follows the pointer while the edge is held. GPUI captures the mouse on press, so moves
    /// over the page's web view still arrive here.
    pub(super) fn sidebar_drag_moved(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sidebar_sizing.is_dragging() {
            return;
        }
        // A release outside every window can be lost; the next move without a button ends it.
        if !event.dragging() {
            self.sidebar_drag_released(f32::from(event.position.x), window, cx);
            return;
        }
        if self.sidebar_sizing.drag_to(f32::from(event.position.x)) {
            cx.notify();
        }
    }

    /// Snaps a drag released at `pointer_x` to wide, favicon-only or hidden. GPUI stops
    /// reporting moves outside the window, so the release position decides. Selection is
    /// untouched.
    pub(super) fn sidebar_drag_released(
        &mut self,
        pointer_x: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = self.sidebar_sizing.state();
        if self.sidebar_sizing.release_at(pointer_x).is_some() {
            self.sidebar_state_settled(previous, window, cx);
        }
    }

    /// Settles a drag at its current width when Atlas loses activation (Alt Tab, a dialog),
    /// because the mouse capture and the release that would end it go elsewhere.
    pub(super) fn sidebar_drag_abandoned(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.sidebar_sizing.state();
        if self.sidebar_sizing.release().is_some() {
            self.sidebar_state_settled(previous, window, cx);
        }
    }

    /// Left narrows and Right widens the sidebar by one state from the focused edge.
    fn sidebar_edge_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.sidebar_sizing.state();
        let next = match event.keystroke.key.as_str() {
            "left" => current.narrower(),
            "right" => current.wider(),
            _ => return,
        };
        cx.stop_propagation();
        self.set_sidebar_state(next, window, cx);
    }

    fn set_sidebar_state(
        &mut self,
        state: SidebarWidthState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sidebar_sizing.is_dragging() && self.sidebar_sizing.state() == state {
            return;
        }
        self.retire_peek(cx);
        let previous = self.sidebar_sizing.state();
        self.sidebar_sizing.set_state(state);
        self.sidebar_state_settled(previous, window, cx);
    }

    /// Shows the width state saved last time; this restores, so nothing is saved back.
    pub(super) fn restore_sidebar_state(&mut self, settings: &BTreeMap<String, String>) {
        self.sidebar_sizing = restored_sidebar_sizing(settings);
    }

    /// Saves a changed state (DW-2) and redraws; render then moves the page card.
    pub(super) fn sidebar_state_settled(
        &mut self,
        previous: SidebarWidthState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_sizing.state().can_peek() {
            self.peek_state.rearm();
            self.watch_peek(window, cx);
        }
        if let Some(setting) = settled_sidebar_setting(previous, self.sidebar_sizing.state()) {
            let (_, visible) = sidebar_setting(self.sidebar_sizing.last_visible());
            self.command(SessionCommand::SaveSettings(vec![
                setting,
                (LAST_VISIBLE_KEY.to_owned(), visible),
            ]));
        }
        cx.notify();
    }

    /// Saved rows in layout order (skipping collapsed folders), then ordinary tabs.
    fn rows(&self, labels: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        // Index only the visible rows: a large collapsed import must not cost a map per frame.
        let visible = visible_saved_rows(&self.model.saved_layout, &self.collapsed_folders);
        let visible_pages: HashSet<SavedPageId> = visible
            .iter()
            .filter_map(|row| match row {
                SavedRow::Page { id, .. } => Some(*id),
                SavedRow::Folder { .. } => None,
            })
            .collect();
        let pages: HashMap<SavedPageId, &SavedPageSnapshot> = self
            .model
            .saved_pages
            .iter()
            .filter(|page| visible_pages.contains(&page.id))
            .map(|page| (page.id, page))
            .collect();
        let folders: HashMap<SavedFolderId, &SavedFolderSnapshot> = self
            .model
            .saved_folders
            .iter()
            .map(|folder| (folder.id, folder))
            .collect();
        let mut rows = div()
            .id("sidebar-rows")
            .role(Role::Tree)
            .aria_label("Saved and open tabs")
            .flex()
            .flex_col()
            .gap_1();
        for row in visible {
            let element = match row {
                SavedRow::Folder {
                    id,
                    depth,
                    expanded,
                } => folders
                    .get(&id)
                    .map(|folder| self.folder_row(folder, depth, expanded, labels, cx)),
                SavedRow::Page { id, depth } => pages
                    .get(&id)
                    .map(|page| self.saved_row(page, depth, labels, cx)),
            };
            rows = rows.children(element);
        }
        for tab in self
            .model
            .tabs
            .iter()
            .filter(|tab| tab.saved_page_id.is_none())
        {
            let title = ordinary_title(tab, self.popup_pages.contains(&tab.id));
            rows = rows.child(self.ordinary_row(tab.id, title, labels, cx));
        }
        rows
    }

    fn saved_row(
        &self,
        page: &SavedPageSnapshot,
        depth: usize,
        labels: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let saved_id = page.id;
        let live = self
            .model
            .tabs
            .iter()
            .find(|tab| tab.saved_page_id == Some(saved_id))
            .map(|tab| tab.id);
        let title = title_or_address(&page.metadata.title, &page.metadata.address);
        let look = RowLook {
            icon: monogram(&title),
            title: title.clone(),
            active: live.is_some() && live == self.model.active_id,
            depth,
            saved: true,
        };
        let row = row_frame(("saved", saved_id.get()), look, labels)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_saved_row(saved_id, window, cx);
            }))
            .on_action(cx.listener(move |this, _: &OpenRow, window, cx| {
                this.open_saved_row(saved_id, window, cx);
            }))
            .on_action(cx.listener(move |this, _: &RemoveRow, _, cx| {
                this.command(SessionCommand::Unsave(saved_id));
                cx.notify();
            }));
        let mut row = movable_row(row, SavedEntryId::Page(saved_id), title, cx);
        if !labels {
            return row;
        }
        if let Some(id) = live {
            row = row.child(
                row_action(("unload", id.get()), "×", "Unload page").on_click(cx.listener(
                    move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.command(SessionCommand::CloseTab(id));
                        cx.notify();
                    },
                )),
            );
        }
        row.child(
            row_action(("unsave", saved_id.get()), "−", "Remove saved page")
                .text_color(theme::icon_dim())
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.command(SessionCommand::Unsave(saved_id));
                    cx.notify();
                })),
        )
    }

    /// Opens or focuses a saved page through the one activation path, as a click does.
    fn open_saved_row(&mut self, id: SavedPageId, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(ActivationTarget::Saved(id));
        self.queue_peek_action(super::peek::PeekAction::Dismiss, window, cx);
        cx.notify();
    }

    fn ordinary_row(
        &self,
        id: TabId,
        title: String,
        labels: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let look = RowLook {
            icon: monogram(&title),
            title,
            active: Some(id) == self.model.active_id,
            depth: 0,
            saved: false,
        };
        let row = row_frame(("tab", id.get()), look, labels)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_tab_row(id, window, cx);
            }))
            .on_action(cx.listener(move |this, _: &OpenRow, window, cx| {
                this.open_tab_row(id, window, cx);
            }));
        if !labels {
            return row;
        }
        row.child(
            row_action(("close", id.get()), "×", "Close tab").on_click(cx.listener(
                move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.command(SessionCommand::CloseTab(id));
                    cx.notify();
                },
            )),
        )
    }

    fn open_tab_row(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(ActivationTarget::Tab(id));
        self.queue_peek_action(super::peek::PeekAction::Dismiss, window, cx);
        cx.notify();
    }

    /// A folder row: its arrow toggles the contents. Renaming and removal need the wide
    /// sidebar's form, so the favicon-only width offers only toggling and moving (D44).
    fn folder_row(
        &self,
        folder: &SavedFolderSnapshot,
        depth: usize,
        expanded: bool,
        labels: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = folder.id;
        let look = RowLook {
            icon: if expanded { "▾" } else { "▸" }.to_owned(),
            title: folder.title.clone(),
            active: false,
            depth,
            saved: true,
        };
        let row = row_frame(("folder", id.get()), look, labels)
            .aria_expanded(expanded)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_folder(id);
                cx.notify();
            }))
            .on_action(cx.listener(move |this, _: &OpenRow, _, cx| {
                this.toggle_folder(id);
                cx.notify();
            }));
        let row = movable_row(row, SavedEntryId::Folder(id), folder.title.clone(), cx);
        if !labels {
            return row;
        }
        row.on_action(cx.listener(move |this, _: &RenameRow, window, cx| {
            this.start_rename_folder(id, window, cx);
        }))
        .on_action(cx.listener(move |this, _: &RemoveRow, window, cx| {
            this.request_folder_removal(id, false, window, cx);
        }))
        .on_action(
            cx.listener(move |this, _: &DeleteFolderContents, window, cx| {
                this.request_folder_removal(id, true, window, cx);
            }),
        )
        .child(
            row_action(("rename-folder", id.get()), "✎", "Rename folder")
                .text_color(theme::icon_dim())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.start_rename_folder(id, window, cx);
                })),
        )
        .child(
            row_action(("remove-folder", id.get()), "−", "Remove folder")
                .text_color(theme::icon_dim())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.request_folder_removal(id, false, window, cx);
                })),
        )
    }
}

/// Lets a saved page or folder move by Alt arrows while focused, be dragged, and take drops.
fn movable_row(
    row: Stateful<Div>,
    entry: SavedEntryId,
    title: String,
    cx: &mut Context<BrowserView>,
) -> Stateful<Div> {
    let step = |step: Step| {
        move |this: &mut BrowserView, cx: &mut Context<BrowserView>| {
            this.move_saved_row(entry, step);
            cx.notify();
        }
    };
    let (up, down, out, into) = (
        step(Step::Up),
        step(Step::Down),
        step(Step::Out),
        step(Step::In),
    );
    row.on_action(cx.listener(move |this, _: &MoveRowUp, _, cx| up(this, cx)))
        .on_action(cx.listener(move |this, _: &MoveRowDown, _, cx| down(this, cx)))
        .on_action(cx.listener(move |this, _: &MoveRowOut, _, cx| out(this, cx)))
        .on_action(cx.listener(move |this, _: &MoveRowIn, _, cx| into(this, cx)))
        .on_drag(DraggedRow { entry, title }, |dragged, _, _, cx| {
            cx.new(|_| dragged.clone())
        })
        .drag_over::<DraggedRow>(|style, _, _, _| style.bg(theme::preview_row_selected()))
        .on_drop(cx.listener(move |this, dragged: &DraggedRow, _, cx| {
            this.drop_saved_row(dragged.entry, DropTarget::Row(entry));
            cx.notify();
        }))
}

/// The shared row shape: a titled row when labels show, a centred icon button otherwise.
/// Every row is a focusable tree item at its nesting level, keeping its title as the
/// accessible name; the focused row shows an accent outline (what Enter will act on).
fn row_frame(id: impl Into<ElementId>, look: RowLook, labels: bool) -> Stateful<Div> {
    let row = div()
        .id(id)
        .role(Role::TreeItem)
        .aria_level(look.depth + 1)
        .aria_label(look.title.clone())
        .aria_selected(look.active)
        .key_context(ROW_CONTEXT)
        .tab_index(0)
        .border_1()
        .border_color(gpui::transparent_black())
        .focus(|style| style.border_color(theme::accent()))
        .h(px(ROW_HEIGHT))
        .flex_none()
        .rounded_md()
        .flex()
        .items_center()
        .cursor_pointer()
        .bg(if look.active {
            theme::preview_row_selected()
        } else {
            theme::ground()
        });
    let icon = row_icon(&look.icon, look.saved);
    if !labels {
        return row.w(px(ICON_BUTTON)).justify_center().child(icon);
    }
    let indent = 8. + FOLDER_INDENT * look.depth as f32;
    row.pl(px(indent))
        .pr_1()
        .gap_2()
        .child(icon)
        .child(div().flex_1().min_w_0().truncate().child(look.title))
}

/// The placeholder favicon: a letter in a small square, accent-coloured for saved rows.
fn row_icon(letter: &str, saved: bool) -> Div {
    div()
        .size(px(MONOGRAM_SIZE))
        .flex_none()
        .rounded_md()
        .bg(theme::line())
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(if saved {
            theme::accent()
        } else {
            theme::icon()
        })
        .child(letter.to_owned())
}

/// A small control at the end of a wide row, named for screen readers.
fn row_action(id: impl Into<ElementId>, glyph: &'static str, name: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(name)
        .px_1()
        .child(glyph)
}

/// Opens the palette: a labelled button when wide, an icon button otherwise.
fn search_button(labels: bool, cx: &mut Context<BrowserView>) -> Stateful<Div> {
    let open = cx.listener(|this, _, window, cx| this.open_palette(false, window, cx));
    if labels {
        return button("palette", "Search · Ctrl K").on_click(open);
    }
    icon_button("palette", "⌕", "Search · Ctrl K").on_click(open)
}

/// The plugin placeholders: names when wide, letters with the same names otherwise.
fn plugin_section(labels: bool) -> Div {
    let section = div().flex().flex_col().gap_1();
    if labels {
        return section
            .child(
                div()
                    .text_xs()
                    .text_color(theme::icon_dim())
                    .px_2()
                    .child("PLUGINS"),
            )
            .children(PLUGINS.map(|name| {
                div()
                    .px_2()
                    .py_1()
                    .text_color(theme::icon_dim())
                    .child(format!("{name} · coming soon"))
            }));
    }
    section.children(PLUGINS.map(|name| {
        div()
            .id(name)
            .aria_label(format!("{name} · coming soon"))
            .w(px(ICON_BUTTON))
            .h(px(ICON_BUTTON))
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme::icon_dim())
            .child(monogram(name))
    }))
}

/// The tab section's header with its new-folder and new-tab buttons. A row dropped on it
/// moves to the end of the top level, out of any folder.
fn tabs_header(labels: bool, cx: &mut Context<BrowserView>) -> Div {
    let new_tab = cx.listener(|this, _, window, cx| {
        this.command(SessionCommand::NewTab {
            address: "about:blank".to_owned(),
        });
        this.queue_peek_action(super::peek::PeekAction::Dismiss, window, cx);
        cx.notify();
    });
    let header = div()
        .flex()
        .items_center()
        .rounded_md()
        .drag_over::<DraggedRow>(|style, _, _, _| style.bg(theme::preview_row_selected()))
        .on_drop(cx.listener(|this, dragged: &DraggedRow, _, cx| {
            this.drop_saved_row(dragged.entry, DropTarget::TopLevel);
            cx.notify();
        }));
    if !labels {
        return header.child(icon_button("new-tab", "+", "New tab").on_click(new_tab));
    }
    header
        .child(
            div()
                .flex_1()
                .px_2()
                .text_xs()
                .text_color(theme::icon_dim())
                .child("SAVED & OPEN TABS"),
        )
        .child(
            keyboard_button(button("new-folder", "New folder"))
                .aria_label("New folder")
                .on_click(cx.listener(|this, _, window, cx| this.start_new_folder(window, cx)))
                .on_action(cx.listener(|this, _: &PressButton, window, cx| {
                    this.start_new_folder(window, cx);
                })),
        )
        .child(
            button("new-tab", "+")
                .aria_label("New tab")
                .on_click(new_tab),
        )
}

/// A square icon button the width of the favicon-only column.
fn icon_button(id: &'static str, glyph: &'static str, name: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(name)
        .w(px(ICON_BUTTON))
        .h(px(ICON_BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(theme::line())
        .cursor_pointer()
        .child(glyph)
}

/// The setting to save when the sidebar settles, or `None` when its state did not change.
fn settled_sidebar_setting(
    previous: SidebarWidthState,
    settled: SidebarWidthState,
) -> Option<(String, String)> {
    (settled != previous).then(|| sidebar_setting(settled))
}

/// The stored width state; a missing or unreadable value keeps the default, wide.
fn restored_sidebar_state(settings: &BTreeMap<String, String>) -> SidebarWidthState {
    settings
        .get(SIDEBAR_WIDTH_STATE_KEY)
        .and_then(|value| parse_sidebar_setting(value))
        .unwrap_or(SidebarWidthState::Wide)
}

/// Retain the width to pin even when the saved presentation starts hidden.
fn restored_sidebar_sizing(settings: &BTreeMap<String, String>) -> atlas_ui::SidebarSizing {
    let visible = settings
        .get(LAST_VISIBLE_KEY)
        .and_then(|value| parse_sidebar_setting(value))
        .filter(|state| !state.can_peek())
        .unwrap_or(SidebarWidthState::Wide);
    let mut sizing = atlas_ui::SidebarSizing::new(visible);
    sizing.set_state(restored_sidebar_state(settings));
    sizing
}

/// A saved page shows its title, or its address when it has none.
fn title_or_address(title: &str, address: &str) -> String {
    if title.is_empty() {
        address.to_owned()
    } else {
        title.to_owned()
    }
}

/// An ordinary tab's title; a blank tab is Home, unless it is an adopted popup (P1-11)
/// that has not reported its first address yet.
fn ordinary_title(tab: &TabSnapshot, popup: bool) -> String {
    if tab.metadata.address == "about:blank" && !popup {
        return "Home".to_owned();
    }
    title_or_address(&tab.metadata.title, &tab.metadata.address)
}

#[cfg(test)]
mod tests {
    use atlas_ui::{SidebarSizing, sidebar::WIDE_WIDTH};

    use super::*;

    /// What the settings table would hold after the given writes, as a restart reads it.
    fn stored(writes: impl IntoIterator<Item = (String, String)>) -> BTreeMap<String, String> {
        writes.into_iter().collect()
    }

    #[test]
    fn a_settled_drag_is_saved_and_restored_after_restart() {
        let mut sizing = SidebarSizing::default();
        let previous = sizing.state();
        sizing.begin_drag(WIDE_WIDTH);
        let settled = sizing.release_at(40.).expect("a drag was in progress");
        assert_eq!(settled, SidebarWidthState::Compact);

        let write = settled_sidebar_setting(previous, settled).expect("the state changed");
        let restarted = SidebarSizing::new(restored_sidebar_state(&stored([write])));
        assert_eq!(restarted.state(), SidebarWidthState::Compact);
        assert_eq!(restarted.width(), sizing.width());
    }

    #[test]
    fn every_state_survives_a_restart() {
        for state in SidebarWidthState::ALL {
            let restored = restored_sidebar_state(&stored([sidebar_setting(state)]));
            assert_eq!(restored, state);
        }
    }

    #[test]
    fn hidden_restart_pins_the_previously_visible_width() {
        for state in [SidebarWidthState::Wide, SidebarWidthState::Compact] {
            let (_, value) = sidebar_setting(state);
            let mut sizing = restored_sidebar_sizing(&stored([
                sidebar_setting(SidebarWidthState::Hidden),
                (LAST_VISIBLE_KEY.to_owned(), value),
            ]));
            assert_eq!(sizing.width(), 0.);
            sizing.toggle_hidden();
            assert_eq!(sizing.state(), state);
        }
    }

    #[test]
    fn an_unchanged_state_is_not_saved_again() {
        for state in SidebarWidthState::ALL {
            assert_eq!(settled_sidebar_setting(state, state), None);
        }
    }

    #[test]
    fn a_missing_or_unreadable_state_keeps_the_wide_default() {
        assert_eq!(
            restored_sidebar_state(&BTreeMap::new()),
            SidebarWidthState::Wide
        );
        for value in ["", "compact", "peek", "Hidden"] {
            let settings = stored([(SIDEBAR_WIDTH_STATE_KEY.to_owned(), value.to_owned())]);
            assert_eq!(
                restored_sidebar_state(&settings),
                SidebarWidthState::Wide,
                "{value}"
            );
        }
    }
}

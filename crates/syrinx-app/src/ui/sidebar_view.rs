//! Draws the sidebar at its current D44 width and lets the owner drag its edge (P1-08, P1-09),
//! with the saved rows' folder controls (P1-21).

use std::collections::BTreeMap;

use gpui::{
    App, AppContext, ClickEvent, Context, Div, ElementId, FocusHandle, FontWeight,
    InteractiveElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement,
    Role, Stateful, StatefulInteractiveElement, Styled, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};
use syrinx_core::usage::Feature;
use syrinx_core::{
    ActivationTarget, BuiltInPlugin, DestinationTarget, PageOrigin, SavedEntryId,
    SavedFolderSnapshot, SavedPageSnapshot, TabId,
};
use syrinx_ui::{
    SIDEBAR_WIDTH_STATE_KEY, ShadowRole, SidebarWidthState,
    metrics::chrome::ICON_BUTTON,
    parse_sidebar_setting,
    sidebar::{SIDEBAR_PADDING_LEFT, SIDEBAR_PADDING_RIGHT, draws_content},
    sidebar_setting,
};

use crate::controls::{control, glyph, icon_button, raised, ring_shown};

use super::{
    BrowserView,
    favorites::{DraggedFavorite, FavoriteSource},
    folder_editor::{
        CloseRowPage, DeleteFolderContents, DraggedRow, MoveRowDown, MoveRowIn, MoveRowOut,
        MoveRowUp, OpenRowMenu, ROW_CONTEXT, RemoveRow, RenameRow,
    },
    menus::MenuKind,
    page_card::edge_strip,
    row_menu::TabAction,
    saved_moves::{DropTarget, Step},
    sidebar_tree::{RowClose, SidebarEntry, TreeId, active_tree_id, row_close, sidebar_entries},
    tiles::{self, monogram},
};
use crate::{session::SessionCommand, theme};

const ROW_HEIGHT: f32 = 36.;
/// How far each folder level indents its contents in the wide sidebar.
const FOLDER_INDENT: f32 = 12.;
const MONOGRAM_SIZE: f32 = 20.;
/// The compact row's hover group, which shows its close badge (#128).
const COMPACT_ROW_GROUP: &str = "compact-row";
/// The compact close badge: its hit target, how far below the row's top it sits, and its ×.
const BADGE_SIZE: f32 = 16.;
const BADGE_INSET: f32 = 2.;
const BADGE_GLYPH_SIZE: f32 = 11.;
/// The compact row's muted badge on the icon's bottom-right corner, clear of the close badge
/// above it (#95), and the wide row's mute glyph.
const MUTED_BADGE_SIZE: f32 = 12.;
const MUTED_BADGE_INSET: f32 = 4.;
const MUTED_BADGE_GLYPH_SIZE: f32 = 8.;
const MUTE_GLYPH_SIZE: f32 = 12.;
/// The wide row's × and mute buttons share this square, so they line up and are as easy to hit.
const ROW_ACTION_SIZE: f32 = 20.;
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
    pub(super) fn sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.sidebar_at(
            self.sidebar_sizing.width(),
            self.sidebar_sizing.labels_visible(),
            window,
            cx,
        )
    }

    /// The popup and persistent sidebar share all content and listeners. `window` is the one
    /// drawing it, so each window keeps its own row focus. Only the main window and the peek
    /// may draw the sidebar: `draws_peek_tree` treats every other window as the peek.
    pub(super) fn sidebar_at(
        &mut self,
        width: f32,
        labels: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let frame = div()
            .w(px(width))
            .flex_none()
            .h_full()
            .overflow_hidden()
            .bg(theme::ground());
        if !draws_content(width) {
            return frame;
        }
        let rows = self.rows(labels, window, cx);
        // The main window's title bar draws the address unless the sidebar is wide.
        let address = labels.then(|| self.address_field(true, !self.draws_peek_tree(window), cx));
        frame
            .pt_1()
            .pb_2()
            .pl(px(SIDEBAR_PADDING_LEFT))
            .pr(px(SIDEBAR_PADDING_RIGHT))
            .flex()
            .flex_col()
            .gap_3()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.open_menu_at(MenuKind::Sidebar, event, window, cx);
                }),
            )
            .children(address)
            .child(self.favorites(labels, window, cx))
            .children(
                labels
                    .then(|| self.folder_editor_view(window, cx))
                    .flatten(),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.plugin_rows(labels, cx))
                    .child(rows)
                    .child(new_tab_row(labels, cx)),
            )
            .child(library_row(labels, cx))
    }

    /// The built-in plugins, inline above the tabs as the canvas draws them: a coloured icon
    /// and the name, dimmed until the plugin runtime exists. Each can be dragged to Favorites.
    fn plugin_rows(&self, labels: bool, cx: &mut Context<Self>) -> Div {
        let palette = theme::current();
        let mut rows = div().flex_none().flex().flex_col().gap_1();
        for plugin in BuiltInPlugin::ALL {
            let colors = tiles::tile_colors(&palette, &DestinationTarget::Plugin(plugin));
            let icon = div()
                .size(px(22.))
                .flex_none()
                .rounded(px(7.))
                .bg(theme::rgba(colors.background))
                .text_color(theme::rgba(colors.foreground))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .flex()
                .items_center()
                .justify_center()
                .child(monogram(plugin.name()));
            let dragged = DraggedFavorite {
                title: plugin.name().to_owned(),
                source: FavoriteSource::Plugin(plugin),
            };
            let row = control(
                div().id(plugin.id()),
                Role::Button,
                format!("{} · not available yet", plugin.name()),
            )
            .h(px(34.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(9.))
            .opacity(0.7)
            .cursor_pointer()
            .text_color(theme::rgba(palette.text_secondary))
            .hover(move |style| style.bg(theme::rgba(palette.row_selected)))
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.message = tiles::unavailable_message(plugin);
                cx.notify();
            }))
            .child(icon);
            rows = rows.child(if labels {
                row.px(px(8.)).child(plugin.name())
            } else {
                row.w(px(ICON_BUTTON)).justify_center()
            });
        }
        rows
    }

    /// The address an open tab shows, for dragging it to Favorites.
    fn tab_address(&self, id: TabId) -> String {
        self.model
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.metadata.address.clone())
            .unwrap_or_default()
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
            .aria_orientation(gpui::Orientation::Vertical)
            .aria_keyshortcuts("ArrowLeft ArrowRight")
            .track_focus(&self.sidebar_edge_focus)
            .tab_index(0)
            .focus(|style| crate::controls::focus_ring(style.bg(theme::line())))
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

    /// Settles a drag at its current width when Syrinx loses activation (Alt Tab, a dialog),
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
            self.count(match self.sidebar_sizing.state() {
                SidebarWidthState::Wide => Feature::SidebarSetWide,
                SidebarWidthState::Compact => Feature::SidebarSetCompact,
                SidebarWidthState::Hidden => Feature::SidebarSetHidden,
            });
            let (_, visible) = sidebar_setting(self.sidebar_sizing.last_visible());
            self.command(SessionCommand::SaveSettings(vec![
                setting,
                (LAST_VISIBLE_KEY.to_owned(), visible),
            ]));
        }
        cx.notify();
    }

    /// The tree of saved rows in layout order (skipping collapsed folders), then ordinary
    /// tabs. It has one Tab stop, and arrows move between its rows (DW-61); it scrolls itself
    /// so a row focused by an arrow key stays in view.
    fn rows(&mut self, labels: bool, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let entries = sidebar_entries(&self.model, &self.collapsed_folders);
        let tree_rows = entries.iter().map(SidebarEntry::tree_row).collect();
        let peek = self.draws_peek_tree(window);
        let active = active_tree_id(&self.model);
        let prepared = self
            .sidebar_trees
            .prepare(peek, tree_rows, active, window, cx);
        let mut rows = div()
            .id("sidebar-rows")
            .role(Role::Tree)
            .aria_label("Saved and open tabs")
            // Shrinks and scrolls when long, but does not grow, so New tab follows the last tab.
            .flex_shrink(1.)
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&prepared.scroll)
            .flex()
            .flex_col()
            .gap_1();
        for (entry, focus) in entries.iter().zip(prepared.handles) {
            let tree_id = entry.tree_row().id;
            // The row's close control, if it has one, and whether the row shows its focus
            // ring, which also shows a compact row's close badge.
            let close = row_close(&self.model, tree_id);
            let shows_ring = ring_shown(&focus, window);
            let row = match entry {
                SidebarEntry::Folder {
                    folder,
                    depth,
                    expanded,
                } => self.folder_row(folder, *depth, *expanded, labels, cx),
                SidebarEntry::Page { page, depth } => {
                    self.saved_row(page, *depth, labels, close, shows_ring, cx)
                }
                SidebarEntry::Tab(tab) => {
                    let title = tab.metadata.display_title(PageOrigin::from_adopted_popup(
                        self.popup_pages.contains(&tab.id),
                    ));
                    self.ordinary_row(tab.id, title, labels, close, shows_ring, cx)
                }
            };
            rows = rows.child(tree_item(row, tree_id, &focus, cx));
        }
        rows
    }

    fn saved_row(
        &self,
        page: &SavedPageSnapshot,
        depth: usize,
        labels: bool,
        close: Option<RowClose>,
        shows_ring: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let saved_id = page.id;
        // Only a live page's row has a close control, and the tab it unloads is the live tab.
        let live = close.map(RowClose::tab);
        let title = page.display_title();
        let look = RowLook {
            icon: monogram(&title),
            title: title.clone(),
            active: live.is_some() && live == self.model.active_id,
            depth,
            saved: true,
        };
        let target = ActivationTarget::Saved(saved_id);
        let remove = TabAction::RemoveSaved(saved_id);
        let row = row_frame(("saved", saved_id.get()), look, labels).on_click(cx.listener(
            move |this, _, window, cx| {
                this.activate_row(target, window, cx);
            },
        ));
        // The confirmation for an unopened page needs the wide sidebar's card.
        let row = if labels || live.is_some() {
            row.on_action(cx.listener(move |this, _: &RemoveRow, window, cx| {
                this.run_tab_action(remove, None, window, cx);
            }))
        } else {
            row
        };
        let mut row = movable_row(row, SavedEntryId::Page(saved_id), title, cx);
        if let Some(tab) = live {
            row = self.with_mute_marker(row, tab, labels, cx);
        }
        if let Some(close) = close {
            row = row.child(close_control(close, labels, shows_ring, cx));
        }
        if !labels {
            return row;
        }
        row.child(
            row_action(("unsave", saved_id.get()), "−", "Remove saved page")
                .text_color(theme::icon_dim())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run_tab_action(remove, None, window, cx);
                })),
        )
    }

    fn ordinary_row(
        &self,
        id: TabId,
        title: String,
        labels: bool,
        close: Option<RowClose>,
        shows_ring: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let look = RowLook {
            icon: monogram(&title),
            title,
            active: Some(id) == self.model.active_id,
            depth: 0,
            saved: false,
        };
        let target = ActivationTarget::Tab(id);
        let dragged = DraggedFavorite {
            title: look.title.clone(),
            source: FavoriteSource::Address(self.tab_address(id)),
        };
        let view = cx.entity().downgrade();
        let row = row_frame(("tab", id.get()), look, labels)
            .on_drag(dragged, move |dragged, _, window, cx| {
                release_on_drag(&view, window, cx);
                cx.new(|_| dragged.clone())
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.activate_row(target, window, cx);
            }))
            // Delete closes the tab, as its button does: the button is not a Tab stop (DW-61).
            .on_action(cx.listener(move |this, _: &RemoveRow, window, cx| {
                this.run_tab_action(TabAction::Close(id), None, window, cx);
            }));
        self.with_mute_marker(row, id, labels, cx)
            .children(close.map(|close| close_control(close, labels, shows_ring, cx)))
    }

    /// A muted tab's marker (#95): a clickable mute glyph before the wide row's ×, which
    /// unmutes, or a small badge on the compact icon's bottom-right corner, shown only. It
    /// follows what the page reports, so a mute the runtime refused never shows.
    fn with_mute_marker(
        &self,
        row: Stateful<Div>,
        tab: TabId,
        labels: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        if !self.shown_muted.contains(&tab) {
            return row;
        }
        let row = row.aria_description("Muted");
        if !labels {
            return row.child(muted_badge(tab));
        }
        row.child(
            row_action_box(row_button(("unmute", tab.get()), "Unmute tab"))
                .font_family(glyph::FONT)
                .text_size(px(MUTE_GLYPH_SIZE))
                .text_color(theme::icon())
                .hover(|style| style.text_color(theme::text()))
                .child(glyph::MUTE)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run_tab_action(TabAction::SetMuted(tab, false), None, window, cx);
                    // The row stays, so its keys must not stay armed after the click.
                    this.release_row_focus(window, cx);
                })),
        )
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
            // A click lets go of focus, as page rows do, so Alt Left then means Back; a
            // keyboard toggle keeps focus on the row so the tree can still be walked.
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.toggle_folder(id);
                if !event.is_keyboard() {
                    this.release_row_focus(window, cx);
                }
                cx.notify();
            }));
        let row = movable_row(row, SavedEntryId::Folder(id), folder.title.clone(), cx);
        if !labels {
            return row;
        }
        row.on_action(cx.listener(move |this, _: &RenameRow, window, cx| {
            this.run_tab_action(TabAction::RenameFolder(id), None, window, cx);
        }))
        .on_action(cx.listener(move |this, _: &RemoveRow, window, cx| {
            this.run_tab_action(TabAction::RemoveFolder(id), None, window, cx);
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
                    this.run_tab_action(TabAction::RenameFolder(id), None, window, cx);
                })),
        )
        .child(
            row_action(("remove-folder", id.get()), "−", "Remove folder")
                .text_color(theme::icon_dim())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run_tab_action(TabAction::RemoveFolder(id), None, window, cx);
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
    let view = cx.entity().downgrade();
    // An Alt arrow that moves nothing goes on to the window's own binding (Alt Left is Back).
    let step = |step: Step| {
        move |this: &mut BrowserView, cx: &mut Context<BrowserView>| {
            if this.move_saved_row(entry, step) {
                cx.notify();
            } else {
                cx.propagate();
            }
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
        // A row focused from the keyboard and then dragged lets go of focus, as a click does.
        .on_drag(
            DraggedRow { entry, title },
            move |dragged, _, window, cx| {
                release_on_drag(&view, window, cx);
                cx.new(|_| dragged.clone())
            },
        )
        .drag_over::<DraggedRow>(|style, _, _, _| style.bg(theme::preview_row_selected()))
        .on_drop(cx.listener(move |this, dragged: &DraggedRow, window, cx| {
            this.drop_saved_row(dragged.entry, DropTarget::Row(entry));
            this.release_row_focus(window, cx);
            cx.notify();
        }))
}

/// Releases row focus through the view at a drag's start; a view already gone (Syrinx
/// closing) leaves nothing to protect, so the window's focus is simply cleared.
fn release_on_drag(view: &WeakEntity<BrowserView>, window: &mut Window, cx: &mut App) {
    if view
        .update(cx, |browser, cx| browser.release_row_focus(window, cx))
        .is_err()
    {
        window.blur(cx);
    }
}

/// Makes a drawn row a member of the tree's roving focus (DW-61): it tracks its own focus
/// handle, which is a Tab stop only for the one row `syrinx_ui::tree_focus` picks, and its
/// arrow keys move focus between rows. Ctrl W closes its live page (DW-63); every row kind
/// handles it, so a row with nothing to close never passes it on to the window's Ctrl W. A
/// right-click, Shift F10 or the Menu key opens the row's own menu (#95).
fn tree_item(
    row: Stateful<Div>,
    id: TreeId,
    focus: &FocusHandle,
    cx: &mut Context<BrowserView>,
) -> Stateful<Div> {
    row.track_focus(focus)
        // Announced like the close control's keys: Shift F10 or the Menu key opens the row menu.
        .aria_keyshortcuts("Shift+F10 ContextMenu")
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            this.tree_key_down(id, event, window, cx);
        }))
        // A handled action stops here: GPUI tries the window's binding only if it propagates.
        .on_action(cx.listener(move |this, _: &CloseRowPage, window, cx| {
            this.close_row_page(id, window, cx);
        }))
        .on_action(cx.listener(move |this, _: &OpenRowMenu, window, cx| {
            this.open_row_menu_by_key(id, window, cx);
        }))
        // On the row itself, so a press on its own buttons opens it too; the sidebar's ⋯
        // menu keeps the empty space around the rows.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_row_menu_at(id, event.position, window, cx);
            }),
        )
}

/// The shared row shape: a titled row when labels show, a centred icon button otherwise.
/// Every row is a tree item at its nesting level, keeping its title as the accessible name,
/// with P1-36's focus ring: Enter or Space uses it (a keyboard click). `tree_item` decides
/// whether it is the tree's Tab stop.
fn row_frame(id: impl Into<ElementId>, look: RowLook, labels: bool) -> Stateful<Div> {
    let row = without_mouse_focus(control(div().id(id), Role::TreeItem, look.title.clone()))
        .aria_level(look.depth + 1)
        .aria_selected(look.active)
        .key_context(ROW_CONTEXT)
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
    let row = if look.active {
        raised(row, ShadowRole::RowSelected)
    } else {
        row
    };
    let icon = row_icon(&look.icon, look.saved);
    if !labels {
        // The square anchors its close badge and is the hover group that reveals it.
        return row
            .relative()
            .group(COMPACT_ROW_GROUP)
            .w(px(ICON_BUTTON))
            .justify_center()
            .child(icon);
    }
    let indent = 8. + FOLDER_INDENT * look.depth as f32;
    row.pl(px(indent))
        .pr_1()
        .gap_2()
        .child(icon)
        .child(div().flex_1().min_w_0().truncate().child(look.title))
}

/// The placeholder favicon: a letter in a small square, accent-coloured for saved rows.
pub(super) fn row_icon(letter: &str, saved: bool) -> Div {
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

/// A row's close control: the wide row's × or the compact row's corner badge (#128), with
/// one click for both. It closes or unloads the tab and, when the row stays, lets go of row
/// focus so the row's keys do not keep applying after the click.
fn close_control(
    close: RowClose,
    labels: bool,
    shows_ring: bool,
    cx: &mut Context<BrowserView>,
) -> Stateful<Div> {
    let id = close_control_id(close, labels);
    let control = if labels {
        row_action(id, "×", close.name())
    } else {
        close_badge(id, close.name(), shows_ring)
    };
    control
        .aria_keyshortcuts(close.keys())
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.run_tab_action(close.action(), None, window, cx);
            if close.row_stays() {
                this.release_row_focus(window, cx);
            }
        }))
}

/// The close control's element id: one per tab and per layout, so it never matches its row,
/// another row's control or the other layout's. Two role-bearing siblings with one id panic
/// the accessibility tree (`docs/dev/trapstoknow.md`). The wide ids are the ones the
/// accessibility probe has already seen.
fn close_control_id(close: RowClose, labels: bool) -> ElementId {
    let name = match (close, labels) {
        (RowClose::Tab(_), true) => "close",
        (RowClose::Unload(_), true) => "unload",
        (RowClose::Tab(_), false) => "close-badge",
        (RowClose::Unload(_), false) => "unload-badge",
    };
    ElementId::from((name, close.tab().get()))
}

/// A named button inside a row. It is not a Tab stop, so the tree keeps one (DW-61), and a
/// press does not focus it.
fn row_button(id: impl Into<ElementId>, name: &'static str) -> Stateful<Div> {
    without_mouse_focus(control(div().id(id), Role::Button, name)).tab_stop(false)
}

/// A small control at the end of a wide row, named for screen readers. The row's own keys do
/// what the buttons do: Delete and F2 close, remove and rename, and Ctrl W unloads a saved
/// page or closes a tab (DW-63).
fn row_action(id: impl Into<ElementId>, glyph: &'static str, name: &'static str) -> Stateful<Div> {
    row_action_box(row_button(id, name)).child(glyph)
}

/// The square every wide row button sits in, its glyph centred.
fn row_action_box(button: Stateful<Div>) -> Stateful<Div> {
    button
        .size(px(ROW_ACTION_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
}

/// The compact row's close badge (#128): a round × on the icon's top-right corner, kept inside
/// the row's square so it never reaches the page card. It is hidden until the row is hovered
/// or `shown` (the row shows its focus ring). Hidden, GPUI paints neither it nor its mouse
/// listeners, so what the owner sees is what a click hits.
fn close_badge(id: impl Into<ElementId>, name: &'static str, shown: bool) -> Stateful<Div> {
    row_button(id, name)
        .absolute()
        .top(px(BADGE_INSET))
        .right(px(0.))
        .size(px(BADGE_SIZE))
        .rounded_full()
        .bg(theme::ground())
        .border_1()
        .border_color(theme::line())
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(BADGE_GLYPH_SIZE))
        .text_color(theme::icon())
        .cursor_pointer()
        .hover(|style| style.bg(theme::line()).text_color(theme::text()))
        .when(!shown, |badge| badge.invisible())
        .group_hover(COMPACT_ROW_GROUP, |style| style.visible())
        .child("×")
}

/// The compact row's muted badge: the mute glyph in a small circle on the icon's bottom-right
/// corner. It is drawn only, with no role, focus or click of its own; the row's own
/// description says "Muted" to screen readers.
fn muted_badge(tab: TabId) -> Stateful<Div> {
    div()
        .id(("muted-badge", tab.get()))
        .absolute()
        .bottom(px(MUTED_BADGE_INSET))
        .right(px(0.))
        .size(px(MUTED_BADGE_SIZE))
        .rounded_full()
        .bg(theme::ground())
        .border_1()
        .border_color(theme::line())
        .flex()
        .items_center()
        .justify_center()
        .font_family(glyph::FONT)
        .text_size(px(MUTED_BADGE_GLYPH_SIZE))
        .text_color(theme::icon())
        .child(glyph::MUTE)
}

/// Keeps a mouse press from focusing a row or its buttons, so a right-click, a middle-click
/// or a press released elsewhere never leaves the row's keys (Alt arrows, Delete) armed.
/// Tab and F6 still focus them. GPUI focuses a focusable element on any mouse-down unless the
/// event's default is prevented, and an element's own mouse-down listeners run before its
/// focus transfer (`gpui-pre-0.3.8/src/elements/div.rs`, the focus listener's comment).
fn without_mouse_focus(element: Stateful<Div>) -> Stateful<Div> {
    element.on_any_mouse_down(|_, window, _| window.prevent_default())
}

/// The quiet row under the tabs: New tab, and the ⋯ menu with New folder and editing
/// Favorites. A row dropped on it moves to the end of the top level, out of any folder.
fn new_tab_row(labels: bool, cx: &mut Context<BrowserView>) -> Stateful<Div> {
    let palette = theme::current();
    let row = control(div().id("new-tab"), Role::Button, "New tab")
        .aria_keyshortcuts("Control+T")
        .h(px(34.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(9.))
        .cursor_pointer()
        .text_color(theme::rgba(palette.text_muted))
        .hover(move |style| style.bg(theme::rgba(palette.row_selected)))
        .drag_over::<DraggedRow>(|style, _, _, _| style.bg(theme::preview_row_selected()))
        .on_drop(cx.listener(|this, dragged: &DraggedRow, _, cx| {
            this.drop_saved_row(dragged.entry, DropTarget::TopLevel);
            cx.notify();
        }))
        .on_click(cx.listener(|this, _, window, cx| {
            this.command(SessionCommand::NewTab {
                address: "about:blank".to_owned(),
            });
            this.queue_peek_action(super::peek::PeekAction::NewTab, window, cx);
            cx.notify();
        }));
    let plus = div()
        .size(px(22.))
        .flex_none()
        .rounded(px(7.))
        .border_1()
        .border_dashed()
        .border_color(theme::rgba(palette.dash))
        .flex()
        .items_center()
        .justify_center()
        .font_family(glyph::FONT)
        .text_size(px(10.))
        .child(glyph::ADD);
    if !labels {
        return row.w(px(ICON_BUTTON)).justify_center().child(plus);
    }
    row.px(px(8.))
        .pr_1()
        .child(plus)
        .child(div().flex_1().child("New tab"))
        .child(
            icon_button(div().id("sidebar-more"), glyph::MORE, "More")
                .tab_stop(false)
                .size(px(26.))
                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    let position = event.position();
                    this.open_sidebar_menu(position, window, cx);
                })),
        )
}

/// The library row at the sidebar's foot, as the canvas draws it: History, Bookmarks,
/// Passwords and Appearance. Bookmarks and Passwords say what is coming until they exist.
fn library_row(labels: bool, cx: &mut Context<BrowserView>) -> Div {
    let buttons = [
        icon_button(div().id("library-history"), glyph::HISTORY, "History")
            .aria_keyshortcuts("Control+H")
            .on_click(cx.listener(|this, _, window, cx| {
                let return_focus = this.chrome_return_focus(window, cx);
                this.toggle_history(return_focus, window, cx);
                cx.notify();
            })),
        icon_button(div().id("library-bookmarks"), glyph::BOOKMARKS, "Bookmarks").on_click(
            cx.listener(|this, _, _, cx| {
                this.message =
                    "Saved pages live in the sidebar; a bookmarks manager comes later.".to_owned();
                cx.notify();
            }),
        ),
        icon_button(div().id("library-passwords"), glyph::PASSWORDS, "Passwords").on_click(
            cx.listener(|this, _, _, cx| {
                this.message = "Password saving is not part of Syrinx v1.".to_owned();
                cx.notify();
            }),
        ),
        // The gear opens Settings in its own tab from any tab (owner, 2026-10-21; #91).
        icon_button(div().id("library-settings"), glyph::APPEARANCE, "Settings")
            .aria_keyshortcuts("Ctrl+,")
            .on_click(cx.listener(|this, _, _, cx| {
                this.open_settings(None);
                cx.notify();
            })),
    ];
    let row = div().flex_none().flex().text_color(theme::icon_dim());
    if !labels {
        return row.flex_col().gap_1().children(buttons);
    }
    row.items_center()
        .justify_between()
        .px_1()
        .children(buttons)
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
fn restored_sidebar_sizing(settings: &BTreeMap<String, String>) -> syrinx_ui::SidebarSizing {
    let visible = settings
        .get(LAST_VISIBLE_KEY)
        .and_then(|value| parse_sidebar_setting(value))
        .filter(|state| !state.can_peek())
        .unwrap_or(SidebarWidthState::Wide);
    let mut sizing = syrinx_ui::SidebarSizing::new(visible);
    sizing.set_state(restored_sidebar_state(settings));
    sizing
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use syrinx_ui::{SidebarSizing, sidebar::WIDE_WIDTH};

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
    fn the_close_control_id_differs_per_layout_variant_and_tab() {
        let tab = |id: u64| TabId::new(id).expect("synthetic tab id");
        let controls = [
            RowClose::Tab(tab(7)),
            RowClose::Unload(tab(7)),
            RowClose::Tab(tab(8)),
        ];
        let ids: Vec<ElementId> = controls
            .iter()
            .flat_map(|close| {
                [
                    close_control_id(*close, true),
                    close_control_id(*close, false),
                ]
            })
            .collect();
        let distinct: HashSet<&ElementId> = ids.iter().collect();
        assert_eq!(distinct.len(), ids.len(), "{ids:?}");
        // The wide × keeps the ids the accessibility probe has already seen.
        assert_eq!(
            close_control_id(RowClose::Tab(tab(7)), true),
            ElementId::from(("close", 7u64))
        );
        assert_eq!(
            close_control_id(RowClose::Unload(tab(7)), true),
            ElementId::from(("unload", 7u64))
        );
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

//! Draws the sidebar at its current D44 width and lets the owner drag its edge (P1-08, P1-09),
//! with the saved rows' folder controls (P1-21).

use std::collections::BTreeMap;

use gpui::{
    App, AppContext, ClickEvent, Context, Div, ElementId, FocusHandle, FontWeight,
    InteractiveElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement,
    Role, Stateful, StatefulInteractiveElement, Styled, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};
use syrinx_core::{
    ActivationTarget, BuiltInPlugin, DestinationTarget, SavedEntryId, SavedFolderSnapshot,
    SavedPageSnapshot, TabId, TabSnapshot,
};
use syrinx_ui::{
    SIDEBAR_WIDTH_STATE_KEY, SidebarWidthState,
    metrics::chrome::ICON_BUTTON,
    parse_sidebar_setting,
    sidebar::{SIDEBAR_PADDING_LEFT, SIDEBAR_PADDING_RIGHT, draws_content},
    sidebar_setting,
};

use crate::controls::{control, glyph, icon_button};

use super::{
    BrowserView,
    favorites::{DraggedFavorite, FavoriteSource},
    folder_editor::{
        DeleteFolderContents, DraggedRow, MoveRowDown, MoveRowIn, MoveRowOut, MoveRowUp,
        ROW_CONTEXT, RemoveRow, RenameRow,
    },
    menus::MenuKind,
    page_card::edge_strip,
    saved_moves::{DropTarget, Step},
    sidebar_tree::{SidebarEntry, TreeId, active_tree_id, sidebar_entries},
    tiles::{self, monogram},
};
use crate::{session::SessionCommand, theme};

const ROW_HEIGHT: f32 = 36.;
/// How far each folder level indents its contents in the wide sidebar.
const FOLDER_INDENT: f32 = 12.;
const MONOGRAM_SIZE: f32 = 20.;
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
            .child(self.favorites(labels, cx))
            .children(labels.then(|| self.folder_editor_view(cx)).flatten())
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
            let row = match entry {
                SidebarEntry::Folder {
                    folder,
                    depth,
                    expanded,
                } => self.folder_row(folder, *depth, *expanded, labels, cx),
                SidebarEntry::Page { page, depth } => self.saved_row(page, *depth, labels, cx),
                SidebarEntry::Tab(tab) => {
                    let title = ordinary_title(tab, self.popup_pages.contains(&tab.id));
                    self.ordinary_row(tab.id, title, labels, cx)
                }
            };
            rows = rows.child(tree_item(row, entry.tree_row().id, &focus, cx));
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
        let target = ActivationTarget::Saved(saved_id);
        let row = row_frame(("saved", saved_id.get()), look, labels).on_click(cx.listener(
            move |this, _, window, cx| {
                this.activate_row(target, window, cx);
            },
        ));
        // The confirmation for an unopened page needs the wide sidebar's card.
        let row = if labels || live.is_some() {
            row.on_action(cx.listener(move |this, _: &RemoveRow, window, cx| {
                this.request_page_removal(saved_id, window, cx);
            }))
        } else {
            row
        };
        let mut row = movable_row(row, SavedEntryId::Page(saved_id), title, cx);
        if !labels {
            return row;
        }
        if let Some(id) = live {
            row = row.child(
                row_action(("unload", id.get()), "×", "Unload page").on_click(cx.listener(
                    move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.command(SessionCommand::CloseTab(id));
                        // The row stays, so its keys must not keep applying after the click.
                        this.release_row_focus(window, cx);
                        cx.notify();
                    },
                )),
            );
        }
        row.child(
            row_action(("unsave", saved_id.get()), "−", "Remove saved page")
                .text_color(theme::icon_dim())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.request_page_removal(saved_id, window, cx);
                })),
        )
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
            .on_action(cx.listener(move |this, _: &RemoveRow, _, cx| {
                this.command(SessionCommand::CloseTab(id));
                cx.notify();
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
/// arrow keys move focus between rows.
fn tree_item(
    row: Stateful<Div>,
    id: TreeId,
    focus: &FocusHandle,
    cx: &mut Context<BrowserView>,
) -> Stateful<Div> {
    row.track_focus(focus).on_key_down(cx.listener(
        move |this, event: &KeyDownEvent, window, cx| {
            this.tree_key_down(id, event, window, cx);
        },
    ))
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

/// A small control at the end of a wide row, named for screen readers. It is not a Tab stop,
/// so the tree keeps one (DW-61). The row's Delete and F2 close, remove and rename; only
/// unloading a saved page has no key of its own (Enter, then Ctrl W).
fn row_action(id: impl Into<ElementId>, glyph: &'static str, name: &'static str) -> Stateful<Div> {
    without_mouse_focus(control(
        div().id(id).px_1().rounded_md(),
        Role::Button,
        name,
    ))
    .tab_stop(false)
    .child(glyph)
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
            this.queue_peek_action(super::peek::PeekAction::Dismiss, window, cx);
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
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_history();
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
        icon_button(
            div().id("library-appearance"),
            glyph::APPEARANCE,
            "Appearance",
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.open_appearance();
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

//! Draws the sidebar at its current D44 width and lets the owner drag its edge (P1-08, P1-09).

use std::collections::HashMap;

use atlas_core::{
    SavedFolderId, SavedFolderSnapshot, SavedPageId, SavedPageSnapshot, TabId, TabSnapshot,
};
use atlas_ui::{
    metrics::chrome::{CARD_INSET, ICON_BUTTON},
    sidebar::{SIDEBAR_PADDING_LEFT, SIDEBAR_PADDING_RIGHT},
};
use gpui::{
    Context, Div, ElementId, InteractiveElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement, Role, Stateful, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder,
    px,
};

use super::{
    BrowserView, button,
    sidebar_rows::{SavedRow, monogram, visible_saved_rows},
};
use crate::{session::SessionCommand, theme};

const ROW_HEIGHT: f32 = 36.;
/// How far each folder level indents its contents in the wide sidebar.
const FOLDER_INDENT: f32 = 12.;
const MONOGRAM_SIZE: f32 = 20.;
const PLUGINS: [&str; 3] = ["Forge", "Notes", "Fledge"];

/// What a row shows: its icon letter, title and selection, and how deep it is nested.
struct RowLook {
    icon: String,
    title: String,
    active: bool,
    depth: usize,
    saved: bool,
}

impl BrowserView {
    /// The sidebar at the width the sizing reserves; a hidden sidebar draws nothing.
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let width = self.sidebar_sizing.width();
        let frame = div()
            .w(px(width))
            .flex_none()
            .h_full()
            .overflow_hidden()
            .bg(theme::ground());
        if width <= 0. {
            return frame;
        }
        let labels = self.sidebar_sizing.labels_visible();
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
            .child(plugin_section(labels))
            .child(div().h(px(1.)).bg(theme::line()))
            .child(tabs_header(labels, cx))
            .child(
                div()
                    .id("tab-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(self.rows(labels, cx)),
            )
    }

    /// The strip between the sidebar and the page card that the owner drags.
    pub(super) fn sidebar_edge(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let dragging = self.sidebar_sizing.is_dragging();
        div()
            .id("sidebar-edge")
            .role(Role::Splitter)
            .aria_label("Resize sidebar")
            .w(px(CARD_INSET))
            .flex_none()
            .h_full()
            .cursor_col_resize()
            .when(dragging, |edge| edge.bg(theme::accent()))
            .hover(|style| style.bg(theme::line()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.sidebar_sizing.begin_drag(f32::from(event.position.x));
                    cx.notify();
                }),
            )
    }

    /// Follows the pointer while the edge is held. GPUI captures the mouse on press, so moves
    /// over the page's web view still arrive here.
    pub(super) fn sidebar_drag_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.sidebar_sizing.is_dragging() {
            return;
        }
        // A release outside every window can be lost; the next move without a button ends it.
        if !event.dragging() {
            self.sidebar_drag_released(cx);
            return;
        }
        if self.sidebar_sizing.drag_to(f32::from(event.position.x)) {
            cx.notify();
        }
    }

    /// Snaps a released drag to wide, favicon-only or hidden. Selection is untouched.
    pub(super) fn sidebar_drag_released(&mut self, cx: &mut Context<Self>) {
        // TODO(DW-2): persist the settled state once atlas-store has a settings table (P1-29).
        if self.sidebar_sizing.release().is_some() {
            cx.notify();
        }
    }

    /// Saved rows in layout order (skipping collapsed folders), then ordinary tabs.
    fn rows(&self, labels: bool, cx: &mut Context<Self>) -> Div {
        let pages: HashMap<SavedPageId, &SavedPageSnapshot> = self
            .model
            .saved_pages
            .iter()
            .map(|page| (page.id, page))
            .collect();
        let folders: HashMap<SavedFolderId, &SavedFolderSnapshot> = self
            .model
            .saved_folders
            .iter()
            .map(|folder| (folder.id, folder))
            .collect();
        let mut rows = div().flex().flex_col().gap_1();
        for row in visible_saved_rows(&self.model.saved_layout, &self.collapsed_folders) {
            let element = match row {
                SavedRow::Folder {
                    id,
                    depth,
                    expanded,
                } => folders
                    .get(&id)
                    .map(|folder| folder_row(folder, depth, expanded, labels, cx)),
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
            rows = rows.child(self.ordinary_row(tab.id, ordinary_title(tab), labels, cx));
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
            title,
            active: live.is_some() && live == self.model.active_id,
            depth,
            saved: true,
        };
        let mut row = row_frame(("saved", saved_id.get()), look, labels).on_click(cx.listener(
            move |this, _, _, cx| {
                this.command(SessionCommand::OpenSaved(saved_id));
                cx.notify();
            },
        ));
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
        let row = row_frame(("tab", id.get()), look, labels).on_click(cx.listener(
            move |this, _, _, cx| {
                this.command(SessionCommand::FocusTab(id));
                cx.notify();
            },
        ));
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

    /// Expands or collapses one folder's rows. This is view state, not a model change.
    fn toggle_folder(&mut self, id: SavedFolderId) {
        if !self.collapsed_folders.remove(&id) {
            self.collapsed_folders.insert(id);
        }
    }
}

/// A folder row: a disclosure arrow that expands or collapses its contents.
fn folder_row(
    folder: &SavedFolderSnapshot,
    depth: usize,
    expanded: bool,
    labels: bool,
    cx: &mut Context<BrowserView>,
) -> Stateful<Div> {
    let id = folder.id;
    let look = RowLook {
        icon: if expanded { "▾" } else { "▸" }.to_owned(),
        title: folder.title.clone(),
        active: false,
        depth,
        saved: true,
    };
    row_frame(("folder", id.get()), look, labels)
        .role(Role::TreeItem)
        .aria_expanded(expanded)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.toggle_folder(id);
            cx.notify();
        }))
}

/// The shared row shape: a titled row when labels show, a centred icon button otherwise.
/// Both keep the title as the accessible name.
fn row_frame(id: impl Into<ElementId>, look: RowLook, labels: bool) -> Stateful<Div> {
    let row = div()
        .id(id)
        .role(Role::Tab)
        .aria_label(look.title.clone())
        .aria_selected(look.active)
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

/// The tab section's header with its new-tab button.
fn tabs_header(labels: bool, cx: &mut Context<BrowserView>) -> Div {
    let new_tab = cx.listener(|this, _, _, cx| {
        this.command(SessionCommand::NewTab {
            address: "about:blank".to_owned(),
        });
        cx.notify();
    });
    let header = div().flex().items_center();
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

/// A saved page shows its title, or its address when it has none.
fn title_or_address(title: &str, address: &str) -> String {
    if title.is_empty() {
        address.to_owned()
    } else {
        title.to_owned()
    }
}

/// An ordinary tab's title; a blank tab is Home.
fn ordinary_title(tab: &TabSnapshot) -> String {
    if tab.metadata.address == "about:blank" {
        return "Home".to_owned();
    }
    title_or_address(&tab.metadata.title, &tab.metadata.address)
}

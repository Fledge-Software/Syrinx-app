//! Draws the canvas home page: name, Customize, the search pill, Quick access and plugins (P1-28).

use atlas_core::{
    BuiltInPlugin, Destination, DestinationEdit, DestinationSurface, QuickDestinations,
};
use atlas_ui::metrics::{chrome, radius, text};
use atlas_ui::{Color, Palette};
use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, div, px,
};

use super::BrowserView;
use super::customize::CustomAccent;
use super::tile_editor::TileEditor;
use super::tiles::{self, TileAction};
use crate::appearance::Appearance;
use crate::session::SessionCommand;
use crate::theme::{self, rgba};

/// The home grid's columns (spec: "8 per row").
const QUICK_ACCESS_COLUMNS: u16 = 8;
const QUICK_TILE_SIZE: f32 = 72.;
const QUICK_TILE_RADIUS: f32 = 18.;
const SEARCH_PILL_MAX_WIDTH: f32 = 760.;

/// Home, Customize and tile-editing state, owned by the browser window.
#[derive(Default)]
pub(super) struct HomeState {
    pub(super) appearance: Appearance,
    pub(super) destinations: QuickDestinations,
    pub(super) customize_open: bool,
    /// The surface whose tiles show move and remove controls.
    pub(super) editing: Option<DestinationSurface>,
    pub(super) editor: Option<TileEditor>,
    pub(super) custom_accent: Option<CustomAccent>,
}

impl BrowserView {
    /// The page card shown when no web page is: the canvas home board.
    pub(super) fn home_page(&self, cx: &mut Context<Self>) -> Div {
        let palette = theme::current();
        let mut card = div()
            .relative()
            .size_full()
            .rounded(px(radius::HOME_CARD))
            .bg(rgba(palette.card))
            .border_1()
            .border_color(rgba(palette.card_line))
            .overflow_hidden()
            .child(
                div()
                    .id("home-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .px(px(64.))
                    .pt(px(28.))
                    .pb(px(32.))
                    .flex()
                    .flex_col()
                    .child(self.home_header(&palette, cx))
                    .child(self.search_pill(&palette, cx))
                    .child(self.quick_access(&palette, cx))
                    .child(self.plugin_strip(&palette, cx)),
            );
        if self.home.customize_open {
            card = card.child(self.customize_panel(&palette, cx));
        }
        div()
            .flex_1()
            .h_full()
            .min_w_0()
            .pr(px(chrome::CARD_INSET))
            .pb(px(chrome::CARD_INSET))
            .child(card)
    }

    /// Whether the visible page is Home, so a web tile reuses it instead of adding a tab.
    pub(super) fn home_showing(&self) -> bool {
        self.shown.is_none() && self.active_address().is_empty()
    }

    /// Runs a tile's action: open its page, or say its plugin is not available yet.
    pub(super) fn open_tile(&mut self, tile: &Destination) {
        match tiles::open_action(tile, self.home_showing()) {
            TileAction::Session(command) => self.command(command),
            TileAction::Unavailable(plugin) => self.message = tiles::unavailable_message(plugin),
        }
    }

    pub(super) fn edit_tiles(&mut self, surface: DestinationSurface, edit: DestinationEdit) {
        self.command(SessionCommand::EditDestinations { surface, edit });
    }

    fn home_header(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let customize_background = if self.home.customize_open {
            palette.row_selected
        } else {
            Color::rgba(0, 0, 0, 0.)
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .size(px(30.))
                    .rounded(px(9.))
                    .bg(rgba(palette.accent.fill))
                    .text_color(rgba(palette.accent.on_fill))
                    .font_weight(FontWeight::BOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("A"),
            )
            .child(
                div()
                    .text_size(px(text::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Atlas"),
            )
            // The canvas's local date waits for a calendar helper in `atlas-platform`, the only
            // crate allowed to call Win32; a UTC date would be wrong each evening.
            .child(div().flex_1())
            .child(
                div()
                    .id("customize")
                    .h(px(30.))
                    .px(px(12.))
                    .rounded(px(15.))
                    .border_1()
                    .border_color(rgba(palette.pill_line))
                    .bg(rgba(customize_background))
                    .text_color(rgba(palette.text_secondary))
                    .text_size(px(text::ADDRESS))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child("Customize")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.home.customize_open = !this.home.customize_open;
                        this.home.custom_accent = None;
                        cx.notify();
                    })),
            )
    }

    fn search_pill(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        // The pill opens the palette, so it is a button, not a second text field.
        let pill = div()
            .id("home-search")
            .w_full()
            .max_w(px(SEARCH_PILL_MAX_WIDTH))
            .h(px(54.))
            .rounded(px(27.))
            .bg(rgba(palette.pill))
            .border_1()
            .border_color(rgba(palette.pill_line))
            .pl(px(22.))
            .pr(px(16.))
            .flex()
            .items_center()
            .gap(px(14.))
            .text_color(rgba(palette.text_muted))
            .cursor_text()
            .child(div().text_size(px(18.)).child("⌕"))
            .child(
                div()
                    .flex_1()
                    .text_size(px(text::SUBTITLE))
                    .child("Search the web, your tabs, notes and plugins"),
            )
            .child(key_hint(palette, "Ctrl K"))
            .on_click(cx.listener(|this, _, window, cx| this.open_palette(false, window, cx)));
        div()
            .mt(px(60.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .child(pill)
            .child(
                div()
                    .flex()
                    .gap(px(24.))
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_muted))
                    .child(prefix_hint(palette, ">", "commands"))
                    .child(prefix_hint(palette, "@", "plugins"))
                    .child(prefix_hint(palette, "/", "open tabs")),
            )
    }

    fn quick_access(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let surface = DestinationSurface::QuickAccess;
        let editing = self.home.editing == Some(surface);
        let tiles = self.home.destinations.list(surface);
        let mut grid = div()
            .grid()
            .grid_cols(QUICK_ACCESS_COLUMNS)
            .gap_x(px(10.))
            .gap_y(px(18.));
        for (index, tile) in tiles.iter().enumerate() {
            grid = grid.child(self.quick_tile(palette, tile, index, tiles.len(), editing, cx));
        }
        grid = grid.child(self.add_quick_tile(palette, cx));
        let mut section = div()
            .mt(px(48.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                section_heading(palette, "Quick access")
                    .child(div().flex_1())
                    .child(self.edit_toggle("quick-edit", surface, palette, cx)),
            )
            .child(grid);
        if let Some(editor) = self
            .home
            .editor
            .as_ref()
            .filter(|editor| editor.surface == surface)
        {
            section = section.child(self.tile_editor(editor, palette, cx));
        }
        section
    }

    fn quick_tile(
        &self,
        palette: &Palette,
        tile: &Destination,
        index: usize,
        count: usize,
        editing: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let surface = DestinationSurface::QuickAccess;
        let opened = tile.clone();
        let mut cell = div()
            .id(("quick-tile", index))
            .min_w_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .text_color(rgba(palette.text_secondary))
            .cursor_pointer()
            .child(tiles::tile_face(
                palette,
                tile,
                QUICK_TILE_SIZE,
                QUICK_TILE_RADIUS,
                text::DISPLAY,
            ))
            .child(
                div()
                    .max_w_full()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(text::SMALL))
                    .child(tile.label.clone()),
            );
        if editing {
            // In edit mode a click edits the tile instead of opening it.
            cell = cell
                .child(self.tile_controls(palette, surface, index, count, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.start_tile_editor(surface, Some(index), window, cx);
                }));
        } else {
            cell = cell.on_click(cx.listener(move |this, _, _, cx| {
                this.open_tile(&opened);
                cx.notify();
            }));
        }
        cell
    }

    fn add_quick_tile(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("quick-add")
            .min_w_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .text_color(rgba(palette.text_muted))
            .cursor_pointer()
            .child(tiles::add_face(palette, QUICK_TILE_SIZE, QUICK_TILE_RADIUS))
            .child(div().text_size(px(text::SMALL)).child("Add shortcut"))
            .on_click(cx.listener(|this, _, window, cx| {
                this.start_tile_editor(DestinationSurface::QuickAccess, None, window, cx);
            }))
    }

    /// Move-left, remove and move-right buttons under a tile in edit mode.
    pub(super) fn tile_controls(
        &self,
        palette: &Palette,
        surface: DestinationSurface,
        index: usize,
        count: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut controls = div().flex().gap(px(2.));
        if index > 0 {
            controls = controls.child(small_button(("tile-left", index), "‹", palette).on_click(
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.edit_tiles(
                        surface,
                        DestinationEdit::Move {
                            from: index,
                            to: index - 1,
                        },
                    );
                    cx.notify();
                }),
            ));
        }
        controls = controls.child(small_button(("tile-remove", index), "×", palette).on_click(
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.edit_tiles(surface, DestinationEdit::Remove { index });
                cx.notify();
            }),
        ));
        if index + 1 < count {
            controls = controls.child(small_button(("tile-right", index), "›", palette).on_click(
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.edit_tiles(
                        surface,
                        DestinationEdit::Move {
                            from: index,
                            to: index + 1,
                        },
                    );
                    cx.notify();
                }),
            ));
        }
        controls
    }

    /// The "Edit"/"Done" switch for one surface's edit mode.
    pub(super) fn edit_toggle(
        &self,
        id: &'static str,
        surface: DestinationSurface,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let editing = self.home.editing == Some(surface);
        div()
            .id(id)
            .h(px(28.))
            .px(px(10.))
            .rounded(px(radius::CONTROL))
            .flex()
            .items_center()
            .text_size(px(text::ADDRESS))
            .text_color(rgba(if editing {
                palette.accent.ink
            } else {
                palette.icon_dim
            }))
            .cursor_pointer()
            .child(if editing { "Done" } else { "Edit" })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.home.editing = if editing { None } else { Some(surface) };
                cx.notify();
            }))
    }

    fn plugin_strip(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let mut cards = div().grid().grid_cols(3).gap(px(14.));
        for plugin in BuiltInPlugin::ALL {
            cards = cards.child(plugin_card(palette, plugin).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.message = tiles::unavailable_message(plugin);
                    cx.notify();
                },
            )));
        }
        div()
            .mt(px(36.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                section_heading(palette, "Plugins").child(
                    div()
                        .text_size(px(text::SMALL))
                        .text_color(rgba(palette.text_muted))
                        .child("Built in. Each opens as a tab once the plugin runtime exists."),
                ),
            )
            .child(cards)
    }
}

fn plugin_card(palette: &Palette, plugin: BuiltInPlugin) -> Stateful<Div> {
    let (colors, description) = match plugin {
        BuiltInPlugin::Forge => (palette.plugins.forge, "Code editor, git and terminal."),
        BuiltInPlugin::Notes => (
            palette.plugins.notes,
            "Linked pages, databases and backlinks.",
        ),
        BuiltInPlugin::Fledge => (palette.plugins.fledge, "Leads, mail, calendar and docs."),
    };
    div()
        .id(plugin.id())
        .min_w_0()
        .rounded(px(radius::HOME_CARD))
        .bg(rgba(palette.surface))
        .border_1()
        .border_color(rgba(palette.surface_line))
        .px(px(16.))
        .py(px(14.))
        .flex()
        .gap(px(12.))
        .cursor_pointer()
        .child(
            div()
                .size(px(34.))
                .flex_none()
                .rounded(px(radius::TILE))
                .bg(rgba(colors.background))
                .text_color(rgba(colors.foreground))
                .font_weight(FontWeight::SEMIBOLD)
                .opacity(0.55)
                .flex()
                .items_center()
                .justify_center()
                .child(tiles::monogram(plugin.name())),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(plugin.name()))
                .child(
                    div()
                        .text_size(px(text::SMALL))
                        .text_color(rgba(palette.text_muted))
                        .child(description),
                )
                .child(
                    div()
                        .text_size(px(text::SMALL))
                        .text_color(rgba(palette.text_secondary))
                        .child("Not available yet"),
                ),
        )
}

fn section_heading(palette: &Palette, title: &'static str) -> Div {
    div().flex().items_center().gap(px(14.)).child(
        div()
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(palette.text))
            .child(title),
    )
}

fn key_hint(palette: &Palette, label: &'static str) -> Div {
    div()
        .px(px(6.))
        .rounded(px(radius::KEY_HINT))
        .border_1()
        .border_b_2()
        .border_color(rgba(palette.key))
        .text_size(px(text::CAPTION))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgba(palette.icon_dim))
        .child(label)
}

fn prefix_hint(palette: &Palette, prefix: &'static str, label: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .font_family(text::MONO_FONT_FAMILIES[0])
                .text_color(rgba(palette.text_secondary_bright))
                .child(prefix),
        )
        .child(label)
}

/// A compact glyph button used by tile edit controls.
pub(super) fn small_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    palette: &Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .min_w(px(20.))
        .h(px(20.))
        .px(px(4.))
        .rounded(px(radius::XS))
        .bg(rgba(palette.field))
        .text_color(rgba(palette.text_secondary))
        .text_size(px(text::SMALL))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .child(label)
}

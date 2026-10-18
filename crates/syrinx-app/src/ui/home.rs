//! Draws the canvas home page: name, Customize, the search pill, Quick access and plugins (P1-28).

use gpui::{
    Context, Div, FontWeight, Image, ImageFormat, InteractiveElement, ObjectFit, ParentElement,
    Role, Stateful, StatefulInteractiveElement, Styled, StyledImage, Toggled, Window, div, img, px,
};
use std::sync::{Arc, OnceLock};

use syrinx_core::usage::Feature;
use syrinx_core::{
    BuiltInPlugin, Destination, DestinationEdit, DestinationSurface, QuickDestinations,
};
use syrinx_ui::metrics::{chrome, radius, text};
use syrinx_ui::{Color, Palette};

use super::BrowserView;
use super::customize::CustomAccent;
use super::site_permissions::SitePermissions;
use super::theme_editor::ThemeEditor;
use super::tile_editor::TileEditor;
use super::tiles::{self, TileAction};
use crate::appearance::{Appearance, SavedThemes};
use crate::controls::control;
use crate::session::SessionCommand;
use crate::theme::{self, rgba};

/// The home grid's columns (spec: "8 per row").
const QUICK_ACCESS_COLUMNS: u16 = 8;
const QUICK_TILE_SIZE: f32 = 72.;
const QUICK_TILE_RADIUS: f32 = 18.;
const SEARCH_PILL_MAX_WIDTH: f32 = 760.;
const LOGO_TILE_SIZE: f32 = 30.;

/// The brand's small-variant icon tile at 64 px (`design/brand/icon`): the brand asks for the small
/// variant at 32 px and below, and this mark is 30 px, drawn at twice that for sharp HiDPI.
static LOGO_PNG: &[u8] = include_bytes!("../../../../design/brand/icon/syrinx-icon-small-64.png");

/// The Syrinx bird tile, built once so each frame clones an Arc instead of copying and hashing the bytes.
fn logo_image() -> Arc<Image> {
    static LOGO: OnceLock<Arc<Image>> = OnceLock::new();
    LOGO.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, LOGO_PNG.to_vec())))
        .clone()
}

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
    /// The owner's saved themes, and the one whose controls are open.
    pub(super) themes: SavedThemes,
    pub(super) theme_editor: Option<ThemeEditor>,
    /// Customize's list of remembered site permissions (DW-7).
    pub(super) site_permissions: SitePermissions,
}

impl BrowserView {
    /// The page card shown when no web page is: the canvas home board.
    pub(super) fn home_page(&self, window: &Window, cx: &mut Context<Self>) -> Div {
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
                    .child(self.quick_access(&palette, window, cx))
                    .child(self.plugin_strip(&palette, cx)),
            );
        if self.home.customize_open {
            card = card.child(self.customize_panel(&palette, window, cx));
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
    pub(super) fn open_tile(&mut self, surface: DestinationSurface, tile: &Destination) {
        self.count(match surface {
            DestinationSurface::QuickAccess => Feature::HomeTileOpened,
            DestinationSurface::Favorites => Feature::FavoriteOpened,
        });
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
                img(logo_image())
                    .size(px(LOGO_TILE_SIZE))
                    .object_fit(ObjectFit::Contain),
            )
            .child(
                div()
                    .text_size(px(text::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Syrinx"),
            )
            .child(div().flex_1())
            .child(home_date(palette))
            .child(
                control(div().id("customize"), Role::Button, "Customize")
                    .aria_expanded(self.home.customize_open)
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
                        if this.home.customize_open {
                            this.close_customize();
                        } else {
                            this.count(Feature::SettingsOpened);
                            this.home.customize_open = true;
                        }
                        cx.notify();
                    })),
            )
    }

    fn search_pill(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        // The pill opens the palette, so it is a button, not a second text field.
        let pill = control(div().id("home-search"), Role::Button, "Search")
            .aria_description("Search the web, your tabs, notes and plugins")
            .aria_keyshortcuts("Control+K")
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

    fn quick_access(&self, palette: &Palette, window: &Window, cx: &mut Context<Self>) -> Div {
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
            section = section.child(self.tile_editor(editor, palette, window, cx));
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
        let name = if editing {
            format!("Edit {}", tile.label)
        } else {
            tile.label.to_string()
        };
        let mut cell = control(div().id(("quick-tile", index)), Role::Button, name)
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
                .child(self.tile_controls(palette, surface, (index, tile), count, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.start_tile_editor(surface, Some(index), window, cx);
                }));
        } else {
            cell = cell.on_click(cx.listener(move |this, _, _, cx| {
                this.open_tile(surface, &opened);
                cx.notify();
            }));
        }
        cell
    }

    fn add_quick_tile(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        control(div().id("quick-add"), Role::Button, "Add shortcut")
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

    /// Move-left, remove and move-right buttons under a tile in edit mode. Each edit carries the
    /// tile it was made against, so a repeated or late click cannot hit the tile that moved up.
    pub(super) fn tile_controls(
        &self,
        palette: &Palette,
        surface: DestinationSurface,
        (index, tile): (usize, &Destination),
        count: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let edit_button = |id: &'static str, glyph: &'static str, edit: DestinationEdit| {
            let name = match id {
                "tile-left" => format!("Move {} left", tile.label),
                "tile-right" => format!("Move {} right", tile.label),
                _ => format!("Remove {}", tile.label),
            };
            small_button((id, index), glyph, palette)
                .aria_label(name)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.edit_tiles(surface, edit.clone());
                    cx.notify();
                }))
        };
        let moved = |to: usize| DestinationEdit::Move {
            from: index,
            to,
            expected: tile.clone(),
        };
        let mut controls = div().flex().gap(px(2.));
        if index > 0 {
            controls = controls.child(edit_button("tile-left", "‹", moved(index - 1)));
        }
        let removed = DestinationEdit::Remove {
            index,
            expected: tile.clone(),
        };
        controls = controls.child(edit_button("tile-remove", "×", removed));
        if index + 1 < count {
            controls = controls.child(edit_button("tile-right", "›", moved(index + 1)));
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
        let name = match (editing, surface) {
            (true, _) => "Done editing",
            (false, DestinationSurface::Favorites) => "Edit Favorites",
            (false, DestinationSurface::QuickAccess) => "Edit Quick access",
        };
        control(div().id(id), Role::Button, name)
            .aria_toggled(if editing {
                Toggled::True
            } else {
                Toggled::False
            })
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
    let name = format!("{} · not available yet", plugin.name());
    control(div().id(plugin.id()), Role::Button, name)
        .aria_description(description)
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

/// The header's local date, or a plain notice when Windows cannot format it (the error goes to
/// stderr). Read at each render, so an idle window keeps yesterday's date until it redraws.
fn local_date() -> String {
    match syrinx_platform::calendar::local_date_heading() {
        Ok(date) => date,
        Err(error) => {
            eprintln!("Syrinx could not format the local date: {error}");
            "Date unavailable".to_owned()
        }
    }
}

/// The header's date as static text a screen reader reads as "Today, <date>".
fn home_date(palette: &Palette) -> Stateful<Div> {
    let date = local_date();
    div()
        .id("home-date")
        .role(Role::Label)
        .aria_label(format!("Today, {date}"))
        .text_color(rgba(palette.icon_dim))
        .text_size(px(text::ADDRESS))
        .child(date)
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
    control(div().id(id), Role::Button, label)
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

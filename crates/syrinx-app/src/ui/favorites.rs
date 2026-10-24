//! Draws the sidebar's square Favorites, separate from saved pages and Quick access (D45),
//! and adds a tab, saved page or plugin dragged onto them.

use gpui::{
    Context, Div, InteractiveElement, IntoElement, ParentElement, Render, Role, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use syrinx_core::{
    BuiltInPlugin, Destination, DestinationEdit, DestinationSurface, MAX_LABEL_CHARS, SavedEntryId,
};
use syrinx_ui::Palette;
use syrinx_ui::metrics::chrome::ICON_BUTTON;

use super::BrowserView;
use super::folder_editor::{DraggedRow, drag_chip};
use super::tile_editor::{TargetChoice, tile_from_form};
use super::tiles;
use crate::controls::control;
use crate::theme;

/// A tab or plugin being dragged towards Favorites, and the label that follows the pointer.
#[derive(Clone)]
pub(super) struct DraggedFavorite {
    pub(super) title: String,
    pub(super) source: FavoriteSource,
}

/// What a dropped favorite opens.
#[derive(Clone)]
pub(super) enum FavoriteSource {
    Address(String),
    Plugin(BuiltInPlugin),
}

impl Render for DraggedFavorite {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        drag_chip(&self.title)
    }
}

/// The canvas sidebar's four-column grid of 46px-high tiles.
const COLUMNS: u16 = 4;
const TILE_HEIGHT: f32 = 46.;
const TILE_RADIUS: f32 = 12.;
const LETTER_SIZE: f32 = 17.;
/// Favicon-only mode's single column of icon-button-sized tiles.
const COMPACT_RADIUS: f32 = 8.;
const COMPACT_LETTER_SIZE: f32 = 13.;

impl BrowserView {
    /// Favorites at the sidebar's width: the full section when labels show, otherwise one
    /// column of icon tiles that fits the favicon-only sidebar.
    pub(super) fn favorites(&self, labels: bool, window: &Window, cx: &mut Context<Self>) -> Div {
        let favorites = if labels {
            self.favorites_section(window, cx)
        } else {
            self.favorites_column(cx)
        };
        favorites_drop_target(favorites, cx)
    }

    /// Adds a dropped tab or plugin as the last Favorite, or says why it cannot be one.
    fn drop_favorite(&mut self, title: &str, source: &FavoriteSource) {
        let tile = match favorite_tile(title, source) {
            Ok(tile) => tile,
            Err(message) => {
                self.message = message;
                return;
            }
        };
        let surface = DestinationSurface::Favorites;
        if is_favorite(self.home.destinations.list(surface), &tile) {
            self.message = format!("{} is already a favorite.", tile.label);
            return;
        }
        self.edit_tiles(surface, DestinationEdit::Add(tile));
    }

    /// A saved page dropped on Favorites becomes one; a folder cannot.
    fn drop_saved_favorite(&mut self, entry: SavedEntryId) {
        let SavedEntryId::Page(id) = entry else {
            self.message = "A folder cannot be a favorite. Drag one of its pages.".to_owned();
            return;
        };
        let Some(page) = self.model.saved_pages.iter().find(|page| page.id == id) else {
            return;
        };
        let source = FavoriteSource::Address(page.metadata.address.clone());
        let title = page.display_title();
        self.drop_favorite(&title, &source);
    }

    /// The Favorites grid and, while one is open, the Favorites tile form. Editing starts
    /// from the sidebar's menu, so there is no heading.
    fn favorites_section(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let palette = theme::current();
        let surface = DestinationSurface::Favorites;
        let mut section = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(self.favorites_grid(&palette, cx));
        if let Some(editor) = self
            .home
            .editor
            .as_ref()
            .filter(|editor| editor.surface == surface)
        {
            section = section.child(self.tile_editor(editor, &palette, window, cx));
        }
        section
    }

    fn favorites_grid(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let surface = DestinationSurface::Favorites;
        let editing = self.home.editing == Some(surface);
        let favorites = self.home.destinations.list(surface);
        let mut grid = div().grid().grid_cols(COLUMNS).gap(px(8.));
        for (index, tile) in favorites.iter().enumerate() {
            let opened = tile.clone();
            let name = if editing {
                format!("Edit {}", tile.label)
            } else {
                tile.label.to_string()
            };
            let mut cell = control(div().id(("favorite", index)), Role::Button, name)
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .child(
                    tiles::tile_face(palette, tile, TILE_HEIGHT, TILE_RADIUS, LETTER_SIZE).w_full(),
                );
            cell = if editing {
                cell.child(self.tile_controls(palette, surface, (index, tile), favorites.len(), cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.start_tile_editor(surface, Some(index), window, cx);
                    }))
            } else {
                cell.on_click(cx.listener(move |this, _, window, cx| {
                    this.open_tile(DestinationSurface::Favorites, &opened);
                    this.queue_peek_action(super::peek::PeekAction::Dismiss, window, cx);
                    cx.notify();
                }))
            };
            grid = grid.child(cell);
        }
        grid.child(self.add_favorite(palette, cx))
    }

    fn add_favorite(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        control(div().id("favorite-add"), Role::Button, "Add favorite")
            .cursor_pointer()
            .child(tiles::add_face(palette, TILE_HEIGHT, TILE_RADIUS).w_full())
            .on_click(cx.listener(|this, _, window, cx| {
                this.start_tile_editor(DestinationSurface::Favorites, None, window, cx);
            }))
    }

    /// Favicon-only Favorites: each tile as a named icon that opens it. Editing needs the
    /// wide sidebar, so the add tile and the tile form are left out here.
    fn favorites_column(&self, cx: &mut Context<Self>) -> Div {
        let palette = theme::current();
        let favorites = self.home.destinations.list(DestinationSurface::Favorites);
        let mut column = div().flex().flex_col().gap_1();
        for (index, tile) in favorites.iter().enumerate() {
            let opened = tile.clone();
            column = column.child(
                control(
                    div().id(("favorite", index)),
                    Role::Button,
                    tile.label.clone(),
                )
                .cursor_pointer()
                .child(tiles::tile_face(
                    &palette,
                    tile,
                    ICON_BUTTON,
                    COMPACT_RADIUS,
                    COMPACT_LETTER_SIZE,
                ))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_tile(DestinationSurface::Favorites, &opened);
                    this.queue_peek_action(super::peek::PeekAction::Dismiss, window, cx);
                    cx.notify();
                })),
            );
        }
        column
    }
}

/// Highlights Favorites while a tab, saved page or plugin is dragged over them and adds it on drop.
fn favorites_drop_target(favorites: Div, cx: &mut Context<BrowserView>) -> Div {
    favorites
        .rounded(px(8.))
        .drag_over::<DraggedFavorite>(|style, _, _, _| style.bg(theme::preview_row_selected()))
        .drag_over::<DraggedRow>(|style, _, _, _| style.bg(theme::preview_row_selected()))
        .on_drop(cx.listener(|this, dragged: &DraggedFavorite, _, cx| {
            this.drop_favorite(&dragged.title, &dragged.source);
            cx.notify();
        }))
        .on_drop(cx.listener(|this, dragged: &DraggedRow, _, cx| {
            this.drop_saved_favorite(dragged.entry);
            cx.notify();
        }))
}

/// The tile a dropped tab or plugin becomes. A page's title is its name, cut to fit; an
/// untitled page uses its address. Only websites qualify: not a blank tab or Syrinx's own pages.
fn favorite_tile(title: &str, source: &FavoriteSource) -> Result<Destination, String> {
    match source {
        FavoriteSource::Plugin(plugin) => {
            tile_from_form(plugin.name(), "", TargetChoice::Plugin(*plugin))
        }
        FavoriteSource::Address(address) => tile_from_form(
            &favorite_label(title, address),
            address,
            TargetChoice::Website,
        )
        .map_err(|_| "Only a website can be a favorite.".to_owned()),
    }
}

/// A short tile name: the title without control characters, or the address when it has none.
fn favorite_label(title: &str, address: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let name = if cleaned.trim().is_empty() {
        address
    } else {
        cleaned.trim()
    };
    name.chars().take(MAX_LABEL_CHARS).collect()
}

/// Whether Favorites already open the same place, whatever its name.
fn is_favorite(favorites: &[Destination], tile: &Destination) -> bool {
    favorites
        .iter()
        .any(|favorite| favorite.target == tile.target)
}

#[cfg(test)]
mod tests {
    use syrinx_core::DestinationTarget;

    use super::*;

    #[test]
    fn a_dragged_website_becomes_a_named_favorite() {
        let source = FavoriteSource::Address("https://drag.example.test/page".to_owned());
        let tile = favorite_tile("Synthetic page", &source).expect("a website qualifies");
        assert_eq!(tile.label, "Synthetic page");
        // Destinations have no Debug output (owner data), so compare without assert_eq.
        assert!(tile.target == DestinationTarget::Web("https://drag.example.test/page".to_owned()));
    }

    #[test]
    fn a_blank_tab_or_internal_page_cannot_be_a_favorite() {
        for address in ["about:blank", "", "syrinx://home"] {
            let source = FavoriteSource::Address(address.to_owned());
            assert!(favorite_tile("Blank", &source).is_err(), "{address}");
        }
    }

    #[test]
    fn a_dragged_plugin_is_named_for_itself() {
        let tile = favorite_tile("ignored", &FavoriteSource::Plugin(BuiltInPlugin::Notes))
            .expect("plugins qualify");
        assert_eq!(tile.label, "Notes");
        assert!(tile.target == DestinationTarget::Plugin(BuiltInPlugin::Notes));
    }

    #[test]
    fn long_or_missing_titles_still_make_a_valid_name() {
        let long = "x".repeat(MAX_LABEL_CHARS + 20);
        let address = "https://a.example.test/";
        assert_eq!(
            favorite_label(&long, address).chars().count(),
            MAX_LABEL_CHARS
        );
        assert_eq!(favorite_label(" \t ", address), address);
        assert_eq!(favorite_label("Tab\nname", address), "Tabname");
    }

    #[test]
    fn the_same_place_is_not_added_twice() {
        let target = DestinationTarget::Web("https://twice.example.test/".to_owned());
        let existing = Destination::new("First name", target.clone()).expect("valid");
        let again = Destination::new("Other name", target).expect("valid");
        assert!(is_favorite(std::slice::from_ref(&existing), &again));
    }
}

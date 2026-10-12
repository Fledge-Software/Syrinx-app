//! Draws the sidebar's square Favorites, separate from saved pages and Quick access (D45).

use atlas_core::DestinationSurface;
use atlas_ui::Palette;
use atlas_ui::metrics::text;
use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, div, px,
};

use super::BrowserView;
use super::tiles;
use crate::theme::{self, rgba};

/// The canvas sidebar's four-column grid of 46px-high tiles.
const COLUMNS: u16 = 4;
const TILE_HEIGHT: f32 = 46.;
const TILE_RADIUS: f32 = 12.;
const LETTER_SIZE: f32 = 17.;

impl BrowserView {
    /// The Favorites heading, grid and, while one is open, the Favorites tile form.
    pub(super) fn favorites(&self, cx: &mut Context<Self>) -> Div {
        let palette = theme::current();
        let surface = DestinationSurface::Favorites;
        let mut section = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .text_size(px(text::CAPTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgba(palette.text_muted))
                            .child("FAVORITES"),
                    )
                    .child(self.edit_toggle("favorites-edit", surface, &palette, cx)),
            )
            .child(self.favorites_grid(&palette, cx));
        if let Some(editor) = self
            .home
            .editor
            .as_ref()
            .filter(|editor| editor.surface == surface)
        {
            section = section.child(self.tile_editor(editor, &palette, cx));
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
            let mut cell = div()
                .id(("favorite", index))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .child(
                    tiles::tile_face(palette, tile, TILE_HEIGHT, TILE_RADIUS, LETTER_SIZE).w_full(),
                );
            cell = if editing {
                cell.child(self.tile_controls(palette, surface, index, favorites.len(), cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.start_tile_editor(surface, Some(index), window, cx);
                    }))
            } else {
                cell.on_click(cx.listener(move |this, _, _, cx| {
                    this.open_tile(&opened);
                    cx.notify();
                }))
            };
            grid = grid.child(cell);
        }
        grid.child(self.add_favorite(palette, cx))
    }

    fn add_favorite(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("favorite-add")
            .cursor_pointer()
            .child(tiles::add_face(palette, TILE_HEIGHT, TILE_RADIUS).w_full())
            .on_click(cx.listener(|this, _, window, cx| {
                this.start_tile_editor(DestinationSurface::Favorites, None, window, cx);
            }))
    }
}

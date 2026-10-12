//! Gives the chrome its colours from `atlas-ui`'s tokens instead of hex literals.

use std::sync::LazyLock;

use atlas_ui::{Accent, Color, Palette, PaletteId, palette};
use gpui::Rgba;

/// Graphite with the default accent, until P1-29 persists the owner's Customize choice.
static PREVIEW: LazyLock<Palette> =
    LazyLock::new(|| palette(PaletteId::Graphite, Accent::default()));

/// Converts a token colour to the GPUI colour type.
pub(crate) fn rgba(color: Color) -> Rgba {
    gpui::rgba(color.to_rgba_u32())
}

pub(crate) fn ground() -> Rgba {
    rgba(PREVIEW.ground.representative())
}

pub(crate) fn text() -> Rgba {
    rgba(PREVIEW.text)
}

pub(crate) fn icon() -> Rgba {
    rgba(PREVIEW.icon)
}

pub(crate) fn icon_dim() -> Rgba {
    rgba(PREVIEW.icon_dim)
}

pub(crate) fn line() -> Rgba {
    rgba(PREVIEW.line)
}

pub(crate) fn panel() -> Rgba {
    rgba(PREVIEW.panel)
}

pub(crate) fn accent() -> Rgba {
    rgba(PREVIEW.accent.ink)
}

// The preview's selected-row and secondary-button greys are not canvas tokens. They are kept
// unchanged here, recorded in docs/p1-07-design-tokens.md, until the sidebar slices adopt
// `row_selected`.

/// The preview's opaque selected-row grey (the canvas uses translucent `row_selected`).
pub(crate) fn preview_row_selected() -> Rgba {
    gpui::rgb(0x343037)
}

/// The preview palette's "Search the web" button grey.
pub(crate) fn preview_secondary_button() -> Rgba {
    gpui::rgb(0x29272a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphite_tokens_reproduce_the_preview_hex_values() {
        assert_eq!(ground(), gpui::rgb(0x141519));
        assert_eq!(text(), gpui::rgb(0xecedf0));
        assert_eq!(icon(), gpui::rgb(0xb4b7bf));
        assert_eq!(icon_dim(), gpui::rgb(0xa3a6ae));
        assert_eq!(line(), gpui::rgb(0x24262c));
        assert_eq!(panel(), gpui::rgb(0x1b1c21));
        assert_eq!(accent(), gpui::rgb(0xff8a65));
    }
}

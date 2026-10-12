//! Gives the chrome its colours from `atlas-ui`'s tokens instead of hex literals.

use std::cell::RefCell;

use atlas_ui::{Accent, Color, Ground, Palette, PaletteId, palette};
use gpui::{Background, Rgba, WindowBackgroundAppearance, linear_color_stop, linear_gradient};

thread_local! {
    // GPUI draws every Atlas window on one thread; Customize replaces this palette live.
    static CURRENT: RefCell<Palette> = RefCell::new(palette(PaletteId::Graphite, Accent::default()));
}

/// Makes `palette` the one every later render draws with.
pub(crate) fn apply(palette: Palette) {
    CURRENT.set(palette);
}

/// A copy of the palette being drawn.
pub(crate) fn current() -> Palette {
    CURRENT.with_borrow(Palette::clone)
}

fn token(read: impl FnOnce(&Palette) -> Color) -> Rgba {
    CURRENT.with_borrow(|palette| rgba(read(palette)))
}

/// Converts a token colour to the GPUI colour type.
pub(crate) fn rgba(color: Color) -> Rgba {
    gpui::rgba(color.to_rgba_u32())
}

/// The colour a chrome region paints over the window fill. Gradient and material grounds are
/// painted once by the window, so regions stay transparent and let them show through.
pub(crate) fn ground() -> Rgba {
    token(|palette| match palette.ground {
        Ground::Solid(color) => color,
        Ground::Gradient { .. } | Ground::Material { .. } => Color::rgba(0, 0, 0, 0.),
    })
}

/// What the main window paints behind all chrome: Tint's gradient, or a see-through palette's
/// translucent tint over the OS material.
pub(crate) fn window_fill() -> Background {
    CURRENT.with_borrow(|palette| match palette.ground {
        Ground::Solid(color) | Ground::Material { tint: color, .. } => rgba(color).into(),
        Ground::Gradient {
            angle_degrees,
            stops,
        } => {
            // GPUI gradients take two stops, so Tint keeps its first and last.
            let [(from, from_at), _, (to, to_at)] = stops;
            linear_gradient(
                angle_degrees,
                linear_color_stop(rgba(from), from_at),
                linear_color_stop(rgba(to), to_at),
            )
        }
    })
}

/// The OS backdrop the drawn palette asks for. Glass's Acrylic needs `atlas-platform`; until
/// then it shares Haze's Mica (recorded in `docs/p1-28-29-home-settings.md`).
pub(crate) fn window_material() -> WindowBackgroundAppearance {
    CURRENT.with_borrow(|palette| match palette.ground {
        Ground::Material { .. } => WindowBackgroundAppearance::MicaBackdrop,
        Ground::Solid(_) | Ground::Gradient { .. } => WindowBackgroundAppearance::Opaque,
    })
}

pub(crate) fn text() -> Rgba {
    token(|palette| palette.text)
}

pub(crate) fn icon() -> Rgba {
    token(|palette| palette.icon)
}

pub(crate) fn icon_dim() -> Rgba {
    token(|palette| palette.icon_dim)
}

pub(crate) fn line() -> Rgba {
    token(|palette| palette.line)
}

pub(crate) fn panel() -> Rgba {
    token(|palette| palette.panel)
}

pub(crate) fn accent() -> Rgba {
    token(|palette| palette.accent.ink)
}

// The preview's selected-row and secondary-button greys were opaque stand-ins (P1-07). Live
// palettes need them to follow Paper too, so they now read the canvas tokens.

/// The selected row (the canvas's translucent `rowOn`).
pub(crate) fn preview_row_selected() -> Rgba {
    token(|palette| palette.row_selected)
}

/// The palette's "Search the web" button (the canvas's `field`).
pub(crate) fn preview_secondary_button() -> Rgba {
    token(|palette| palette.field)
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

    #[test]
    fn applying_a_palette_changes_every_later_token() {
        let paper = palette(PaletteId::Paper, Accent::default());
        apply(paper.clone());
        assert_eq!(text(), rgba(paper.text));
        assert_eq!(preview_row_selected(), rgba(paper.row_selected));
        assert_eq!(current(), paper);
        // Restore the default for any later test sharing this thread.
        apply(palette(PaletteId::Graphite, Accent::default()));
        assert_eq!(ground(), gpui::rgb(0x141519));
    }
}

//! Gives the chrome its colours from `syrinx-ui`'s tokens instead of hex literals.

use std::cell::RefCell;

use gpui::{App, Background, Rgba, linear_color_stop, linear_gradient};
use gpui_component::{Theme, ThemeMode, ThemeToken};
use syrinx_ui::{Accent, Color, Ground, Palette, PaletteId, palette};

thread_local! {
    // GPUI draws every Syrinx window on one thread; Customize replaces this palette live.
    static CURRENT: RefCell<Palette> = RefCell::new(palette(PaletteId::Graphite, Accent::default()));
}

/// Makes `palette` the one every later render draws with.
pub(crate) fn apply(palette: Palette) {
    CURRENT.set(palette);
}

/// Gives gpui-component's widgets, such as text fields, the drawn palette's light or dark
/// theme; Paper is the only light palette.
pub(crate) fn apply_component_theme(cx: &mut App) {
    let mode = CURRENT.with_borrow(|palette| {
        if palette.id == PaletteId::Paper {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        }
    });
    Theme::change(mode, None, cx);
    // Root must not paint over the native page's rectangle (the Phase 0 stacking trap).
    Theme::update(cx, |theme| {
        theme.tokens.background =
            ThemeToken::new(gpui::transparent_black(), gpui::transparent_black().into());
    });
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

/// Accent text and thin marks: the ink, which Haze/Glass may lighten and Paper darkens.
pub(crate) fn accent() -> Rgba {
    token(|palette| palette.accent.ink)
}

/// The chosen accent itself, for filled controls, so they match badges in every palette.
pub(crate) fn accent_fill() -> Rgba {
    token(|palette| palette.accent.fill)
}

/// Text drawn on `accent_fill` (the audited badge text).
pub(crate) fn on_accent_fill() -> Rgba {
    token(|palette| palette.accent.on_fill)
}

/// The keyboard focus ring and its inner line (`syrinx_ui::focus`).
pub(crate) fn focus_ring() -> (Rgba, Rgba) {
    CURRENT.with_borrow(|palette| {
        (
            rgba(syrinx_ui::focus::ring_color(&palette.accent)),
            rgba(syrinx_ui::focus::ring_inner_color(&palette.accent)),
        )
    })
}

// The preview's selected-row grey was an opaque stand-in (P1-07). Live palettes need it to
// follow Paper too, so it now reads the canvas token. The palette's former "Search the web"
// grey went with P1-30, whose palette reads every colour from `current()`.

/// The selected row (the canvas's translucent `rowOn`).
pub(crate) fn preview_row_selected() -> Rgba {
    token(|palette| palette.row_selected)
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
        assert_eq!(accent(), gpui::rgb(0xff612e));
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

//! Gives the chrome its colours from `syrinx-ui`'s tokens instead of hex literals.

use std::cell::RefCell;

use gpui::{App, Background, BoxShadow, Rgba, linear_color_stop, linear_gradient, point, px};
use gpui_component::{Theme, ThemeMode, ThemeToken};
use syrinx_ui::{Accent, Color, Ground, Palette, PaletteId, ShadowLayer, ShadowRole, palette};

thread_local! {
    // GPUI draws every Syrinx window on one thread; Customize replaces this palette live.
    static CURRENT: RefCell<Palette> = RefCell::new(palette(PaletteId::Graphite, Accent::default()));
}

/// Makes `palette` the one every later render draws with.
pub(crate) fn apply(palette: Palette) {
    CURRENT.set(palette);
}

/// Whether the drawn palette is light: Paper, or a light theme built on it.
pub(crate) fn is_light() -> bool {
    CURRENT.with_borrow(Palette::is_light)
}

/// Gives gpui-component's widgets, such as text fields, the drawn palette's light or dark
/// theme.
pub(crate) fn apply_component_theme(cx: &mut App) {
    let mode = if is_light() {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
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

/// What the main window paints behind all chrome: a solid colour, a gradient, or a see-through
/// theme's translucent tint over the OS material. A three-stop gradient or grain is drawn as
/// an image over this (`crate::ground_image`).
pub(crate) fn window_fill() -> Background {
    CURRENT.with_borrow(|palette| match palette.ground {
        Ground::Solid(color) | Ground::Material { tint: color, .. } => rgba(color).into(),
        Ground::Gradient {
            angle_degrees,
            stops,
        } => {
            // GPUI gradients take two stops; this keeps the first and last under the image.
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

/// An icon whose control cannot act now but still responds, at 3:1 on the ground (#126).
pub(crate) fn unavailable_icon() -> Rgba {
    token(syrinx_ui::contrast::unavailable_icon)
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

/// The drawn palette's shadow for `role`, as GPUI layers, bottom first; empty when it has none.
pub(crate) fn shadow(role: ShadowRole) -> Vec<BoxShadow> {
    let (drop, edge) = shadow_layers(role, 0.);
    drop.into_iter().chain(edge).collect()
}

/// The drawn palette's shadow for `role` on an element outlined `border` pixels wide: its
/// drop layer, if any, and its glass-edge layers. CSS draws an inset shadow inside the
/// border, GPUI from the outer edge, so an edge layer is moved in past the border; otherwise
/// the highlight would sit on the outline instead of just inside it.
pub(crate) fn shadow_layers(role: ShadowRole, border: f32) -> (Option<BoxShadow>, Vec<BoxShadow>) {
    CURRENT.with_borrow(|palette| {
        let shadow = palette.shadows.role(role);
        let drop = shadow.drop_layer().map(box_shadow);
        let edge = shadow
            .inset_layers()
            .map(|layer| box_shadow(inside_border(layer, border)))
            .collect();
        (drop, edge)
    })
}

/// An inset layer moved in by `border`: a highlight's offset grows, a ring's spread grows.
fn inside_border(layer: ShadowLayer, border: f32) -> ShadowLayer {
    if layer.offset_y == 0. {
        return ShadowLayer {
            spread: layer.spread + border,
            ..layer
        };
    }
    ShadowLayer {
        offset_y: layer.offset_y + border * layer.offset_y.signum(),
        ..layer
    }
}

fn box_shadow(layer: ShadowLayer) -> BoxShadow {
    BoxShadow {
        color: rgba(layer.color).into(),
        offset: point(px(0.), px(layer.offset_y)),
        blur_radius: px(layer.gaussian_sigma()),
        spread_radius: px(layer.spread),
        inset: layer.inset,
    }
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
        // The pill's canvas `0 8px 24px` becomes GPUI's 12-pixel deviation.
        let pill = shadow(ShadowRole::Pill);
        assert_eq!(pill.len(), 1);
        assert_eq!(pill[0].offset, point(px(0.), px(8.)));
        assert_eq!(pill[0].blur_radius, px(12.));
        // Restore the default for any later test sharing this thread.
        apply(palette(PaletteId::Graphite, Accent::default()));
        assert!(!is_light());
        assert_eq!(ground(), gpui::rgb(0x141519));
        assert!(
            ShadowRole::ALL
                .into_iter()
                .all(|role| shadow(role).is_empty())
        );
    }

    #[test]
    fn glass_edges_move_inside_a_surfaces_border() {
        apply(palette(PaletteId::Glass, Accent::default()));
        let (drop, edge) = shadow_layers(ShadowRole::Pill, 1.);
        // `0 14px 36px rgba(0,0,0,0.25)` becomes GPUI's 18-pixel deviation.
        let drop = drop.expect("the pill has a drop shadow");
        assert!(!drop.inset);
        assert_eq!(drop.blur_radius, px(18.));
        // `inset 0 1px 0` lands just inside the 1px outline, `inset 0 0 0 1px` likewise.
        assert_eq!(edge.len(), 2);
        assert!(edge.iter().all(|layer| layer.inset));
        assert_eq!(edge[0].offset, point(px(0.), px(2.)));
        assert_eq!(edge[0].spread_radius, px(0.));
        assert_eq!(edge[1].offset, point(px(0.), px(0.)));
        assert_eq!(edge[1].spread_radius, px(2.));
        // Without a border the canvas values pass straight through.
        let (_, bare) = shadow_layers(ShadowRole::Field, 0.);
        assert_eq!(bare[0].offset, point(px(0.), px(1.)));
        assert_eq!(bare[1].spread_radius, px(1.));
        apply(palette(PaletteId::Graphite, Accent::default()));
    }
}

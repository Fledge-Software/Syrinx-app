//! Makes Syrinx's own clickable elements keyboard controls that screen readers can name (P1-36).

use gpui::{
    BoxShadow, Div, InteractiveElement, ParentElement, Role, SharedString, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, point, px,
};
use syrinx_ui::focus::{RING_INNER_WIDTH, RING_WIDTH};
use syrinx_ui::metrics::{chrome::ICON_BUTTON, radius, text};

use crate::theme;

/// Glyphs sit at the size of the design's 16-pixel stroke icons.
const GLYPH_SIZE: f32 = 13.;

/// Gives a clickable element a role and accessible name, a place in Tab order (Enter and
/// Space then click it) and the focus ring.
pub(crate) fn control(
    element: Stateful<Div>,
    role: Role,
    name: impl Into<SharedString>,
) -> Stateful<Div> {
    element
        .role(role)
        .aria_label(name)
        .tab_index(0)
        .focus(focus_ring)
}

/// A plain push button.
pub(crate) fn push_button(element: Stateful<Div>, name: impl Into<SharedString>) -> Stateful<Div> {
    control(element, Role::Button, name)
}

/// Icon glyphs from Segoe MDL2 Assets, which ships with Windows, so no icon files are bundled.
pub(crate) mod glyph {
    pub(crate) const FONT: &str = "Segoe MDL2 Assets";
    pub(crate) const BACK: &str = "\u{E76B}";
    pub(crate) const FORWARD: &str = "\u{E76C}";
    pub(crate) const SIDEBAR: &str = "\u{E89F}";
    pub(crate) const MENU: &str = "\u{E700}";
    pub(crate) const MORE: &str = "\u{E712}";
    pub(crate) const BOOKMARKS: &str = "\u{E734}";
    pub(crate) const PASSWORDS: &str = "\u{E8D7}";
    pub(crate) const APPEARANCE: &str = "\u{E713}";
    pub(crate) const RELOAD: &str = "\u{E72C}";
    pub(crate) const SEARCH: &str = "\u{E721}";
    pub(crate) const DOWNLOADS: &str = "\u{E896}";
    pub(crate) const HISTORY: &str = "\u{E81C}";
    pub(crate) const ADD: &str = "\u{E710}";
}

/// The design's square icon button: transparent until hovered, with its glyph centred.
pub(crate) fn icon_button(
    element: Stateful<Div>,
    glyph: &'static str,
    name: impl Into<SharedString>,
) -> Stateful<Div> {
    push_button(element, name)
        .size(px(ICON_BUTTON))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius::CONTROL))
        .font_family(glyph::FONT)
        .text_size(px(GLYPH_SIZE))
        .text_color(theme::icon())
        .cursor_pointer()
        .hover(|style| style.bg(theme::line()))
        .child(glyph)
}

/// The design's text button: a quiet field-coloured shape that brightens when hovered.
pub(crate) fn text_button(element: Stateful<Div>, label: &'static str) -> Stateful<Div> {
    let palette = theme::current();
    push_button(element, label)
        .h(px(ICON_BUTTON))
        .px(px(12.))
        .flex_none()
        .flex()
        .items_center()
        .rounded(px(radius::CONTROL))
        .bg(theme::rgba(palette.field))
        .text_size(px(text::BODY))
        .text_color(theme::rgba(palette.text_secondary))
        .cursor_pointer()
        .hover(move |style| style.bg(theme::rgba(palette.row_selected)))
        .child(label)
}

/// The focus ring: an inset accent outline with a dark inner line, so it never shifts
/// layout, a scrolling parent cannot clip it, and it reads on accent-filled controls. It
/// shows on any focus, because F6 from a page leaves GPUI believing the mouse was last.
pub(crate) fn focus_ring(style: StyleRefinement) -> StyleRefinement {
    let (ring, inner) = theme::focus_ring();
    // Later shadows paint on top: the inner line first, the wider ring band over it.
    style.shadow(vec![
        inset_band(inner, RING_WIDTH + RING_INNER_WIDTH),
        inset_band(ring, RING_WIDTH),
    ])
}

fn inset_band(color: gpui::Rgba, width: f32) -> BoxShadow {
    BoxShadow {
        color: color.into(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(width),
        inset: true,
    }
}

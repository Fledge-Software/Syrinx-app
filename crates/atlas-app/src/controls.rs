//! Makes Atlas's own clickable elements keyboard controls that screen readers can name (P1-36).

use atlas_ui::focus::{RING_INNER_WIDTH, RING_WIDTH};
use gpui::{
    BoxShadow, Div, InteractiveElement, Role, SharedString, Stateful, StatefulInteractiveElement,
    StyleRefinement, Styled, point, px,
};

use crate::theme;

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

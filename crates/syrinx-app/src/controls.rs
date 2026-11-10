//! Makes Syrinx's own clickable elements keyboard controls that screen readers can name (P1-36).

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    App, Bounds, BoxShadow, ContentMask, Corners, DispatchPhase, Div, FocusHandle,
    InteractiveElement, IntoElement, MouseDownEvent, ParentElement, Pixels, Role, SharedString,
    Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, canvas, point, px, size,
};
use syrinx_ui::focus::{Modality, ModalityInput, RING_INNER_WIDTH, RING_WIDTH};
use syrinx_ui::metrics::{chrome::ICON_BUTTON, radius, text};
use syrinx_ui::{Band, Color, ShadowRole, bands_outside};

use crate::icon::{Icon, with_icon};
use crate::theme;

/// Glyphs sit at the size of the design's 16-pixel stroke icons.
const GLYPH_SIZE: f32 = 13.;

/// Whether keys, rather than a mouse press, moved the owner around last (DW-62). One flag
/// for every Syrinx window: there is one owner with one keyboard and one mouse.
static KEYBOARD_MODALITY: AtomicBool = AtomicBool::new(true);

fn modality() -> Modality {
    if KEYBOARD_MODALITY.load(Ordering::Relaxed) {
        Modality::Keyboard
    } else {
        Modality::Pointer
    }
}

/// Whether `handle`'s element is drawing its focus ring right now: it is focused and the
/// owner last used the keyboard (DW-62).
pub(crate) fn ring_shown(handle: &FocusHandle, window: &Window) -> bool {
    modality().shows_focus_ring() && handle.is_focused(window)
}

/// Records what the owner did and, when that shows or hides the ring, redraws `window` now
/// and every other open Syrinx window once the current update ends.
pub(crate) fn note_input(input: ModalityInput<'_>, window: &mut Window, cx: &mut App) {
    let before = modality();
    let after = before.after(input);
    if after == before {
        return;
    }
    KEYBOARD_MODALITY.store(after == Modality::Keyboard, Ordering::Relaxed);
    window.refresh();
    // Other windows are redrawn from a deferred callback, outside this window's update.
    cx.defer(refresh_every_window);
}

/// Redraws every open window, so a ring elsewhere does not wait for an unrelated redraw.
fn refresh_every_window(cx: &mut App) {
    for handle in cx.windows() {
        if let Err(error) = handle.update(cx, |_, window, _| window.refresh()) {
            eprintln!("Cannot redraw a window for the focus ring: {error}");
        }
    }
}

/// Watches every key a Syrinx window receives, before any handler can consume it. Keys a
/// focused page hands over arrive as browser shortcuts instead (`ui.rs`).
pub(crate) fn watch_keys(cx: &mut App) {
    cx.intercept_keystrokes(|event, window, cx| {
        note_input(ModalityInput::Key(&event.keystroke.key), window, cx);
    })
    .detach();
}

/// Makes a mouse press anywhere in a window hide the ring. A raw window listener, because a
/// root's own mouse listener only fires while the root counts as hovered, which an occluding
/// child (the toolbar) or a preceding key press prevents.
pub(crate) fn watch_pointer<E: ParentElement>(root: E) -> E {
    watch_pointer_and(root, |_| {})
}

/// `watch_pointer`, also calling `on_press` for every press in the window.
pub(crate) fn watch_pointer_and<E: ParentElement>(
    root: E,
    on_press: impl Fn(&mut App) + 'static,
) -> E {
    root.child(
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        note_input(ModalityInput::PointerPress, window, cx);
                        on_press(cx);
                    }
                });
            },
        )
        .absolute()
        .size_0(),
    )
}

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

/// The design's square icon button: transparent until hovered, with its icon centred.
pub(crate) fn icon_button(
    element: Stateful<Div>,
    icon: Icon,
    name: impl Into<SharedString>,
) -> Stateful<Div> {
    let button = push_button(element, name)
        .size(px(ICON_BUTTON))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius::CONTROL))
        .text_color(theme::icon())
        .cursor_pointer()
        .hover(|style| style.bg(theme::line()));
    with_icon(button, icon, GLYPH_SIZE)
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
/// shows only after keys (DW-62); Syrinx tracks that itself, because GPUI's own
/// `focus_visible` never sees the F6 or Tab that leaves a page.
pub(crate) fn focus_ring(style: StyleRefinement) -> StyleRefinement {
    focus_ring_over(style, Vec::new())
}

/// The focus ring drawn over a surface's own shadow, which the ring would otherwise replace.
fn focus_ring_over(style: StyleRefinement, mut shadows: Vec<BoxShadow>) -> StyleRefinement {
    if !modality().shows_focus_ring() {
        return style;
    }
    let (ring, inner) = theme::focus_ring();
    // Later shadows paint on top: the inner line first, the wider ring band over it.
    shadows.push(inset_band(inner, RING_WIDTH + RING_INNER_WIDTH));
    shadows.push(inset_band(ring, RING_WIDTH));
    style.shadow(shadows)
}

/// Lifts a surface with the palette's shadow for `role` (#94), drawn under and inside it. It
/// replaces the focus style with the ring over that shadow, so use it on plain controls or
/// elements without focus. For an opaque or near-opaque fill without an outline; a
/// translucent glass surface uses [`lifted`]. Only for chrome and Syrinx's own pages: a
/// shadow over the page card hides the page.
pub(crate) fn raised<E: Styled + InteractiveElement>(element: E, role: ShadowRole) -> E {
    with_shadows(element, theme::shadow(role))
}

/// The focus style for an element carrying `role`'s shadow: the ring drawn over it.
pub(crate) fn focus_ring_keeping(role: ShadowRole) -> impl Fn(StyleRefinement) -> StyleRefinement {
    let shadows = theme::shadow(role);
    move |style| focus_ring_over(style, shadows.clone())
}

/// The outline every lifted surface draws, in pixels.
const SURFACE_BORDER: f32 = 1.;

/// How far inside a device pixel's edge a shadow band stops, in device pixels.
const EDGE_SLACK: f32 = 0.01;

/// A fill at least this opaque hides a drop shadow painted under it.
const NEAR_OPAQUE: u8 = 0xF0;

/// The look of one lifted surface: its fill, outline and corner radius.
pub(crate) struct Surface {
    pub(crate) fill: Color,
    pub(crate) line: Color,
    pub(crate) corner: f32,
}

/// Fills, outlines and rounds a surface, then lifts it with `role`'s shadow as the canvas
/// draws it (#93). GPUI paints a drop shadow under the whole element, which would darken a
/// translucent glass fill from inside, so such a fill gets its drop only outside its shape.
pub(crate) fn lifted(element: Stateful<Div>, role: ShadowRole, surface: Surface) -> Stateful<Div> {
    let Surface { fill, line, corner } = surface;
    let element = element
        .rounded(px(corner))
        .bg(theme::rgba(fill))
        .border_1()
        .border_color(theme::rgba(line));
    let (drop, edge) = theme::shadow_layers(role, SURFACE_BORDER);
    let Some(drop) = drop else {
        return with_shadows(element, edge);
    };
    if fill.alpha >= NEAR_OPAQUE {
        return with_shadows(element, std::iter::once(drop).chain(edge).collect());
    }
    with_shadows(element, edge)
        .relative()
        .child(drop_outside(drop, corner))
}

/// Applies `shadows` and keeps them under the focus ring; nothing when there are none.
fn with_shadows<E: Styled + InteractiveElement>(element: E, shadows: Vec<BoxShadow>) -> E {
    if shadows.is_empty() {
        return element;
    }
    element
        .shadow(shadows.clone())
        .focus(move |style| focus_ring_over(style, shadows))
}

/// Paints `drop` around its parent's outline but never inside it, by clipping the shadow to
/// bands that tile the outside of the rounded shape (`syrinx_ui::bands_outside`).
fn drop_outside(drop: BoxShadow, corner: f32) -> impl IntoElement {
    let reach = f32::from(drop.offset.y).abs()
        + 3. * f32::from(drop.blur_radius)
        + f32::from(drop.spread_radius);
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            // Bands are cut on whole device pixels: GPUI widens a mask to cover any pixel it
            // touches, so fractional edges would paint shared rows twice, as dark stripes.
            let scale = window.scale_factor();
            let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let corner = corner.min(width / 2.).min(height / 2.);
            let radii = Corners::all(px(corner));
            // The shape is widened to whole device pixels, so no shadow lands on the outline.
            let (origin_x, device_width) = device_span(f32::from(bounds.origin.x), width, scale);
            let (origin_y, device_height) = device_span(f32::from(bounds.origin.y), height, scale);
            let device = |logical: f32| (logical * scale).round();
            let device_corner = device(corner).min((device_width.min(device_height) / 2.).floor());
            let device_bands =
                bands_outside(device_width, device_height, device_corner, device(reach));
            for band in device_bands {
                let mask = device_mask((origin_x, origin_y), band, scale);
                window.with_content_mask(Some(mask), |window| {
                    window.paint_drop_shadows(bounds, radii, std::slice::from_ref(&drop));
                });
            }
        },
    )
    .absolute()
    .top(px(-SURFACE_BORDER))
    .left(px(-SURFACE_BORDER))
    .right(px(-SURFACE_BORDER))
    .bottom(px(-SURFACE_BORDER))
}

/// A logical span as whole device pixels: its first pixel and how many it touches.
fn device_span(start: f32, length: f32, scale: f32) -> (f32, f32) {
    let first = (start * scale).floor();
    let end = ((start + length) * scale).ceil();
    (first, end - first)
}

/// A content mask over `band`, given in device pixels from `origin`, stopping a hair inside
/// each edge so rounding error never reaches the neighbouring pixel.
fn device_mask(origin: (f32, f32), band: Band, scale: f32) -> ContentMask<Pixels> {
    let logical = |device: f32| px(device / scale);
    ContentMask {
        bounds: Bounds::new(
            point(
                logical(origin.0 + band.x + EDGE_SLACK),
                logical(origin.1 + band.y + EDGE_SLACK),
            ),
            size(
                logical(band.width - 2. * EDGE_SLACK),
                logical(band.height - 2. * EDGE_SLACK),
            ),
        ),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use syrinx_ui::theme::{Material, ThemeSpec};
    use syrinx_ui::{Accent, PaletteId, palette};

    #[test]
    fn every_panel_is_opaque_enough_to_keep_its_drop_under_its_scrolling_clip() {
        // `lifted` draws a translucent fill's drop as a child, which a scrolling panel clips.
        let glass_theme = ThemeSpec {
            material: Material::Acrylic,
            ..ThemeSpec::new("Glass")
        };
        let mut panels: Vec<_> = PaletteId::ALL
            .into_iter()
            .map(|id| palette(id, Accent::default()).panel)
            .collect();
        panels.push(glass_theme.palette(Accent::default(), false).panel);
        for panel in panels {
            assert!(panel.alpha >= NEAR_OPAQUE, "{panel:?}");
        }
    }
}

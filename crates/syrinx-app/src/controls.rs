//! Makes Syrinx's own clickable elements keyboard controls that screen readers can name (P1-36).

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    App, BoxShadow, DispatchPhase, Div, InteractiveElement, MouseDownEvent, ParentElement, Role,
    SharedString, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, canvas,
    point, px,
};
use syrinx_ui::focus::{Modality, ModalityInput, RING_INNER_WIDTH, RING_WIDTH};

use crate::theme;

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
    root.child(
        canvas(
            |_, _, _| {},
            |_, _, window, _| {
                window.on_mouse_event(|_: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        note_input(ModalityInput::PointerPress, window, cx);
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

/// The focus ring: an inset accent outline with a dark inner line, so it never shifts
/// layout, a scrolling parent cannot clip it, and it reads on accent-filled controls. It
/// shows only after keys (DW-62); Syrinx tracks that itself, because GPUI's own
/// `focus_visible` never sees the F6 or Tab that leaves a page.
pub(crate) fn focus_ring(style: StyleRefinement) -> StyleRefinement {
    if !modality().shows_focus_ring() {
        return style;
    }
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

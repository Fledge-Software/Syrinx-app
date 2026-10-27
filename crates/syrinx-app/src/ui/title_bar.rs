//! Draws Windows caption controls without requiring a separate icon asset bundle.

use gpui::{
    Div, InteractiveElement, ParentElement, Role, StatefulInteractiveElement, Styled,
    WindowControlArea, div, px,
};

use syrinx_platform::caption::{self, CaptionButton};

use super::TITLE_BAR_HEIGHT;
use crate::theme;

/// Each caption button is a square as tall as the title bar, flush with its right edge.
const CAPTION_BUTTON_SIZE: f32 = TITLE_BAR_HEIGHT;

/// The gap between the title bar's own controls and the caption buttons; `title_contents` pads
/// by it and the menus' corner subtracts it, so the drawn gap and the anchor cannot drift apart.
pub(super) const CAPTION_GAP: f32 = 4.;

/// The caption buttons' total width, from their size and count.
pub(super) fn caption_buttons_width() -> f32 {
    CAPTION_BUTTON_SIZE * CaptionButton::LEFT_TO_RIGHT.len() as f32
}

/// Native hit areas retain Windows dragging, snap, minimize and durable close callbacks.
pub(super) fn title_bar(content: Div, maximized: bool) -> Div {
    div()
        .h(px(TITLE_BAR_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .bg(theme::ground())
        .text_color(theme::text())
        .child(
            div()
                .h_full()
                .flex_1()
                .min_w_0()
                .pl(px(12.))
                .flex()
                .items_center()
                .window_control_area(WindowControlArea::Drag)
                .child(content),
        )
        .children(CaptionButton::LEFT_TO_RIGHT.map(|button| {
            let (id, label, glyph, area) = caption_control(button, maximized);
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .w(px(CAPTION_BUTTON_SIZE))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .font_family("Segoe MDL2 Assets")
                .text_size(px(10.))
                .window_control_area(area)
                .hover(|style| style.bg(theme::line()))
                .child(glyph)
        }))
}

/// Let Windows find the caption button under the pointer itself; GPUI's answer lags one move.
pub(super) fn correct_caption_hit_test(hwnd: isize) {
    if let Err(error) = caption::install(hwnd, CAPTION_BUTTON_SIZE) {
        eprintln!("{error}");
    }
}

/// The element id, accessible label, glyph and native hit area of one caption button.
fn caption_control(
    button: CaptionButton,
    maximized: bool,
) -> (&'static str, &'static str, &'static str, WindowControlArea) {
    match button {
        CaptionButton::Minimize => (
            "caption-minimize",
            "Minimize",
            "\u{E921}",
            WindowControlArea::Min,
        ),
        CaptionButton::Maximize => {
            let (label, glyph) = maximize_control(maximized);
            ("caption-maximize", label, glyph, WindowControlArea::Max)
        }
        CaptionButton::Close => (
            "caption-close",
            "Close Syrinx",
            "\u{E8BB}",
            WindowControlArea::Close,
        ),
    }
}

/// Segoe MDL2 Assets supplies the Windows caption outlines without bundled assets.
fn maximize_control(maximized: bool) -> (&'static str, &'static str) {
    if maximized {
        ("Restore", "\u{E923}")
    } else {
        ("Maximize", "\u{E922}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximize_caption_tracks_window_state() {
        assert_eq!(maximize_control(false), ("Maximize", "\u{E922}"));
        assert_eq!(maximize_control(true), ("Restore", "\u{E923}"));
    }

    #[test]
    fn caption_buttons_keep_their_native_hit_areas() {
        let areas = CaptionButton::LEFT_TO_RIGHT.map(|button| caption_control(button, false).3);
        assert_eq!(
            areas,
            [
                WindowControlArea::Min,
                WindowControlArea::Max,
                WindowControlArea::Close
            ]
        );
    }
}

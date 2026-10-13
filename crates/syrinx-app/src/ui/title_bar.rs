//! Draws Windows caption controls without requiring a separate icon asset bundle.

use gpui::{
    Div, InteractiveElement, ParentElement, Role, StatefulInteractiveElement, Styled,
    WindowControlArea, div, px,
};

use super::TITLE_BAR_HEIGHT;
use crate::theme;

/// Native hit areas retain Windows dragging, snap, minimize and durable close callbacks.
pub(super) fn title_bar(content: Div, maximized: bool) -> Div {
    let (maximize_label, maximize_glyph) = maximize_control(maximized);
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
        .children(
            [
                (
                    "caption-minimize",
                    "Minimize",
                    "\u{E921}",
                    WindowControlArea::Min,
                ),
                (
                    "caption-maximize",
                    maximize_label,
                    maximize_glyph,
                    WindowControlArea::Max,
                ),
                (
                    "caption-close",
                    "Close Syrinx",
                    "\u{E8BB}",
                    WindowControlArea::Close,
                ),
            ]
            .map(|(id, label, glyph, area)| {
                div()
                    .id(id)
                    .role(Role::Button)
                    .aria_label(label)
                    .w(px(TITLE_BAR_HEIGHT))
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
            }),
        )
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
}

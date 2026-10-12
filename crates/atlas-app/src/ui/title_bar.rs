//! Draws Windows caption controls without requiring a separate icon asset bundle.

use gpui::{
    Div, InteractiveElement, ParentElement, Role, StatefulInteractiveElement, Styled,
    WindowControlArea, div, px,
};

use super::TITLE_BAR_HEIGHT;
use crate::theme;

/// Native hit areas retain Windows dragging, snap, minimize and durable close callbacks.
pub(super) fn title_bar(content: Div) -> Div {
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
                ("caption-minimize", "Minimize", "−", WindowControlArea::Min),
                (
                    "caption-maximize",
                    "Maximize or restore",
                    "□",
                    WindowControlArea::Max,
                ),
                (
                    "caption-close",
                    "Close Atlas",
                    "×",
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
                    .text_lg()
                    .window_control_area(area)
                    .hover(|style| style.bg(theme::line()))
                    .child(glyph)
            }),
        )
}

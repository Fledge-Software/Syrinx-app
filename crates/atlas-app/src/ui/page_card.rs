//! Places the page web view inside the card beside the sidebar, in physical pixels.

use atlas_platform::browser::PageBounds;
use atlas_ui::metrics::chrome::CARD_INSET;

use super::{FOOTER_HEIGHT, TOOLBAR_HEIGHT};

/// The window's logical size and scale, and the width the sidebar reserves right now.
#[derive(Clone, Copy, Debug)]
pub(super) struct CardFrame {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub sidebar_width: f32,
    /// The runtime notice's height while it shows under the toolbar, otherwise zero.
    pub notice_height: f32,
    pub scale: f32,
}

/// The drag strip's logical start and end beside a sidebar of `sidebar_width`. The sidebar
/// view draws the strip here and the page card starts at its end, so they cannot overlap.
pub(super) fn edge_strip(sidebar_width: f32) -> (f32, f32) {
    let start = finite_or_zero(sidebar_width).max(0.);
    (start, start + CARD_INSET)
}

/// The page card's physical bounds. The child web view does not follow the window on its own
/// (`docs/trapstoknow.md`), so this runs on every drag step, resize and DPI change.
pub(super) fn page_card_bounds(frame: CardFrame) -> PageBounds {
    let scale = usable_scale(frame.scale);

    // Inset the card from the sidebar's strip, the toolbar and notice, the footer and the right.
    let (_, left) = edge_strip(frame.sidebar_width);
    let top = TOOLBAR_HEIGHT + finite_or_zero(frame.notice_height).max(0.) + CARD_INSET;
    let right = finite_or_zero(frame.viewport_width) - CARD_INSET;
    let bottom = finite_or_zero(frame.viewport_height) - FOOTER_HEIGHT - CARD_INSET;

    // Round each edge rather than each size, so neighbouring edges never drift apart.
    let (x, width) = physical_span(left, right, scale);
    let (y, height) = physical_span(top, bottom, scale);
    PageBounds {
        x,
        y,
        width,
        height,
    }
}

/// A span's physical start and length, never shorter than one pixel.
fn physical_span(start: f32, end: f32, scale: f32) -> (i32, u32) {
    let first = (start * scale).round();
    let last = (end * scale).round();
    let length = (last - first).max(1.);
    (first as i32, length as u32)
}

/// GPUI reports a positive scale; anything else is treated as 100 %.
fn usable_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0. {
        scale
    } else {
        1.
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0. }
}

#[cfg(test)]
mod tests {
    use atlas_ui::{SidebarWidthState, sidebar};

    use super::*;

    fn frame(sidebar_width: f32, scale: f32) -> CardFrame {
        CardFrame {
            viewport_width: 1280.,
            viewport_height: 800.,
            sidebar_width,
            notice_height: 0.,
            scale,
        }
    }

    #[test]
    fn the_runtime_notice_pushes_the_card_down_by_its_height() {
        let plain = page_card_bounds(frame(264., 1.25));
        let noticed = page_card_bounds(CardFrame {
            notice_height: 36.,
            ..frame(264., 1.25)
        });
        assert_eq!(noticed.y - plain.y, 45);
        assert_eq!(
            noticed.y + noticed.height as i32,
            plain.y + plain.height as i32
        );
        assert_eq!((noticed.x, noticed.width), (plain.x, plain.width));
    }

    #[test]
    fn the_card_starts_one_inset_right_of_each_sidebar_state() {
        for state in SidebarWidthState::ALL {
            let bounds = page_card_bounds(frame(state.reserved_width(), 1.));
            let left = state.reserved_width() + CARD_INSET;
            assert_eq!(bounds.x, left as i32, "{}", state.name());
            assert_eq!(bounds.width, (1280. - CARD_INSET - left) as u32);
            assert_eq!(bounds.y, (TOOLBAR_HEIGHT + CARD_INSET) as i32);
            assert_eq!(
                bounds.height,
                (800. - TOOLBAR_HEIGHT - FOOTER_HEIGHT - 2. * CARD_INSET) as u32
            );
        }
    }

    #[test]
    fn a_narrower_sidebar_gives_the_page_exactly_that_width() {
        let wide = page_card_bounds(frame(SidebarWidthState::Wide.reserved_width(), 1.));
        let hidden = page_card_bounds(frame(SidebarWidthState::Hidden.reserved_width(), 1.));
        let right_edge = |bounds: PageBounds| bounds.x + bounds.width as i32;
        assert_eq!(right_edge(wide), right_edge(hidden));
        assert_eq!(
            hidden.width - wide.width,
            SidebarWidthState::Wide.reserved_width() as u32
        );
    }

    #[test]
    fn bounds_scale_to_physical_pixels() {
        let logical = page_card_bounds(frame(52., 1.));
        let scaled = page_card_bounds(frame(52., 1.5));
        assert_eq!(scaled.x, 90);
        assert_eq!(scaled.y, 84);
        assert_eq!(scaled.width, (logical.width as f32 * 1.5).round() as u32);
        assert_eq!(scaled.height, (logical.height as f32 * 1.5).round() as u32);
    }

    #[test]
    fn the_right_edge_stays_put_while_a_fractional_drag_moves_the_left() {
        let expected = ((1280. - CARD_INSET) * 1.25f32).round() as i32;
        let mut width = 0.;
        while width <= 264. {
            let bounds = page_card_bounds(frame(width, 1.25));
            assert_eq!(bounds.x + bounds.width as i32, expected, "at {width}");
            width += 0.3;
        }
    }

    #[test]
    fn the_strip_and_the_card_never_overlap_at_any_drag_width() {
        for scale in [1., 1.25, 1.5, 1.75, 2.] {
            let mut width = 0.;
            while width <= 264. {
                // Below favicon-only width the sidebar draws no padded content, so its laid-out
                // width is exactly the drag width and the strip starts there.
                let laid_out = if sidebar::draws_content(width) {
                    width.max(sidebar::SIDEBAR_PADDING_LEFT + sidebar::SIDEBAR_PADDING_RIGHT)
                } else {
                    width
                };
                assert_eq!(laid_out, width, "padding widened the sidebar at {width}");
                let (start, end) = edge_strip(laid_out);
                assert_eq!(start, laid_out);
                let bounds = page_card_bounds(frame(width, scale));
                let strip_end = (end * scale).round() as i32;
                assert!(
                    strip_end <= bounds.x,
                    "strip ends at {strip_end}, card starts at {} (width {width}, scale {scale})",
                    bounds.x
                );
                width += 0.25;
            }
        }
    }

    #[test]
    fn a_tiny_or_invalid_window_still_gets_a_one_pixel_card() {
        let bounds = page_card_bounds(CardFrame {
            viewport_width: 10.,
            viewport_height: 10.,
            sidebar_width: 264.,
            notice_height: 0.,
            scale: 2.,
        });
        assert_eq!((bounds.width, bounds.height), (1, 1));
        let invalid = page_card_bounds(CardFrame {
            viewport_width: f32::NAN,
            viewport_height: f32::INFINITY,
            sidebar_width: f32::NAN,
            notice_height: f32::NAN,
            scale: 0.,
        });
        assert_eq!(invalid.x, CARD_INSET as i32);
        assert_eq!((invalid.width, invalid.height), (1, 1));
    }
}

//! Places the page web view inside the card beside the sidebar, in physical pixels, and keeps
//! the chrome's fill off the card while a page shows there.

use gpui::{
    AnyElement, App, Bounds, ContentMask, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, Pixels, Size, Styled, StyledImage, Window, canvas, div, fill, img, point, px,
    size,
};
use syrinx_platform::browser::PageBounds;
use syrinx_ui::metrics::chrome::CARD_INSET;

use super::{FOOTER_HEIGHT, TITLE_BAR_HEIGHT};
use crate::theme;

/// The window's logical size and scale, and the width the sidebar reserves right now.
#[derive(Clone, Copy, Debug)]
pub(super) struct CardFrame {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub sidebar_width: f32,
    /// The runtime notice's height while it shows under the toolbar, otherwise zero.
    pub notice_height: f32,
    /// The site permission prompt's height while it shows under the notice, otherwise zero.
    pub permission_height: f32,
    /// The find bar's height while it shows under the permission prompt, otherwise zero.
    pub find_height: f32,
    /// A page in element fullscreen takes the whole viewport, with no chrome around it.
    pub fullscreen: bool,
    pub scale: f32,
}

impl CardFrame {
    /// The bars stacked between the chrome and the card, in the order they are drawn.
    fn bar_heights(&self) -> f32 {
        [self.notice_height, self.permission_height, self.find_height]
            .into_iter()
            .map(|height| finite_or_zero(height).max(0.))
            .sum()
    }
}

/// The drag strip's logical start and end beside a sidebar of `sidebar_width`. The sidebar
/// view draws the strip here and the page card starts at its end, so they cannot overlap.
pub(super) fn edge_strip(sidebar_width: f32) -> (f32, f32) {
    let start = finite_or_zero(sidebar_width).max(0.);
    (start, start + CARD_INSET)
}

/// The title bar holds navigation in every sidebar state, so it is the only chrome above the card.
pub(super) fn chrome_height() -> f32 {
    TITLE_BAR_HEIGHT
}

/// The title bar pads left by 12 px and has three square Windows controls.
/// Leave another 64 px empty so the title bar can still be dragged when navigation fills it.
pub(super) fn titlebar_navigation_width(viewport_width: f32) -> f32 {
    (finite_or_zero(viewport_width) - 12. - 64. - 3. * TITLE_BAR_HEIGHT).max(0.)
}

/// The page card's physical bounds. The child web view does not follow the window on its own
/// (`docs/trapstoknow.md`), so this runs on every drag step, resize and DPI change.
pub(super) fn page_card_bounds(frame: CardFrame) -> PageBounds {
    let scale = usable_scale(frame.scale);

    // Fullscreen hides the title bar, toolbar, bars and footer; the page fills the viewport.
    if frame.fullscreen {
        let (x, width) = physical_span(0., finite_or_zero(frame.viewport_width), scale);
        let (y, height) = physical_span(0., finite_or_zero(frame.viewport_height), scale);
        return PageBounds {
            x,
            y,
            width,
            height,
        };
    }

    // Round each edge rather than each size, so neighbouring edges never drift apart.
    let card = card_edges(frame);
    let (x, width) = physical_span(card.left, card.right, scale);
    let (y, height) = physical_span(card.top, card.bottom, scale);
    PageBounds {
        x,
        y,
        width,
        height,
    }
}

/// The card's logical edges outside fullscreen, which the page web view fills.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CardEdges {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// Inset the card from the sidebar's strip, the toolbar and bars, the footer and the right.
pub(super) fn card_edges(frame: CardFrame) -> CardEdges {
    let (_, left) = edge_strip(frame.sidebar_width);
    CardEdges {
        left,
        top: chrome_height() + frame.bar_heights() + CARD_INSET,
        right: finite_or_zero(frame.viewport_width) - CARD_INSET,
        bottom: finite_or_zero(frame.viewport_height) - FOOTER_HEIGHT - CARD_INSET,
    }
}

/// The window fill behind all chrome, leaving `card` unpainted.
///
/// GPUI composites above the page's child window (`docs/trapstoknow.md`), so any pixel painted
/// over the card hides the page. The fill is drawn whole and clipped to each strip around the
/// card, so a gradient stays continuous across them.
pub(crate) fn window_fill_layer(
    card: Option<CardEdges>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let palette = theme::current();
    let viewport = window.viewport_size();
    let plain = plain_fill(card);
    let Some(image) = crate::ground_image::ground_image(&palette.ground, palette.grain, window, cx)
    else {
        return plain.into_any_element();
    };
    // The plain fill stays underneath, so nothing shows through while the image decodes.
    div()
        .absolute()
        .size_full()
        .child(plain)
        .child(image_fill(image, card, viewport))
        .into_any_element()
}

/// The fill GPUI draws itself: one quad clipped to each strip.
fn plain_fill(card: Option<CardEdges>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let background = theme::window_fill();
            for strip in fill_strips(bounds, card) {
                window.with_content_mask(Some(ContentMask { bounds: strip }), |window| {
                    window.paint_quad(fill(bounds, background));
                });
            }
        },
    )
    .absolute()
    .size_full()
}

/// A generated ground: one window-sized image shown through each strip around the card.
fn image_fill(
    image: std::sync::Arc<gpui::Image>,
    card: Option<CardEdges>,
    viewport: Size<Pixels>,
) -> gpui::Div {
    let window = Bounds::new(point(px(0.), px(0.)), viewport);
    let layer = div().absolute().size_full();
    let strips = fill_strips(window, card).into_iter().enumerate();
    layer.children(strips.map(|(index, strip)| {
        div()
            .id(("window-fill", index))
            .absolute()
            .left(strip.origin.x)
            .top(strip.origin.y)
            .w(strip.size.width)
            .h(strip.size.height)
            .overflow_hidden()
            .child(
                img(image.clone())
                    .absolute()
                    .left(-strip.origin.x)
                    .top(-strip.origin.y)
                    .w(viewport.width)
                    .h(viewport.height)
                    .object_fit(ObjectFit::Fill),
            )
    }))
}

/// The whole window, or the four strips around `card` within it.
fn fill_strips(window: Bounds<gpui::Pixels>, card: Option<CardEdges>) -> Vec<Bounds<gpui::Pixels>> {
    let Some(card) = card else {
        return vec![window];
    };
    let (left, top) = (f32::from(window.origin.x), f32::from(window.origin.y));
    let right = left + f32::from(window.size.width);
    let bottom = top + f32::from(window.size.height);
    let strip = |x0: f32, y0: f32, x1: f32, y1: f32| {
        Bounds::new(
            point(px(x0), px(y0)),
            size(px((x1 - x0).max(0.)), px((y1 - y0).max(0.))),
        )
    };
    vec![
        strip(left, top, right, card.top),
        strip(left, card.bottom, right, bottom),
        strip(left, card.top, card.left, card.bottom),
        strip(card.right, card.top, right, card.bottom),
    ]
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
    use syrinx_ui::{SidebarWidthState, sidebar};

    use super::*;

    fn frame(sidebar_width: f32, scale: f32) -> CardFrame {
        CardFrame {
            viewport_width: 1280.,
            viewport_height: 800.,
            sidebar_width,
            notice_height: 0.,
            permission_height: 0.,
            find_height: 0.,
            fullscreen: false,
            scale,
        }
    }

    #[test]
    fn the_fill_surrounds_the_card_and_never_covers_it() {
        let window = Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(800.)));
        let card = card_edges(frame(264., 1.));
        let strips = fill_strips(window, Some(card));
        let area = |bounds: &Bounds<gpui::Pixels>| {
            f32::from(bounds.size.width) * f32::from(bounds.size.height)
        };
        let card_area = (card.right - card.left) * (card.bottom - card.top);
        let painted: f32 = strips.iter().map(area).sum();
        assert_eq!(painted + card_area, 1280. * 800.);
        let inside = point(
            px((card.left + card.right) / 2.),
            px((card.top + card.bottom) / 2.),
        );
        assert!(strips.iter().all(|strip| !strip.contains(&inside)));
        assert_eq!(fill_strips(window, None), vec![window]);
    }

    #[test]
    fn the_web_view_fills_exactly_the_unpainted_card() {
        // Odd sidebar widths, fractional scales and a find bar must not leave a gap or overlap.
        for (sidebar, scale, find) in [(264., 1., 0.), (213., 1.25, 40.), (301., 1.5, 40.)] {
            let frame = CardFrame {
                find_height: find,
                ..frame(sidebar, scale)
            };
            let card = card_edges(frame);
            let bounds = page_card_bounds(frame);
            let edge = |logical: f32| (logical * scale).round();
            assert_eq!(bounds.x as f32, edge(card.left));
            assert_eq!(bounds.y as f32, edge(card.top));
            assert_eq!((bounds.x + bounds.width as i32) as f32, edge(card.right));
            assert_eq!((bounds.y + bounds.height as i32) as f32, edge(card.bottom));
        }
    }

    #[test]
    fn the_permission_prompt_and_find_bar_stack_under_the_notice() {
        let plain = page_card_bounds(frame(264., 1.));
        let barred = page_card_bounds(CardFrame {
            notice_height: 36.,
            permission_height: 44.,
            find_height: 48.,
            ..frame(264., 1.)
        });
        assert_eq!(barred.y - plain.y, 36 + 44 + 48);
        assert_eq!(
            barred.y + barred.height as i32,
            plain.y + plain.height as i32
        );
        assert_eq!((barred.x, barred.width), (plain.x, plain.width));
        let invalid = page_card_bounds(CardFrame {
            permission_height: f32::NAN,
            find_height: -5.,
            ..frame(264., 1.)
        });
        assert_eq!(invalid, plain);
    }

    #[test]
    fn fullscreen_covers_the_viewport_in_every_sidebar_state() {
        for scale in [1., 1.25, 1.5, 2.] {
            for state in SidebarWidthState::ALL {
                let bounds = page_card_bounds(CardFrame {
                    fullscreen: true,
                    notice_height: 36.,
                    permission_height: 44.,
                    find_height: 48.,
                    ..frame(state.reserved_width(), scale)
                });
                assert_eq!((bounds.x, bounds.y), (0, 0), "{}", state.name());
                assert_eq!(bounds.width, (1280. * scale).round() as u32);
                assert_eq!(bounds.height, (800. * scale).round() as u32);
            }
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
            assert_eq!(bounds.y, (chrome_height() + CARD_INSET) as i32);
            assert_eq!(
                bounds.height,
                (800. - chrome_height() - FOOTER_HEIGHT - 2. * CARD_INSET) as u32
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
    fn title_bar_controls_and_page_card_have_disjoint_bounds() {
        for scale in [1., 1.25, 1.5, 1.75, 2.] {
            for state in SidebarWidthState::ALL {
                let frame = frame(state.reserved_width(), scale);
                let card = page_card_bounds(frame);
                let controls_bottom = (chrome_height() * scale).round() as i32;
                assert!(card.y >= controls_bottom + (CARD_INSET * scale).round() as i32);
            }
            let hidden = page_card_bounds(frame(0., scale));
            assert_eq!(
                hidden.y,
                ((TITLE_BAR_HEIGHT + CARD_INSET) * scale).round() as i32
            );
        }
    }

    #[test]
    fn title_bar_navigation_reserves_drag_and_caption_buttons_at_every_window_width() {
        for (name, viewport, expected) in [
            ("narrow", 800., 616.),
            ("small desktop", 1024., 840.),
            ("default", 1280., 1096.),
            ("full HD", 1920., 1736.),
            ("controls fill the width", 184., 0.),
            ("controls exceed the width", 100., 0.),
        ] {
            assert_eq!(titlebar_navigation_width(viewport), expected, "{name}");
        }
        for viewport in [0., -1., f32::NAN] {
            assert_eq!(titlebar_navigation_width(viewport), 0.);
        }
    }

    #[test]
    fn bounds_scale_to_physical_pixels() {
        let logical = page_card_bounds(frame(52., 1.));
        let scaled = page_card_bounds(frame(52., 1.5));
        assert_eq!(scaled.x, 90);
        // One 36 px title bar and the 8 px inset, at 150%.
        assert_eq!(scaled.y, 66);
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
            permission_height: 0.,
            find_height: 0.,
            fullscreen: false,
            scale: 2.,
        });
        assert_eq!((bounds.width, bounds.height), (1, 1));
        let invalid = page_card_bounds(CardFrame {
            viewport_width: f32::NAN,
            viewport_height: f32::INFINITY,
            sidebar_width: f32::NAN,
            notice_height: f32::NAN,
            permission_height: f32::NAN,
            find_height: f32::NAN,
            fullscreen: false,
            scale: 0.,
        });
        assert_eq!(invalid.x, CARD_INSET as i32);
        assert_eq!((invalid.width, invalid.height), (1, 1));
    }
}

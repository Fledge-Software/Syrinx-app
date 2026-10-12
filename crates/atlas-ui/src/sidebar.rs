//! Sidebar width states (D44) and the drag thresholds proposed for the owner's review.
//!
//! D44 fixes the progression (wide with labels, favicon-only, hidden, reversible by dragging)
//! but deliberately fixes no pixel values. Everything marked PROPOSED awaits the owner's
//! approval; the review steps are in `docs/p1-07-design-tokens.md`.

use crate::metrics::chrome::ICON_BUTTON;

/// The wide sidebar's width, from the spec ("Left sidebar, 264px").
pub const WIDE_WIDTH: f32 = 264.;

/// The canvas sidebar's left and right padding (`padding: 8px 10px 10px 12px`).
pub const SIDEBAR_PADDING_LEFT: f32 = 12.;
pub const SIDEBAR_PADDING_RIGHT: f32 = 10.;

/// PROPOSED: favicon-only width: the canvas's side padding around one 30px icon button.
pub const COMPACT_WIDTH: f32 = SIDEBAR_PADDING_LEFT + ICON_BUTTON + SIDEBAR_PADDING_RIGHT;

/// A hidden sidebar reserves no width; it can still peek over the page.
pub const HIDDEN_WIDTH: f32 = 0.;

/// The peeking sidebar overlays the page at full width (canvas `BrowsingPeek`).
pub const PEEK_WIDTH: f32 = WIDE_WIDTH;

/// PROPOSED: how far from the window's left edge the pointer must be to start a peek.
pub const PEEK_EDGE_ZONE: f32 = 8.;

/// PROPOSED: releasing a drag at or above this width settles on wide (midway between
/// compact and wide).
pub const WIDE_SNAP_THRESHOLD: f32 = (COMPACT_WIDTH + WIDE_WIDTH) / 2.;

/// PROPOSED: releasing a drag at or above this width (and below wide's) settles on compact;
/// anything narrower hides the sidebar.
pub const COMPACT_SNAP_THRESHOLD: f32 = COMPACT_WIDTH / 2.;

/// PROPOSED: while dragging, labels show exactly when releasing would settle on wide.
pub const LABELS_VISIBLE_MIN_WIDTH: f32 = WIDE_SNAP_THRESHOLD;

/// The three persistent presentations D44 allows. Peek is a transient overlay, not a state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SidebarWidthState {
    Wide,
    Compact,
    Hidden,
}

impl SidebarWidthState {
    /// All three, widest first.
    pub const ALL: [SidebarWidthState; 3] = [
        SidebarWidthState::Wide,
        SidebarWidthState::Compact,
        SidebarWidthState::Hidden,
    ];

    /// The width this state reserves beside the page card.
    pub const fn reserved_width(self) -> f32 {
        match self {
            SidebarWidthState::Wide => WIDE_WIDTH,
            SidebarWidthState::Compact => COMPACT_WIDTH,
            SidebarWidthState::Hidden => HIDDEN_WIDTH,
        }
    }

    /// Whether hovering the left edge can peek the full sidebar over the page.
    pub const fn can_peek(self) -> bool {
        matches!(self, SidebarWidthState::Hidden)
    }

    pub const fn name(self) -> &'static str {
        match self {
            SidebarWidthState::Wide => "Wide",
            SidebarWidthState::Compact => "Favicon-only",
            SidebarWidthState::Hidden => "Hidden",
        }
    }

    /// One keyboard step narrower; hidden stays hidden.
    pub const fn narrower(self) -> Self {
        match self {
            SidebarWidthState::Wide => SidebarWidthState::Compact,
            SidebarWidthState::Compact | SidebarWidthState::Hidden => SidebarWidthState::Hidden,
        }
    }

    /// One keyboard step wider; wide stays wide.
    pub const fn wider(self) -> Self {
        match self {
            SidebarWidthState::Hidden => SidebarWidthState::Compact,
            SidebarWidthState::Compact | SidebarWidthState::Wide => SidebarWidthState::Wide,
        }
    }
}

/// Whether the sidebar draws its rows at `width`. Below the favicon-only width it draws only
/// its background, without padding, so its laid-out width is exactly `width`.
pub fn draws_content(width: f32) -> bool {
    width >= COMPACT_WIDTH
}

/// The live width while the owner drags the sidebar edge: it follows the pointer, clamped.
pub fn drag_width(pointer_x: f32) -> f32 {
    if pointer_x.is_nan() {
        return HIDDEN_WIDTH;
    }
    pointer_x.clamp(HIDDEN_WIDTH, WIDE_WIDTH)
}

/// The state a drag settles on when released at `width`.
pub fn snap_on_release(width: f32) -> SidebarWidthState {
    let width = drag_width(width);
    if width >= WIDE_SNAP_THRESHOLD {
        return SidebarWidthState::Wide;
    }
    if width >= COMPACT_SNAP_THRESHOLD {
        return SidebarWidthState::Compact;
    }
    SidebarWidthState::Hidden
}

/// Whether tab titles and section labels are drawn at a live drag width.
pub fn labels_visible(width: f32) -> bool {
    drag_width(width) >= LABELS_VISIBLE_MIN_WIDTH
}

/// Whether the pointer is close enough to the left edge to peek a hidden sidebar.
pub fn in_peek_zone(state: SidebarWidthState, pointer_x: f32) -> bool {
    state.can_peek() && (0.0..PEEK_EDGE_ZONE).contains(&pointer_x)
}

/// A drag of the sidebar's edge: the width it shows and where the pointer gripped the handle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct EdgeDrag {
    grip_offset: f32,
    width: f32,
}

/// The sidebar's settled width state plus any drag in progress (D44).
///
/// The page card and the rows both read [`SidebarSizing::width`], so they follow the
/// pointer during a drag and the settled state otherwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarSizing {
    state: SidebarWidthState,
    drag: Option<EdgeDrag>,
}

impl SidebarSizing {
    pub const fn new(state: SidebarWidthState) -> Self {
        Self { state, drag: None }
    }

    /// The state the sidebar settles in when no drag is in progress.
    pub const fn state(&self) -> SidebarWidthState {
        self.state
    }

    pub const fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The width reserved beside the page card right now.
    pub fn width(&self) -> f32 {
        match self.drag {
            Some(drag) => drag.width,
            None => self.state.reserved_width(),
        }
    }

    /// Whether rows draw their titles: exactly when a release now would settle on wide.
    pub fn labels_visible(&self) -> bool {
        match self.drag {
            Some(drag) => labels_visible(drag.width),
            None => self.state == SidebarWidthState::Wide,
        }
    }

    /// Starts a drag with the pointer on the edge handle, keeping the width where it is.
    pub fn begin_drag(&mut self, pointer_x: f32) {
        let width = self.width();
        let grip_offset = if pointer_x.is_finite() {
            pointer_x - width
        } else {
            0.
        };
        self.drag = Some(EdgeDrag { grip_offset, width });
    }

    /// Follows the pointer during a drag. Returns whether the width changed.
    pub fn drag_to(&mut self, pointer_x: f32) -> bool {
        let Some(drag) = self.drag.as_mut() else {
            return false;
        };
        let width = drag_width(pointer_x - drag.grip_offset);
        if width == drag.width {
            return false;
        }
        drag.width = width;
        true
    }

    /// Ends a drag by snapping to the nearest state. Returns the new state, or `None`
    /// when no drag was in progress.
    pub fn release(&mut self) -> Option<SidebarWidthState> {
        let drag = self.drag.take()?;
        self.state = snap_on_release(drag.width);
        Some(self.state)
    }

    /// Ends a drag where the button came up. Moves stop arriving once the pointer leaves the
    /// window, so the release position, not the last move, decides the state.
    pub fn release_at(&mut self, pointer_x: f32) -> Option<SidebarWidthState> {
        self.drag_to(pointer_x);
        self.release()
    }

    /// Settles on `state` directly, abandoning any drag.
    pub fn set_state(&mut self, state: SidebarWidthState) {
        self.drag = None;
        self.state = state;
    }
}

impl Default for SidebarSizing {
    fn default() -> Self {
        Self::new(SidebarWidthState::Wide)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drags from the edge of `sizing` to `pointer_x` and releases there.
    fn drag_and_release(sizing: &mut SidebarSizing, pointer_x: f32) -> Option<SidebarWidthState> {
        sizing.begin_drag(sizing.width());
        sizing.drag_to(pointer_x);
        sizing.release()
    }

    #[test]
    fn a_settled_sidebar_reserves_its_state_width() {
        let sizing = SidebarSizing::default();
        assert_eq!(sizing.state(), SidebarWidthState::Wide);
        assert_eq!(sizing.width(), WIDE_WIDTH);
        assert!(sizing.labels_visible());
        assert!(!sizing.is_dragging());
        assert!(!SidebarSizing::new(SidebarWidthState::Compact).labels_visible());
    }

    #[test]
    fn dragging_follows_the_pointer_from_where_the_handle_was_gripped() {
        let mut sizing = SidebarSizing::default();
        // Grip the handle 3 px right of the sidebar's edge.
        sizing.begin_drag(WIDE_WIDTH + 3.);
        assert_eq!(sizing.width(), WIDE_WIDTH);
        assert!(sizing.drag_to(203.));
        assert_eq!(sizing.width(), 200.);
        assert!(
            !sizing.drag_to(203.),
            "an unchanged width reports no change"
        );
        assert!(sizing.drag_to(-50.));
        assert_eq!(sizing.width(), HIDDEN_WIDTH);
        assert!(sizing.drag_to(5_000.));
        assert_eq!(sizing.width(), WIDE_WIDTH);
    }

    #[test]
    fn releases_snap_at_the_owner_reviewed_thresholds() {
        assert_eq!(WIDE_SNAP_THRESHOLD, 158.);
        assert_eq!(COMPACT_SNAP_THRESHOLD, 26.);
        let cases = [
            (158., SidebarWidthState::Wide),
            (157.9, SidebarWidthState::Compact),
            (26., SidebarWidthState::Compact),
            (25.9, SidebarWidthState::Hidden),
        ];
        for (pointer_x, expected) in cases {
            let mut sizing = SidebarSizing::default();
            assert_eq!(drag_and_release(&mut sizing, pointer_x), Some(expected));
            assert_eq!(sizing.width(), expected.reserved_width());
        }
    }

    #[test]
    fn labels_show_during_a_drag_exactly_when_release_would_settle_wide() {
        let mut sizing = SidebarSizing::new(SidebarWidthState::Hidden);
        sizing.begin_drag(0.);
        let mut pointer_x = 0.;
        while pointer_x <= WIDE_WIDTH {
            sizing.drag_to(pointer_x);
            let wide = snap_on_release(sizing.width()) == SidebarWidthState::Wide;
            assert_eq!(sizing.labels_visible(), wide, "at {pointer_x}");
            pointer_x += 0.5;
        }
    }

    #[test]
    fn forward_and_reverse_drags_visit_every_state() {
        let mut sizing = SidebarSizing::default();
        // Wide to favicon-only to hidden...
        assert_eq!(
            drag_and_release(&mut sizing, COMPACT_WIDTH),
            Some(SidebarWidthState::Compact)
        );
        assert_eq!(
            drag_and_release(&mut sizing, 0.),
            Some(SidebarWidthState::Hidden)
        );
        // ...and back: hidden to favicon-only to wide.
        assert_eq!(
            drag_and_release(&mut sizing, 40.),
            Some(SidebarWidthState::Compact)
        );
        assert_eq!(
            drag_and_release(&mut sizing, 200.),
            Some(SidebarWidthState::Wide)
        );
        // A reverse drag straight from hidden to wide skips favicon-only.
        sizing.set_state(SidebarWidthState::Hidden);
        assert_eq!(
            drag_and_release(&mut sizing, WIDE_WIDTH),
            Some(SidebarWidthState::Wide)
        );
    }

    #[test]
    fn a_click_without_movement_keeps_the_state() {
        for state in SidebarWidthState::ALL {
            let mut sizing = SidebarSizing::new(state);
            sizing.begin_drag(state.reserved_width() + 4.);
            assert_eq!(sizing.release(), Some(state));
        }
    }

    #[test]
    fn moves_and_releases_without_a_drag_change_nothing() {
        let mut sizing = SidebarSizing::new(SidebarWidthState::Compact);
        assert!(!sizing.drag_to(10.));
        assert_eq!(sizing.release(), None);
        assert_eq!(sizing.state(), SidebarWidthState::Compact);
    }

    #[test]
    fn setting_a_state_abandons_a_drag() {
        let mut sizing = SidebarSizing::default();
        sizing.begin_drag(WIDE_WIDTH);
        sizing.drag_to(10.);
        sizing.set_state(SidebarWidthState::Compact);
        assert!(!sizing.is_dragging());
        assert_eq!(sizing.width(), COMPACT_WIDTH);
        assert_eq!(sizing.release(), None);
    }

    #[test]
    fn a_flick_past_the_window_edge_snaps_where_the_button_came_up() {
        let mut sizing = SidebarSizing::default();
        sizing.begin_drag(WIDE_WIDTH);
        // The last move seen inside the window would settle on favicon-only...
        sizing.drag_to(60.);
        // ...but the button came up left of the window.
        assert_eq!(sizing.release_at(-120.), Some(SidebarWidthState::Hidden));
        // And a flick right past the window settles wide.
        sizing.begin_drag(0.);
        sizing.drag_to(30.);
        assert_eq!(sizing.release_at(4_000.), Some(SidebarWidthState::Wide));
        // Without a drag, a release changes nothing.
        assert_eq!(sizing.release_at(0.), None);
        assert_eq!(sizing.state(), SidebarWidthState::Wide);
    }

    #[test]
    fn keyboard_steps_walk_the_three_states_and_stop_at_the_ends() {
        use SidebarWidthState::{Compact, Hidden, Wide};
        assert_eq!(Wide.narrower(), Compact);
        assert_eq!(Compact.narrower(), Hidden);
        assert_eq!(Hidden.narrower(), Hidden);
        assert_eq!(Hidden.wider(), Compact);
        assert_eq!(Compact.wider(), Wide);
        assert_eq!(Wide.wider(), Wide);
    }

    #[test]
    fn content_padding_never_exceeds_the_width_it_is_drawn_in() {
        let mut width = 0.;
        while width <= WIDE_WIDTH {
            if draws_content(width) {
                assert!(
                    SIDEBAR_PADDING_LEFT + SIDEBAR_PADDING_RIGHT <= width,
                    "at {width}"
                );
            }
            width += 0.25;
        }
        assert!(draws_content(COMPACT_WIDTH));
        assert!(!draws_content(COMPACT_WIDTH - 0.25));
    }

    #[test]
    fn a_non_finite_pointer_cannot_corrupt_the_width() {
        let mut sizing = SidebarSizing::default();
        sizing.begin_drag(f32::NAN);
        sizing.drag_to(f32::NAN);
        assert_eq!(sizing.width(), HIDDEN_WIDTH);
        sizing.drag_to(f32::INFINITY);
        assert_eq!(sizing.width(), WIDE_WIDTH);
    }

    #[test]
    fn widths_shrink_through_the_d44_progression() {
        const { assert!(WIDE_WIDTH > COMPACT_WIDTH && COMPACT_WIDTH > HIDDEN_WIDTH) };
        assert_eq!(COMPACT_WIDTH, 52.);
        assert_eq!(PEEK_WIDTH, WIDE_WIDTH);
    }

    #[test]
    fn releasing_at_each_state_width_keeps_that_state() {
        for state in SidebarWidthState::ALL {
            assert_eq!(snap_on_release(state.reserved_width()), state);
        }
    }

    #[test]
    fn dragging_narrower_never_widens_the_settled_state() {
        let mut previous = SidebarWidthState::Wide.reserved_width();
        let mut width = WIDE_WIDTH;
        while width >= 0. {
            let reserved = snap_on_release(width).reserved_width();
            assert!(
                reserved <= previous,
                "release at {width} widened the sidebar"
            );
            previous = reserved;
            width -= 1.;
        }
    }

    #[test]
    fn reverse_drag_restores_every_state() {
        // Hidden back to compact and wide, the reverse of D44's progression.
        assert_eq!(
            snap_on_release(COMPACT_SNAP_THRESHOLD),
            SidebarWidthState::Compact
        );
        assert_eq!(
            snap_on_release(WIDE_SNAP_THRESHOLD),
            SidebarWidthState::Wide
        );
        assert_eq!(snap_on_release(f32::MAX), SidebarWidthState::Wide);
    }

    #[test]
    fn out_of_range_pointers_are_clamped() {
        assert_eq!(drag_width(-40.), HIDDEN_WIDTH);
        assert_eq!(drag_width(900.), WIDE_WIDTH);
        assert_eq!(drag_width(f32::NAN), HIDDEN_WIDTH);
        assert_eq!(snap_on_release(-1.), SidebarWidthState::Hidden);
    }

    #[test]
    fn labels_hide_before_the_sidebar_reaches_compact() {
        assert!(labels_visible(WIDE_WIDTH));
        assert!(!labels_visible(COMPACT_WIDTH));
        assert_eq!(
            labels_visible(WIDE_SNAP_THRESHOLD),
            snap_on_release(WIDE_SNAP_THRESHOLD) == SidebarWidthState::Wide
        );
    }

    #[test]
    fn only_a_hidden_sidebar_peeks() {
        assert!(in_peek_zone(SidebarWidthState::Hidden, 0.));
        assert!(!in_peek_zone(SidebarWidthState::Hidden, PEEK_EDGE_ZONE));
        assert!(!in_peek_zone(SidebarWidthState::Compact, 0.));
        assert!(!in_peek_zone(SidebarWidthState::Wide, 0.));
    }
}

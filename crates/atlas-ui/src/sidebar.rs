//! Sidebar width states (D44) and the drag thresholds proposed for the owner's review.
//!
//! D44 fixes the progression (wide with labels, favicon-only, hidden, reversible by dragging)
//! but deliberately fixes no pixel values. Everything marked PROPOSED awaits the owner's
//! approval; the review steps are in `docs/p1-07-design-tokens.md`.

use crate::metrics::chrome::ICON_BUTTON;

/// The wide sidebar's width, from the spec ("Left sidebar, 264px").
pub const WIDE_WIDTH: f32 = 264.;

/// The canvas sidebar's left and right padding (`padding: 8px 10px 10px 12px`).
const SIDEBAR_PADDING_LEFT: f32 = 12.;
const SIDEBAR_PADDING_RIGHT: f32 = 10.;

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

#[cfg(test)]
mod tests {
    use super::*;

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

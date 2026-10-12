//! Decides when a page's web view needs new bounds, so hidden pages skip every drag step.

use super::PageBounds;

/// The bounds a page's web view last received and whether it is shown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Placement {
    applied: PageBounds,
    visible: bool,
}

impl Placement {
    /// A page is created hidden, already at the card's bounds.
    pub(super) const fn new(bounds: PageBounds) -> Self {
        Self {
            applied: bounds,
            visible: false,
        }
    }

    /// The bounds to apply now that the card moved to `card`: only a shown page follows.
    pub(super) fn on_resize(&self, card: PageBounds) -> Option<PageBounds> {
        (self.visible && self.applied != card).then_some(card)
    }

    /// The bounds to catch up to before a page is shown at `card`.
    pub(super) fn on_show(&self, visible: bool, card: PageBounds) -> Option<PageBounds> {
        (visible && self.applied != card).then_some(card)
    }

    pub(super) fn applied(&mut self, bounds: PageBounds) {
        self.applied = bounds;
    }

    pub(super) fn shown(&mut self, visible: bool) {
        self.visible = visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(x: i32) -> PageBounds {
        PageBounds {
            x,
            y: 56,
            width: 900,
            height: 600,
        }
    }

    #[test]
    fn hidden_pages_skip_drag_steps_and_catch_up_when_shown() {
        let mut placement = Placement::new(card(272));
        // A drag moves the card; the hidden page is left alone each step.
        for x in [200, 120, 60] {
            assert_eq!(placement.on_resize(card(x)), None);
        }
        // Shown later, it receives the card's final bounds once, before becoming visible.
        assert_eq!(placement.on_show(true, card(60)), Some(card(60)));
        placement.applied(card(60));
        placement.shown(true);
        assert_eq!(placement.on_show(true, card(60)), None);
    }

    #[test]
    fn the_shown_page_follows_every_change_once() {
        let mut placement = Placement::new(card(272));
        placement.shown(true);
        assert_eq!(placement.on_resize(card(272)), None);
        assert_eq!(placement.on_resize(card(200)), Some(card(200)));
        placement.applied(card(200));
        assert_eq!(placement.on_resize(card(200)), None);
    }

    #[test]
    fn hiding_a_page_needs_no_bounds() {
        let mut placement = Placement::new(card(272));
        placement.shown(true);
        assert_eq!(placement.on_show(false, card(8)), None);
        placement.shown(false);
        assert_eq!(placement.on_resize(card(8)), None);
    }
}

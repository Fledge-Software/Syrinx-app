//! Applies the shared placement policy before GTK maps a page, without requiring a display.

use std::cell::Cell;

use super::{PageBounds, placement::Placement};

pub(super) struct PagePlacement(Cell<Placement>);

impl PagePlacement {
    pub(super) fn new(bounds: PageBounds) -> Self {
        Self(Cell::new(Placement::new(bounds)))
    }

    pub(super) fn resize(&self, card: PageBounds, place: impl FnOnce(PageBounds)) {
        let Some(bounds) = self.0.get().on_resize(card) else {
            return;
        };
        self.apply(bounds, place);
    }

    pub(super) fn show(
        &self,
        visible: bool,
        card: PageBounds,
        place: impl FnOnce(PageBounds),
        map: impl FnOnce(bool),
    ) {
        // A hidden page catches up before GTK and X11 make it visible.
        if let Some(bounds) = self.0.get().on_show(visible, card) {
            self.apply(bounds, place);
        }
        map(visible);
        let mut placement = self.0.get();
        placement.shown(visible);
        self.0.set(placement);
    }

    /// Record geometry only after the native operation has run.
    fn apply(&self, bounds: PageBounds, place: impl FnOnce(PageBounds)) {
        place(bounds);
        let mut placement = self.0.get();
        placement.applied(bounds);
        self.0.set(placement);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, PartialEq)]
    enum NativeCall {
        Bounds(PageBounds),
        Mapped(bool),
    }

    const BEFORE: PageBounds = PageBounds {
        x: 19,
        y: 29,
        width: 480,
        height: 300,
    };
    const NEXT: PageBounds = PageBounds {
        x: 7,
        y: 11,
        width: 320,
        height: 240,
    };

    #[test]
    fn a_hidden_survivor_keeps_its_bounds_until_it_is_shown() {
        let page = PagePlacement::new(BEFORE);
        let calls = RefCell::new(Vec::new());
        page.show(
            true,
            BEFORE,
            |bounds| calls.borrow_mut().push(NativeCall::Bounds(bounds)),
            |visible| calls.borrow_mut().push(NativeCall::Mapped(visible)),
        );
        page.show(
            false,
            BEFORE,
            |bounds| calls.borrow_mut().push(NativeCall::Bounds(bounds)),
            |visible| calls.borrow_mut().push(NativeCall::Mapped(visible)),
        );
        page.resize(NEXT, |bounds| {
            calls.borrow_mut().push(NativeCall::Bounds(bounds))
        });
        assert_eq!(
            *calls.borrow(),
            [NativeCall::Mapped(true), NativeCall::Mapped(false)],
            "hidden resize must not call XMoveResizeWindow or GTK allocation"
        );
        page.show(
            true,
            NEXT,
            |bounds| calls.borrow_mut().push(NativeCall::Bounds(bounds)),
            |visible| calls.borrow_mut().push(NativeCall::Mapped(visible)),
        );
        assert_eq!(
            *calls.borrow(),
            [
                NativeCall::Mapped(true),
                NativeCall::Mapped(false),
                NativeCall::Bounds(NEXT),
                NativeCall::Mapped(true)
            ],
            "catch up to the card before mapping the surviving page"
        );
    }

    #[test]
    fn a_shown_page_applies_each_changed_rectangle_once() {
        let page = PagePlacement::new(BEFORE);
        let calls = RefCell::new(Vec::new());
        page.show(
            true,
            BEFORE,
            |bounds| calls.borrow_mut().push(NativeCall::Bounds(bounds)),
            |visible| calls.borrow_mut().push(NativeCall::Mapped(visible)),
        );
        for bounds in [BEFORE, NEXT, NEXT, BEFORE] {
            page.resize(bounds, |bounds| {
                calls.borrow_mut().push(NativeCall::Bounds(bounds))
            });
        }
        assert_eq!(
            *calls.borrow(),
            [
                NativeCall::Mapped(true),
                NativeCall::Bounds(NEXT),
                NativeCall::Bounds(BEFORE)
            ]
        );
    }
}

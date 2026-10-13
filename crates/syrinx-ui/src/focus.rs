//! The keyboard focus contract: which panes F6 visits, in what order, and how the focus ring looks.
//!
//! Window-independent so the order can be tested without GPUI or WebView2 (P1-36).

use crate::accent::AccentColors;
use crate::color::Color;

/// How wide the keyboard focus ring is, in logical pixels.
pub const RING_WIDTH: f32 = 2.;

/// A thin line just inside the ring, so the ring still reads on accent-filled controls.
pub const RING_INNER_WIDTH: f32 = 1.;

/// The ring's colour: the accent ink, which `contrast::audit` already holds to WCAG 1.4.11's
/// 3:1 against the ground, panel and card it is drawn on.
pub const fn ring_color(accent: &AccentColors) -> Color {
    accent.ink
}

/// The inner line's colour: the text drawn on accent fills, audited at 4.5:1 against them.
pub const fn ring_inner_color(accent: &AccentColors) -> Color {
    accent.on_fill
}

/// The regions F6 moves between, in the order it visits them: top to bottom, then left to
/// right, as Edge moves from its address bar to the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    /// The toolbar's address field (in the title bar when the sidebar is hidden).
    Address,
    /// The WebView2 runtime-change notice under the toolbar.
    RuntimeNotice,
    /// A site's permission request.
    Permission,
    /// The find-in-page bar.
    Find,
    /// The sidebar's search, Favorites and tab rows.
    Sidebar,
    /// The page card: a web page, or Syrinx's own home, history or recovery content.
    Page,
}

impl Pane {
    /// Every pane in F6 order.
    pub const ORDER: [Pane; 6] = [
        Pane::Address,
        Pane::RuntimeNotice,
        Pane::Permission,
        Pane::Find,
        Pane::Sidebar,
        Pane::Page,
    ];

    /// The pane's name for screen readers and logs.
    pub const fn name(self) -> &'static str {
        match self {
            Pane::Address => "Address",
            Pane::RuntimeNotice => "Runtime notice",
            Pane::Permission => "Permission request",
            Pane::Find => "Find in page",
            Pane::Sidebar => "Sidebar",
            Pane::Page => "Page",
        }
    }

    /// The pane's index in `ORDER`; a test keeps the two in step.
    const fn position(self) -> usize {
        match self {
            Pane::Address => 0,
            Pane::RuntimeNotice => 1,
            Pane::Permission => 2,
            Pane::Find => 3,
            Pane::Sidebar => 4,
            Pane::Page => 5,
        }
    }
}

/// Which end of a pane focus enters from: F6 and Tab enter at the first control, Shift Tab
/// out of a page enters the previous pane at its last control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    First,
    Last,
}

/// Where focus goes on entering a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// The native web page, focused through WebView2.
    WebPage,
    /// The address field itself, not the toolbar's buttons.
    AddressField,
    /// A control inside the pane's region, from the given end.
    Control(Entry),
}

/// Decides where entering `pane` puts focus. `web_page_shown` says the page card holds a
/// native web page rather than Syrinx's own content.
pub const fn landing(pane: Pane, entry: Entry, web_page_shown: bool) -> Landing {
    match pane {
        Pane::Page if web_page_shown => Landing::WebPage,
        Pane::Address => Landing::AddressField,
        _ => Landing::Control(entry),
    }
}

/// What stepping through Tab order from a pane's region found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settle {
    /// Keep the control the step reached.
    Control,
    /// Focus the region itself: the step left the pane, which has no Tab stops.
    Region,
}

/// Keeps a stepped-to control only while it lies inside the target pane, so a pane with no
/// Tab stops never hands focus to a control in another pane.
pub const fn settle(reached_control_inside_pane: bool) -> Settle {
    if reached_control_inside_pane {
        Settle::Control
    } else {
        Settle::Region
    }
}

/// Which panes the window currently shows. The address field and the page card are always
/// drawn; the bars come and go, and a hidden sidebar is skipped (it peeks without focus).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenPanes {
    pub runtime_notice: bool,
    pub permission: bool,
    pub find: bool,
    pub sidebar: bool,
}

impl OpenPanes {
    /// Whether F6 may land on `pane` now.
    pub const fn contains(self, pane: Pane) -> bool {
        match pane {
            Pane::Address | Pane::Page => true,
            Pane::RuntimeNotice => self.runtime_notice,
            Pane::Permission => self.permission,
            Pane::Find => self.find,
            Pane::Sidebar => self.sidebar,
        }
    }
}

/// F6 moves forward; Shift F6 moves backward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

/// The pane F6 lands on next, wrapping at either end.
///
/// `current` is the pane holding focus, or `None` when focus is outside every pane (a fresh
/// window). A pane that has just closed still counts from its place in the order, so focus
/// leaving a dismissed bar continues to its neighbour rather than restarting.
pub fn next_pane(current: Option<Pane>, open: OpenPanes, direction: Direction) -> Pane {
    let count = Pane::ORDER.len();
    let Some(current) = current else {
        return first_open(open, direction);
    };
    let start = current.position();
    (1..=count)
        .map(|step| match direction {
            Direction::Forward => (start + step) % count,
            Direction::Backward => (start + count - step) % count,
        })
        .map(|index| Pane::ORDER[index])
        .find(|pane| open.contains(*pane))
        .unwrap_or(Pane::Address)
}

/// Where F6 starts when nothing is focused: the address field forward, the page backward.
fn first_open(open: OpenPanes, direction: Direction) -> Pane {
    let mut panes = Pane::ORDER.into_iter();
    let found = match direction {
        Direction::Forward => panes.find(|pane| open.contains(*pane)),
        Direction::Backward => panes.rev().find(|pane| open.contains(*pane)),
    };
    found.unwrap_or(Pane::Address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Accent, PaletteId, palette};

    const ALL_OPEN: OpenPanes = OpenPanes {
        runtime_notice: true,
        permission: true,
        find: true,
        sidebar: true,
    };

    /// Visits every pane forward from `start` until it returns there.
    fn cycle(start: Pane, open: OpenPanes, direction: Direction) -> Vec<Pane> {
        let mut visited = vec![start];
        let mut pane = next_pane(Some(start), open, direction);
        while pane != start {
            visited.push(pane);
            pane = next_pane(Some(pane), open, direction);
            assert!(visited.len() <= Pane::ORDER.len(), "F6 never returned");
        }
        visited
    }

    #[test]
    fn f6_visits_every_open_pane_in_order_and_wraps() {
        assert_eq!(
            cycle(Pane::Address, ALL_OPEN, Direction::Forward),
            Pane::ORDER
        );
    }

    #[test]
    fn shift_f6_visits_the_same_panes_in_reverse() {
        let mut reversed = Pane::ORDER;
        reversed.reverse();
        assert_eq!(cycle(Pane::Page, ALL_OPEN, Direction::Backward), reversed);
    }

    #[test]
    fn closed_bars_and_a_hidden_sidebar_are_skipped() {
        let quiet = OpenPanes::default();
        assert_eq!(
            cycle(Pane::Address, quiet, Direction::Forward),
            [Pane::Address, Pane::Page]
        );
        let with_sidebar = OpenPanes {
            sidebar: true,
            ..OpenPanes::default()
        };
        assert_eq!(
            cycle(Pane::Page, with_sidebar, Direction::Forward),
            [Pane::Page, Pane::Address, Pane::Sidebar]
        );
    }

    #[test]
    fn each_bar_joins_the_cycle_only_while_open() {
        for (open, bar) in [
            (
                OpenPanes {
                    runtime_notice: true,
                    ..OpenPanes::default()
                },
                Pane::RuntimeNotice,
            ),
            (
                OpenPanes {
                    permission: true,
                    ..OpenPanes::default()
                },
                Pane::Permission,
            ),
            (
                OpenPanes {
                    find: true,
                    ..OpenPanes::default()
                },
                Pane::Find,
            ),
        ] {
            assert_eq!(
                next_pane(Some(Pane::Address), open, Direction::Forward),
                bar
            );
            assert_eq!(next_pane(Some(bar), open, Direction::Forward), Pane::Page);
            assert_eq!(next_pane(Some(Pane::Page), open, Direction::Backward), bar);
        }
    }

    #[test]
    fn a_bar_that_just_closed_hands_focus_to_its_neighbour() {
        let open = OpenPanes {
            sidebar: true,
            ..OpenPanes::default()
        };
        assert_eq!(
            next_pane(Some(Pane::Permission), open, Direction::Forward),
            Pane::Sidebar
        );
        assert_eq!(
            next_pane(Some(Pane::Permission), open, Direction::Backward),
            Pane::Address
        );
    }

    #[test]
    fn with_nothing_focused_f6_starts_at_the_address_and_shift_f6_at_the_page() {
        assert_eq!(
            next_pane(None, OpenPanes::default(), Direction::Forward),
            Pane::Address
        );
        assert_eq!(next_pane(None, ALL_OPEN, Direction::Backward), Pane::Page);
    }

    #[test]
    fn the_page_is_always_reachable() {
        for runtime_notice in [false, true] {
            for permission in [false, true] {
                for find in [false, true] {
                    for sidebar in [false, true] {
                        let open = OpenPanes {
                            runtime_notice,
                            permission,
                            find,
                            sidebar,
                        };
                        assert!(
                            cycle(Pane::Address, open, Direction::Forward).contains(&Pane::Page)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn positions_match_the_order() {
        for (index, pane) in Pane::ORDER.into_iter().enumerate() {
            assert_eq!(pane.position(), index, "{pane:?}");
        }
    }

    #[test]
    fn landing_puts_focus_where_the_pane_starts() {
        for entry in [Entry::First, Entry::Last] {
            assert_eq!(landing(Pane::Page, entry, true), Landing::WebPage);
            assert_eq!(landing(Pane::Page, entry, false), Landing::Control(entry));
            assert_eq!(landing(Pane::Address, entry, true), Landing::AddressField);
            assert_eq!(landing(Pane::Sidebar, entry, true), Landing::Control(entry));
        }
    }

    #[test]
    fn a_pane_without_tab_stops_keeps_focus_on_its_region() {
        assert_eq!(settle(true), Settle::Control);
        assert_eq!(settle(false), Settle::Region);
    }

    #[test]
    fn the_ring_is_the_audited_accent_ink_in_every_palette() {
        for id in PaletteId::ALL {
            let drawn = palette(id, Accent::default());
            assert_eq!(ring_color(&drawn.accent), drawn.accent.ink);
        }
    }

    #[test]
    fn the_inner_line_stands_out_on_every_accent_fill() {
        for accent in crate::ACCENT_PRESETS {
            for id in PaletteId::ALL {
                let colors = palette(id, Accent::new(accent)).accent;
                let ratio = crate::contrast_ratio(ring_inner_color(&colors), colors.fill);
                assert!(ratio >= crate::contrast::NON_TEXT_MINIMUM, "{id:?} {ratio}");
            }
        }
    }
}

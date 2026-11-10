//! Routes Back, Forward and their menus to the tab model's back and forward lists (#126).

use std::{cell::Cell, collections::HashMap, rc::Rc, time::Duration};

use gpui::{
    Context, InteractiveElement, KeyBinding, MouseButton, ParentElement, Pixels, Point,
    StatefulInteractiveElement, Styled, Task, Window, canvas, prelude::FluentBuilder as _,
};
use syrinx_core::{HistoryDirection, HistoryReach, TabId, usage::Feature};
use syrinx_platform::browser::{BrowserCommand, syrinx_in_foreground};

use super::{BrowserView, menus::MenuKind};
use crate::session::{HistoryListRow, SessionCommand};
use crate::theme;
use requests::{Press, RowsRequests, Stale};

mod requests;

gpui::actions!(back_forward, [OpenBackList, OpenForwardList]);

/// How long Back or Forward must be held before its list opens, as in Chrome.
const HOLD: Duration = Duration::from_millis(500);
/// Everything that differs between Back and Forward, in one place.
pub(super) struct Way {
    /// The menu's accessible name.
    pub(super) menu_name: &'static str,
    /// The note a reset list shows when the engine can still go this way.
    pub(super) unlisted: &'static str,
    /// The word a row's distance ends with, as in "3 pages back".
    pub(super) distance: &'static str,
    /// What a screen reader hears on the dimmed button.
    unavailable: &'static str,
    /// The button's key context, where Alt Down opens its menu (owner, 2026-10-28).
    key_context: &'static str,
    /// The existing usage count a step adds to (AN-01); the menu itself counts nothing.
    feature: Feature,
}

const BACK_WAY: Way = Way {
    menu_name: "Back history",
    unlisted: "Earlier pages aren't listed",
    distance: "back",
    unavailable: "No earlier page",
    key_context: "SyrinxBack",
    feature: Feature::NavBack,
};

const FORWARD_WAY: Way = Way {
    menu_name: "Forward history",
    unlisted: "Later pages aren't listed",
    distance: "forward",
    unavailable: "No later page",
    key_context: "SyrinxForward",
    feature: Feature::NavForward,
};

/// Back's or Forward's facts.
pub(super) fn way(direction: HistoryDirection) -> &'static Way {
    match direction {
        HistoryDirection::Back => &BACK_WAY,
        HistoryDirection::Forward => &FORWARD_WAY,
    }
}

/// Binds Alt Down on a focused Back or Forward button.
pub(super) fn bind_keys(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("alt-down", OpenBackList, Some(BACK_WAY.key_context)),
        KeyBinding::new("alt-down", OpenForwardList, Some(FORWARD_WAY.key_context)),
    ]);
}

/// What the window keeps for Back and Forward: where each tab's list and engine reach, as the
/// tab model last reported, the press being held, the menu whose rows are being read and where
/// each button was drawn. The lists themselves live in the model.
#[derive(Default)]
pub(super) struct BackForward {
    reach: HashMap<TabId, Reach>,
    hold: HoldState,
    /// The held press's timer; its result is an error only once the view has closed.
    hold_timer: Option<Task<gpui::Result<()>>>,
    /// The menu whose rows the worker is reading.
    requests: RowsRequests,
    /// Each button's bottom-left in the main window, where its menu hangs.
    back_anchor: Anchor,
    forward_anchor: Anchor,
}

/// Where one tab's list reaches, and where its engine can still go after the list reset.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Reach {
    listed: HistoryReach,
    engine: HistoryReach,
}

/// Whether `reach` goes `direction`.
fn goes(reach: HistoryReach, direction: HistoryDirection) -> bool {
    match direction {
        HistoryDirection::Back => reach.back,
        HistoryDirection::Forward => reach.forward,
    }
}

/// A button's bottom-left as its last main-window paint placed it.
type Anchor = Rc<Cell<Option<Point<Pixels>>>>;

/// Which button a held press began on, and which one it opened a list from. GPUI runs a
/// mouse-up-outside listener on every button the pointer is not over, so each release must
/// name its own button (review of #212).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct HoldState {
    pressed: Option<HistoryDirection>,
    opened: Option<HistoryDirection>,
}

impl HoldState {
    /// A press begins on `direction`'s button.
    fn press(&mut self, direction: HistoryDirection) {
        self.pressed = Some(direction);
        self.opened = None;
    }

    /// The hold time passed; true if that same press is still down.
    fn elapse(&mut self, direction: HistoryDirection) -> bool {
        self.pressed
            .take_if(|pressed| *pressed == direction)
            .is_some()
    }

    /// The held press opened `direction`'s list, so its release is not also a click.
    fn open(&mut self, direction: HistoryDirection) {
        self.opened = Some(direction);
    }

    /// A release reported by `direction`'s button; `on_button` false means it fell elsewhere,
    /// so no click follows. Releases reported by the other button change nothing here.
    fn release(&mut self, direction: HistoryDirection, on_button: bool) {
        if self.pressed == Some(direction) {
            self.pressed = None;
        }
        if !on_button && self.opened == Some(direction) {
            self.opened = None;
        }
    }

    /// A click on `direction`'s button; true when it should step.
    fn click(&mut self, direction: HistoryDirection) -> bool {
        self.opened.take_if(|opened| *opened == direction).is_none()
    }
}

/// What a right-click on Back or Forward does. It never reaches the page menu, even on a
/// dimmed button (owner, 2026-10-28), so no variant passes it on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RightClick {
    OpenMenu,
    Nothing,
}

impl RightClick {
    /// A menu opens only when the list or the engine can go that way.
    fn on(offered: bool) -> Self {
        if offered {
            Self::OpenMenu
        } else {
            Self::Nothing
        }
    }
}

impl BrowserView {
    /// Hands a page's history report to the tab model, which keeps the list (owner, 2026-10-28).
    pub(super) fn history_reported(
        &mut self,
        id: u64,
        report: syrinx_core::HistoryReport,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.open_tab(id) else {
            return;
        };
        if self.shown == Some(tab) {
            let stale = self.back_forward.requests.page_reported(report.change);
            self.close_stale_history_menu(stale, window, cx);
        }
        self.command(SessionCommand::ObserveHistory { id: tab, report });
    }

    /// Closes an open history menu whose rows went stale as Escape does, so the keys go back
    /// to the page (review of #213; trap: "Keys go to the page while GPUI still shows a
    /// caret"). Its rows would otherwise jump to the wrong page.
    fn close_stale_history_menu(
        &mut self,
        stale: Stale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if stale == Stale::Keep || !matches!(self.open_menu_kind(), Some(MenuKind::History(_))) {
            return;
        }
        self.dismiss_menu(window, cx);
    }

    /// Forgets a menu whose rows are still being read, so its late reply opens nothing.
    pub(super) fn cancel_history_request(&mut self) {
        self.back_forward.requests.cancel();
    }

    /// Remembers where tab `id`'s list and engine reach, for Back and Forward's menus and looks.
    pub(super) fn history_reach_changed(
        &mut self,
        id: TabId,
        listed: HistoryReach,
        engine: HistoryReach,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.back_forward.reach.insert(id, Reach { listed, engine });
        // A reset the page did not cause by moving still leaves an open menu's rows gone.
        let relisted = before.is_none_or(|before| before.listed != listed);
        if self.shown != Some(id) || !relisted {
            return;
        }
        let stale = self.back_forward.requests.relisted();
        self.close_stale_history_menu(stale, window, cx);
    }

    /// Forgets the reach of tabs that are no longer open.
    pub(super) fn forget_closed_history(&mut self) {
        let tabs = &self.model.tabs;
        self.back_forward
            .reach
            .retain(|id, _| tabs.iter().any(|tab| tab.id == *id));
    }

    /// The open tab with the page id the engine uses.
    fn open_tab(&self, id: u64) -> Option<TabId> {
        self.model
            .tabs
            .iter()
            .find(|tab| tab.id.get() == id)
            .map(|tab| tab.id)
    }

    /// One Back or Forward on the shown page, from a button or a key, counted once (AN-01).
    pub(super) fn step_history(&mut self, direction: HistoryDirection) {
        if self.shown.is_none() {
            return;
        }
        self.count(way(direction).feature);
        self.go_history(direction, 1);
    }

    /// Tells the tab model where `steps` Back or Forward calls should land, so a wrong
    /// landing resets its list, then sends them in one go. Both go in order through the
    /// worker's queue, ahead of the landing's report. A list with nothing that way plans no
    /// jump, but the engine still steps, so an unknown list never stops Back working.
    fn go_history(&mut self, direction: HistoryDirection, steps: u32) {
        let Some(shown) = self.shown else {
            return;
        };
        if self.history_list_available(direction) {
            self.command(SessionCommand::StartHistoryJump {
                id: shown,
                direction,
                steps,
            });
        }
        self.page_command(BrowserCommand::GoToOffset {
            id: shown.get(),
            direction,
            steps,
        });
    }

    /// Whether the shown tab's list has a row in `direction`. The reach can lag the worker by
    /// one report: a Back pressed just after a commit plans no jump and lands by address
    /// instead, which is harmless.
    fn history_list_available(&self, direction: HistoryDirection) -> bool {
        goes(self.shown_reach().listed, direction)
    }

    /// Whether the shown tab's engine can go `direction`; Back and Forward look disabled when
    /// it cannot. A tab with no reports yet can go neither way.
    fn history_engine_available(&self, direction: HistoryDirection) -> bool {
        goes(self.shown_reach().engine, direction)
    }

    /// Where the shown tab's list and engine reach; nowhere for a tab with no reports.
    fn shown_reach(&self) -> Reach {
        self.shown
            .and_then(|shown| self.back_forward.reach.get(&shown))
            .copied()
            .unwrap_or_default()
    }

    /// Asks the worker for the shown tab's rows in `direction`, or closes that menu when it is
    /// open. True when this took the press, so it is neither a click nor a right-click for ☰.
    fn request_history_list(
        &mut self,
        direction: HistoryDirection,
        keyed: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.shown else {
            return false;
        };
        if !self.history_menu_offered(direction) {
            return false;
        }
        if self.toggled_closed(MenuKind::History(direction), cx) {
            return true;
        }
        let Press::Ask(id) = self.back_forward.requests.press(tab, direction, keyed) else {
            return true; // A second press before the rows arrived: nothing opens.
        };
        let sent = self.try_command(SessionCommand::HistoryRows {
            request_id: id,
            id: tab,
            direction,
        });
        if !sent {
            self.back_forward.requests.unsent(id);
        }
        true
    }

    /// Whether `direction`'s menu has anything to show: rows, or the note a reset list shows
    /// while the engine can still go that way.
    fn history_menu_offered(&self, direction: HistoryDirection) -> bool {
        self.history_list_available(direction) || self.history_engine_available(direction)
    }

    /// Opens the menu the rows were read for, if it is still wanted: Syrinx is in front, the
    /// same tab is shown, and its button was drawn in the main window. Another menu, the
    /// palette or a move of the page cancels the request first (`cancel_history_request`).
    pub(super) fn history_rows_arrived(
        &mut self,
        request_id: u64,
        rows: &[HistoryListRow],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self.back_forward.requests.arrived(request_id) else {
            return; // A later press, menu or move of the page replaced or cancelled it.
        };
        // A reply that waited behind slow work must not take the keys from another program.
        if self.shown != Some(request.tab) || !syrinx_in_foreground() {
            return;
        }
        // The note is true only while the engine can still go that way.
        if rows.is_empty() && !self.history_engine_available(request.direction) {
            return;
        }
        let Some(anchor) = self.history_anchor(request.direction).get() else {
            return; // The button was never drawn, so there is nowhere to hang the menu.
        };
        self.show_history_menu(
            (request.tab, request.direction),
            rows,
            anchor,
            request.keyed,
            window,
            cx,
        );
    }

    /// Jumps the tab the menu opened on straight to a row. A tab switched away since is left
    /// alone; choosing a row counts no new feature (D39).
    pub(super) fn jump_to_history_row(
        &mut self,
        tab: TabId,
        direction: HistoryDirection,
        steps: usize,
    ) {
        if self.shown != Some(tab) {
            return;
        }
        // A list shows at most fifteen rows, so a count past `u32` is never a row.
        let Ok(steps) = u32::try_from(steps) else {
            return;
        };
        self.go_history(direction, steps);
    }

    /// Where `direction`'s button was last drawn, which its menu hangs from.
    fn history_anchor(&self, direction: HistoryDirection) -> &Anchor {
        match direction {
            HistoryDirection::Back => &self.back_forward.back_anchor,
            HistoryDirection::Forward => &self.back_forward.forward_anchor,
        }
    }

    /// Starts timing a press on Back or Forward; a press held for `HOLD` opens the list.
    fn hold_started(&mut self, direction: HistoryDirection, cx: &mut Context<Self>) {
        self.back_forward.hold.press(direction);
        self.back_forward.hold_timer = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(HOLD).await;
            // Fails only when the view closed with its window: no press is left to finish.
            view.update(cx, |view, cx| view.hold_elapsed(direction, cx))
        }));
    }

    /// Opens the list if the press is still down. With nothing to list, the release stays an
    /// ordinary click, so a slow click still steps.
    fn hold_elapsed(&mut self, direction: HistoryDirection, cx: &mut Context<Self>) {
        if !self.back_forward.hold.elapse(direction) {
            return;
        }
        if self.request_history_list(direction, false, cx) {
            self.back_forward.hold.open(direction);
        }
    }

    /// A release reported by `direction`'s button. Its own release drops the hold timer, so
    /// an ordinary click wakes nothing half a second later.
    fn hold_released(&mut self, direction: HistoryDirection, on_button: bool) {
        if self.back_forward.hold.pressed == Some(direction) {
            self.back_forward.hold_timer = None;
        }
        self.back_forward.hold.release(direction, on_button);
    }

    /// A click on Back or Forward steps once, unless its press was held open into the list.
    fn history_button_clicked(&mut self, direction: HistoryDirection, cx: &mut Context<Self>) {
        if !self.back_forward.hold.click(direction) {
            return;
        }
        // A click on the button of the open menu only closes it, as in Chrome.
        if self.toggled_closed(MenuKind::History(direction), cx) {
            return;
        }
        self.step_history(direction);
        cx.notify();
    }

    /// Back or Forward's own listeners (click, right-click, hold and Alt Down), where its menu
    /// hangs, and its dimmed look when the engine cannot go that way.
    pub(super) fn history_button(
        &self,
        button: gpui::Stateful<gpui::Div>,
        direction: HistoryDirection,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let unavailable = !self.history_engine_available(direction);
        let expanded = self.open_menu_kind() == Some(MenuKind::History(direction));
        button
            .child(self.anchor_probe(direction))
            .aria_expanded(expanded)
            // Dimmed to 3:1 (owner, 2026-10-28), but it keeps its hover and a click still reaches
            // the engine: the reach can lag a fresh page. (`icon_button` sets the hover; GPUI
            // allows only one.)
            .when(unavailable, |button| {
                button
                    .text_color(theme::unavailable_icon())
                    .aria_description(way(direction).unavailable)
            })
            .key_context(way(direction).key_context)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.history_button_clicked(direction, cx);
            }))
            // On the button itself, and never passed on to the group's page menu (owner,
            // 2026-10-28), even when there is nothing to list.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    let offered = this.history_menu_offered(direction);
                    if RightClick::on(offered) == RightClick::OpenMenu {
                        this.request_history_list(direction, false, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.hold_started(direction, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, _| this.hold_released(direction, true)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |this, _, _, _| this.hold_released(direction, false)),
            )
            .on_action(cx.listener(|this, _: &OpenBackList, _, cx| {
                this.request_history_list(HistoryDirection::Back, true, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenForwardList, _, cx| {
                this.request_history_list(HistoryDirection::Forward, true, cx);
            }))
    }

    /// An invisible layer over the button that records its bottom-left, where its menu hangs.
    /// Only the main window's paint counts: menus open only there.
    fn anchor_probe(&self, direction: HistoryDirection) -> impl gpui::IntoElement {
        let anchor = self.history_anchor(direction).clone();
        let main = self.main_window;
        canvas(
            move |bounds, window, _| {
                if window.window_handle() == main {
                    anchor.set(Some(bounds.bottom_left()));
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACK: HistoryDirection = HistoryDirection::Back;
    const FORWARD: HistoryDirection = HistoryDirection::Forward;

    #[test]
    fn a_right_click_on_a_dimmed_button_opens_nothing_and_never_reaches_the_page_menu() {
        // The owner, 2026-10-28: a dimmed Back or Forward keeps its right-click from the page
        // menu. `RightClick` has no variant that passes it on, and the handler always stops it.
        assert_eq!(RightClick::on(false), RightClick::Nothing);
        assert_eq!(RightClick::on(true), RightClick::OpenMenu);
    }

    #[test]
    fn a_held_press_released_on_its_button_does_not_also_step() {
        // Review B1: GPUI runs Forward's mouse-up-outside on a release over Back.
        let mut hold = HoldState::default();
        hold.press(BACK);
        assert!(hold.elapse(BACK));
        hold.open(BACK);
        hold.release(FORWARD, false);
        hold.release(BACK, true);
        assert!(!hold.click(BACK));
    }

    #[test]
    fn a_short_press_steps() {
        let mut hold = HoldState::default();
        hold.press(BACK);
        hold.release(FORWARD, false);
        hold.release(BACK, true);
        assert!(
            !hold.elapse(BACK),
            "the timer finds the press already ended"
        );
        assert!(hold.click(BACK));
    }

    #[test]
    fn a_held_press_released_off_its_button_does_not_swallow_the_next_click() {
        let mut hold = HoldState::default();
        hold.press(BACK);
        assert!(hold.elapse(BACK));
        hold.open(BACK);
        hold.release(BACK, false);
        assert!(hold.click(BACK), "Enter on the focused button still steps");
    }

    #[test]
    fn a_held_press_with_nothing_to_list_still_steps() {
        let mut hold = HoldState::default();
        hold.press(FORWARD);
        assert!(hold.elapse(FORWARD));
        hold.release(FORWARD, true);
        assert!(hold.click(FORWARD));
    }

    // Which history changes close an open menu is tested with the request bookkeeping, in
    // `requests.rs` (`a_move_of_the_shown_page_closes_the_menu_and_drops_its_rows` and
    // `a_single_page_sites_own_address_change_keeps_the_menu`).

    #[test]
    fn each_direction_counts_its_own_existing_feature_and_names_its_menu() {
        assert_eq!(way(BACK).feature, Feature::NavBack);
        assert_eq!(way(FORWARD).feature, Feature::NavForward);
        assert_eq!(way(BACK).menu_name, "Back history");
        assert_eq!(way(FORWARD).unlisted, "Later pages aren't listed");
    }
}

//! Routes Back, Forward and their list requests to the tab model's back and forward lists (#126).

use std::{collections::HashMap, time::Duration};

use gpui::{
    Context, InteractiveElement, KeyBinding, MouseButton, StatefulInteractiveElement, Task,
};
use syrinx_core::{HistoryDirection, HistoryReach, TabId, usage::Feature};
use syrinx_platform::browser::BrowserCommand;

use super::BrowserView;
use crate::session::SessionCommand;

gpui::actions!(back_forward, [OpenBackList, OpenForwardList]);

/// How long Back or Forward must be held before its list opens, as in Chrome.
const HOLD: Duration = Duration::from_millis(500);
/// The key contexts of the two buttons, where Alt Down opens their list (owner, 2026-10-28).
const BACK_CONTEXT: &str = "SyrinxBack";
const FORWARD_CONTEXT: &str = "SyrinxForward";

/// Binds Alt Down on a focused Back or Forward button.
pub(super) fn bind_keys(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("alt-down", OpenBackList, Some(BACK_CONTEXT)),
        KeyBinding::new("alt-down", OpenForwardList, Some(FORWARD_CONTEXT)),
    ]);
}

/// What the window keeps for Back and Forward: where each tab's list reaches, as the tab
/// model last reported, and the press being held. The lists themselves live in the model.
#[derive(Default)]
pub(super) struct BackForward {
    reach: HashMap<TabId, HistoryReach>,
    hold: HoldState,
    /// The held press's timer; its result is an error only once the view has closed.
    hold_timer: Option<Task<gpui::Result<()>>>,
}

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

/// The existing usage count each direction adds to (AN-01); the list itself counts nothing.
fn feature(direction: HistoryDirection) -> Feature {
    match direction {
        HistoryDirection::Back => Feature::NavBack,
        HistoryDirection::Forward => Feature::NavForward,
    }
}

impl BrowserView {
    /// Hands a page's history report to the tab model, which keeps the list (owner, 2026-10-28).
    pub(super) fn history_reported(&mut self, id: u64, report: syrinx_core::HistoryReport) {
        let Some(tab) = self.open_tab(id) else {
            return;
        };
        self.command(SessionCommand::ObserveHistory { id: tab, report });
    }

    /// Remembers where tab `id`'s list reaches, for Back and Forward's list requests.
    pub(super) fn history_reach_changed(&mut self, id: TabId, reach: HistoryReach) {
        self.back_forward.reach.insert(id, reach);
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
        self.count(feature(direction));
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

    /// Whether the shown tab's list has a row in `direction`, so its list may open.
    fn history_list_available(&self, direction: HistoryDirection) -> bool {
        let Some(reach) = self
            .shown
            .and_then(|shown| self.back_forward.reach.get(&shown))
        else {
            return false;
        };
        match direction {
            HistoryDirection::Back => reach.back,
            HistoryDirection::Forward => reach.forward,
        }
    }

    /// Asks for the shown tab's list in `direction`; true when there is one to show.
    fn request_history_list(&mut self, direction: HistoryDirection) -> bool {
        // TODO(#126): the interface slice opens the menu here, in an owned popup.
        self.history_list_available(direction)
    }

    /// Starts timing a press on Back or Forward; a press held for `HOLD` opens the list.
    fn hold_started(&mut self, direction: HistoryDirection, cx: &mut Context<Self>) {
        self.back_forward.hold.press(direction);
        self.back_forward.hold_timer = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(HOLD).await;
            // Fails only when the view closed with its window: no press is left to finish.
            view.update(cx, |view, _| view.hold_elapsed(direction))
        }));
    }

    /// Opens the list if the press is still down. With nothing to list, the release stays an
    /// ordinary click, so a slow click still steps.
    fn hold_elapsed(&mut self, direction: HistoryDirection) {
        if !self.back_forward.hold.elapse(direction) {
            return;
        }
        if self.request_history_list(direction) {
            self.back_forward.hold.open(direction);
        }
    }

    /// A click on Back or Forward steps once, unless its press was held open into the list.
    fn history_button_clicked(&mut self, direction: HistoryDirection, cx: &mut Context<Self>) {
        if !self.back_forward.hold.click(direction) {
            return;
        }
        self.step_history(direction);
        cx.notify();
    }

    /// Back or Forward's own listeners: click, right-click, hold and Alt Down.
    pub(super) fn history_button(
        &self,
        button: gpui::Stateful<gpui::Div>,
        direction: HistoryDirection,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        button
            .key_context(match direction {
                HistoryDirection::Back => BACK_CONTEXT,
                HistoryDirection::Forward => FORWARD_CONTEXT,
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.history_button_clicked(direction, cx);
            }))
            // On the button itself; with no list to open, the right-click goes on to ☰.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    if this.request_history_list(direction) {
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.hold_started(direction, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, _| this.back_forward.hold.release(direction, true)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |this, _, _, _| this.back_forward.hold.release(direction, false)),
            )
            .on_action(cx.listener(|this, _: &OpenBackList, _, _| {
                this.request_history_list(HistoryDirection::Back);
            }))
            .on_action(cx.listener(|this, _: &OpenForwardList, _, _| {
                this.request_history_list(HistoryDirection::Forward);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACK: HistoryDirection = HistoryDirection::Back;
    const FORWARD: HistoryDirection = HistoryDirection::Forward;

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

    #[test]
    fn each_direction_counts_its_own_existing_feature() {
        assert_eq!(feature(BACK), Feature::NavBack);
        assert_eq!(feature(FORWARD), Feature::NavForward);
    }
}

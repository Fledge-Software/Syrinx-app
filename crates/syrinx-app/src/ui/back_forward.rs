//! Keeps each tab's back and forward list and routes Back, Forward and their list requests (#126).

use std::time::Duration;

use gpui::{
    Context, InteractiveElement, KeyBinding, MouseButton, StatefulInteractiveElement, Task,
};
use syrinx_core::{HistoryDirection, HistoryReport, NavigationLists, TabId, usage::Feature};
use syrinx_platform::browser::BrowserCommand;

use super::BrowserView;

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

/// Every tab's list, the list request waiting to be drawn, and a press being held.
#[derive(Default)]
pub(super) struct BackForward {
    lists: NavigationLists,
    /// The tab and direction whose list was asked for, until the next step or tab change.
    requested: Option<(TabId, HistoryDirection)>,
    /// The held press's timer; its result is an error only once the view has closed.
    hold: Option<Task<gpui::Result<()>>>,
    /// A held press opened the list, so its release is not also a click.
    hold_opened: bool,
}

/// The engine's signed step count for `steps` rows in `direction`.
fn offset(direction: HistoryDirection, steps: usize) -> i32 {
    let steps = i32::try_from(steps).unwrap_or(i32::MAX);
    match direction {
        HistoryDirection::Back => -steps,
        HistoryDirection::Forward => steps,
    }
}

fn feature(direction: HistoryDirection) -> Feature {
    match direction {
        HistoryDirection::Back => Feature::NavBack,
        HistoryDirection::Forward => Feature::NavForward,
    }
}

impl BrowserView {
    /// Follows a page's history report; the list never leaves memory (owner, 2026-10-28).
    pub(super) fn history_reported(&mut self, id: u64, report: &HistoryReport) {
        let Some(tab) = self.open_tab(id) else {
            return;
        };
        self.back_forward.lists.observe(tab, report);
    }

    /// Names the current row of the page's list.
    pub(super) fn history_retitled(&mut self, id: u64, title: &str) {
        if let Some(tab) = self.open_tab(id) {
            self.back_forward.lists.retitle(tab, title);
        }
    }

    /// Drops the lists of tabs that are no longer open, and a request for a tab not shown.
    pub(super) fn forget_closed_history(&mut self) {
        let tabs = &self.model.tabs;
        self.back_forward
            .lists
            .retain(|id| tabs.iter().any(|tab| tab.id == id));
        if self
            .back_forward
            .requested
            .is_some_and(|(tab, _)| Some(tab) != self.shown)
        {
            self.back_forward.requested = None;
        }
    }

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

    /// Sends `steps` Back or Forward calls in one go and tells the list where they should land,
    /// so a wrong landing resets it. An unknown list still lets the engine step.
    fn go_history(&mut self, direction: HistoryDirection, steps: usize) {
        let Some(shown) = self.shown else {
            return;
        };
        self.back_forward.requested = None;
        if let Some(list) = self.back_forward.lists.get_mut(shown) {
            list.start_jump(direction, steps);
        }
        self.page_command(BrowserCommand::GoToOffset {
            id: shown.get(),
            offset: offset(direction, steps),
        });
    }

    /// Asks for the shown tab's list in `direction`; nothing opens when it has no rows.
    fn request_history_list(&mut self, direction: HistoryDirection, cx: &mut Context<Self>) {
        let Some(shown) = self.shown else {
            return;
        };
        let has_rows = self
            .back_forward
            .lists
            .get(shown)
            .is_some_and(|list| !list.rows(direction, 1).is_empty());
        if !has_rows {
            return;
        }
        // TODO(#126): the interface slice draws this request as a menu in an owned popup.
        self.back_forward.requested = Some((shown, direction));
        cx.notify();
    }

    /// Starts timing a press on Back or Forward; a press held for `HOLD` opens the list.
    fn hold_started(&mut self, direction: HistoryDirection, cx: &mut Context<Self>) {
        self.back_forward.hold_opened = false;
        self.back_forward.hold = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(HOLD).await;
            // Fails only when the view closed with its window: no press is left to finish.
            view.update(cx, |view, cx| view.hold_elapsed(direction, cx))
        }));
    }

    fn hold_elapsed(&mut self, direction: HistoryDirection, cx: &mut Context<Self>) {
        if self.back_forward.hold.take().is_none() {
            return;
        }
        self.back_forward.hold_opened = true;
        self.request_history_list(direction, cx);
    }

    /// A release ends the timing; dropping the task cancels it.
    fn hold_ended(&mut self) {
        self.back_forward.hold = None;
    }

    /// A click on Back or Forward steps once, unless its press was held open into the list.
    fn history_button_clicked(&mut self, direction: HistoryDirection, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.back_forward.hold_opened) {
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
        let context = match direction {
            HistoryDirection::Back => BACK_CONTEXT,
            HistoryDirection::Forward => FORWARD_CONTEXT,
        };
        button
            .key_context(context)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.history_button_clicked(direction, cx);
            }))
            // On the button itself, so the group's ☰ menu keeps every other right-click.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.request_history_list(direction, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.hold_started(direction, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.hold_ended()),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.hold_ended()),
            )
            .on_action(cx.listener(|this, _: &OpenBackList, _, cx| {
                this.request_history_list(HistoryDirection::Back, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenForwardList, _, cx| {
                this.request_history_list(HistoryDirection::Forward, cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_steps_are_negative_and_forward_steps_positive() {
        assert_eq!(offset(HistoryDirection::Back, 3), -3);
        assert_eq!(offset(HistoryDirection::Forward, 2), 2);
        assert_eq!(offset(HistoryDirection::Back, usize::MAX), -i32::MAX);
    }

    #[test]
    fn each_direction_counts_its_own_existing_feature() {
        assert_eq!(feature(HistoryDirection::Back), Feature::NavBack);
        assert_eq!(feature(HistoryDirection::Forward), Feature::NavForward);
    }
}

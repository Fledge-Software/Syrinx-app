//! Customize's Usage report section and the window's own usage measurements (AN-01).
//!
//! Counting is off until the owner switches it on; turning it off deletes the counts.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Role, Stateful,
    StatefulInteractiveElement, Styled, div, px,
};
use syrinx_core::TabId;
use syrinx_core::usage::{Feature, Timing, UsageRecorder};
use syrinx_ui::Palette;
use syrinx_ui::metrics::text;

use super::BrowserView;
use super::customize::switch_control;
use super::updates::pill;
use crate::controls::control;
use crate::session::{SessionCommand, SessionError, UsageControl, UsageStatus};
use crate::theme::rgba;

/// The switch's approved wording (`docs/proposals/an-01-02-local-analytics.md`).
const SWITCH_LABEL: &str = "Count how I use Syrinx, on this computer only";
/// Shortened by the owner (2026-10-14) until AN-02 adds viewing and exporting; AN-02 restores
/// "Nothing is sent anywhere. You can view, export or delete the counts at any time."
const SWITCH_NOTE: &str = "Nothing is sent anywhere. You can delete the counts at any time.";

/// The step that waits for the owner to confirm.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Confirming {
    TurnOff,
    Reset,
}

/// A tab selection being timed until its page is shown.
#[derive(Clone, Copy, Debug)]
struct PendingSwitch {
    started: Instant,
    /// The tab that was active when the owner asked.
    from: Option<TabId>,
    /// The tab to be shown; learned from the next state when the request could not say.
    target: Option<TabId>,
}

/// The window's usage state: the shared counters and what the section shows.
pub(super) struct UsageState {
    pub(super) recorder: Arc<UsageRecorder>,
    /// `None` until the session reports it.
    status: Option<UsageStatus>,
    confirming: Option<Confirming>,
    /// A failed control, shown under the switch.
    problem: Option<String>,
    /// When `main` started, for the two start-up timings.
    launched: Instant,
    first_frame: Option<Duration>,
    session_loaded: bool,
    /// Set once recorded, or when start-up waited on the owner and is not worth timing.
    startup_done: bool,
    switch: Option<PendingSwitch>,
}

impl UsageState {
    pub(super) fn new(recorder: Arc<UsageRecorder>, launched: Instant) -> Self {
        Self {
            recorder,
            status: None,
            confirming: None,
            problem: None,
            launched,
            first_frame: None,
            session_loaded: false,
            startup_done: false,
            switch: None,
        }
    }

    /// Notes the first frame; recorded once the session says whether counting is on.
    pub(super) fn frame_drawn(&mut self) {
        if self.first_frame.is_none() {
            self.first_frame = Some(self.launched.elapsed());
            self.record_startup();
        }
    }

    /// A recovery or repair prompt waited on the owner, so this start-up is not timed.
    pub(super) fn startup_waited(&mut self) {
        self.startup_done = true;
    }

    /// The restored session is listed: the start-up timings can be recorded.
    pub(super) fn session_restored(&mut self) {
        if self.session_loaded {
            return;
        }
        self.session_loaded = true;
        self.record_startup();
    }

    /// The worker loads the usage file before the first state, so the recorder is ready here.
    fn record_startup(&mut self) {
        let Some(first_frame) = self.first_frame else {
            return;
        };
        if !self.session_loaded || self.startup_done {
            return;
        }
        self.startup_done = true;
        self.recorder.time(Timing::StartupFirstFrame, first_frame);
        self.recorder
            .time(Timing::StartupSessionRestored, self.launched.elapsed());
    }

    /// Starts timing a selection away from `from`; the clock is read only while counting is on.
    pub(super) fn switch_requested(&mut self, from: Option<TabId>, target: Option<TabId>) {
        self.switch = self.recorder.is_enabled().then(|| PendingSwitch {
            started: Instant::now(),
            from,
            target,
        });
    }

    /// A state arrived: a selection that changed nothing, or ended on Home, has nothing to
    /// time; otherwise the active tab is the one to wait for.
    pub(super) fn state_arrived(&mut self, active: Option<TabId>, home: bool) {
        let Some(switch) = &mut self.switch else {
            return;
        };
        if home || active == switch.from {
            self.switch = None;
            return;
        }
        switch.target = active;
    }

    /// A page was shown; only the selected tab's page completes the selection.
    pub(super) fn page_shown(&mut self, id: TabId) {
        let Some(switch) = self.switch else {
            return;
        };
        if switch.target != Some(id) {
            return;
        }
        self.switch = None;
        self.recorder
            .time(Timing::TabSwitchToVisible, switch.started.elapsed());
    }
}

impl BrowserView {
    pub(super) fn count(&self, feature: Feature) {
        self.usage.recorder.count(feature);
    }

    /// The session's answer at start-up and after each control: always the real status.
    pub(super) fn usage_status_received(
        &mut self,
        status: UsageStatus,
        problem: Option<SessionError>,
    ) {
        self.usage.confirming = None;
        self.usage.status = Some(status);
        self.usage.problem = problem.map(|problem| problem.to_string());
        if let Some(problem) = &self.usage.problem {
            self.message = problem.clone();
        }
    }

    fn usage_control(&mut self, control: UsageControl) {
        self.usage.confirming = None;
        self.command(SessionCommand::Usage(control));
    }

    /// Customize's Usage report section: the switch, the period and Reset.
    pub(super) fn usage_section(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let counting = self.usage.status.is_some_and(|status| status.counting);
        let mut section = div()
            .pt(px(14.))
            .border_t_1()
            .border_color(rgba(palette.panel_divider))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(switch_text(palette))
                    .child(self.usage_switch(palette, counting, cx)),
            );
        if let Some(since) = self.usage.status.and_then(|status| status.since) {
            section = section.child(self.period_row(palette, &since.to_string(), cx));
        }
        if let Some(confirming) = self.usage.confirming {
            section = section.child(self.confirm_row(palette, confirming, cx));
        }
        if let Some(problem) = &self.usage.problem {
            section = section.child(
                div()
                    .id("usage-problem")
                    .role(Role::Label)
                    .aria_label(problem.clone())
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_secondary))
                    .child(problem.clone()),
            );
        }
        section
    }

    /// On asks nothing; off asks first, because it deletes the counts.
    fn usage_switch(&self, palette: &Palette, on: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let known = self.usage.status.is_some();
        switch_control("usage-counting", SWITCH_LABEL, on, palette).on_click(cx.listener(
            move |this, _, _, cx| {
                // Until the session reports, the switch's state is unknown and does nothing.
                if !known {
                    return;
                }
                if on {
                    this.usage.confirming = Some(Confirming::TurnOff);
                } else {
                    this.usage_control(UsageControl::TurnOn);
                }
                cx.notify();
            },
        ))
    }

    fn period_row(&self, palette: &Palette, since: &str, cx: &mut Context<Self>) -> Div {
        let reset = pill(
            palette,
            control(div().id("usage-reset"), Role::Button, "Reset usage counts"),
        )
        .child("Reset")
        .on_click(cx.listener(|this, _, _, cx| {
            this.usage.confirming = Some(Confirming::Reset);
            cx.notify();
        }));
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_muted))
                    .child(format!("Counting since {since}. Periods last 30 days.")),
            )
            .child(reset)
    }

    fn confirm_row(
        &self,
        palette: &Palette,
        confirming: Confirming,
        cx: &mut Context<Self>,
    ) -> Div {
        let (question, action, control_choice) = match confirming {
            Confirming::TurnOff => (
                "Turn off counting? This deletes all usage counts on this computer.",
                "Turn off and delete",
                UsageControl::TurnOff,
            ),
            Confirming::Reset => (
                "Reset? This deletes all usage counts and starts a new period today.",
                "Reset counts",
                UsageControl::Reset,
            ),
        };
        let confirm = pill(
            palette,
            control(div().id("usage-confirm"), Role::Button, action),
        )
        .child(action)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.usage_control(control_choice);
            cx.notify();
        }));
        let cancel = pill(
            palette,
            control(div().id("usage-cancel"), Role::Button, "Cancel"),
        )
        .child("Cancel")
        .on_click(cx.listener(|this, _, _, cx| {
            this.usage.confirming = None;
            cx.notify();
        }));
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_secondary))
                    .child(question),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(confirm)
                    .child(cancel),
            )
    }
}

fn switch_text(palette: &Palette) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().font_weight(FontWeight::MEDIUM).child("Usage report"))
        .child(
            div()
                .text_size(px(text::SMALL))
                .text_color(rgba(palette.text_secondary))
                .child(SWITCH_LABEL),
        )
        .child(
            div()
                .text_size(px(text::SMALL))
                .text_color(rgba(palette.text_muted))
                .child(SWITCH_NOTE),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64) -> TabId {
        TabId::new(id).expect("synthetic tab id")
    }

    fn counting() -> UsageState {
        let recorder = Arc::new(UsageRecorder::new());
        recorder.switch_on();
        UsageState::new(recorder, Instant::now())
    }

    fn timed(state: &UsageState) -> u32 {
        let counts = state.recorder.take().unwrap_or_default();
        counts.timing(Timing::TabSwitchToVisible).iter().sum()
    }

    #[test]
    fn only_the_selected_tab_ends_a_switch() {
        let mut state = counting();
        state.switch_requested(Some(tab(1)), Some(tab(2)));
        // Another page appearing first, such as a popup, is not the selection.
        state.page_shown(tab(3));
        assert_eq!(timed(&state), 0);
        state.state_arrived(Some(tab(2)), false);
        state.page_shown(tab(2));
        assert_eq!(timed(&state), 1);
        // The selection is complete; showing the page again records nothing.
        state.page_shown(tab(2));
        assert_eq!(timed(&state), 0);
    }

    #[test]
    fn a_switch_learns_its_target_from_the_state() {
        let mut state = counting();
        state.switch_requested(Some(tab(1)), None);
        state.state_arrived(Some(tab(4)), false);
        state.page_shown(tab(4));
        assert_eq!(timed(&state), 1);
    }

    #[test]
    fn a_switch_that_changes_nothing_or_ends_on_home_is_dropped() {
        let mut state = counting();
        // Ctrl Tab with a single tab: the state keeps the same active tab.
        state.switch_requested(Some(tab(1)), None);
        state.state_arrived(Some(tab(1)), false);
        state.page_shown(tab(1));
        assert_eq!(timed(&state), 0);

        state.switch_requested(Some(tab(1)), Some(tab(5)));
        state.state_arrived(Some(tab(5)), true);
        state.page_shown(tab(5));
        assert_eq!(timed(&state), 0);
    }

    #[test]
    fn nothing_is_timed_while_counting_is_off() {
        let mut state = UsageState::new(Arc::new(UsageRecorder::new()), Instant::now());
        state.switch_requested(Some(tab(1)), Some(tab(2)));
        assert!(state.switch.is_none(), "no clock is read while off");
    }

    #[test]
    fn a_start_that_waited_on_the_owner_is_not_timed() {
        let mut state = counting();
        state.startup_waited();
        state.frame_drawn();
        state.session_restored();
        let counts = state.recorder.take().unwrap_or_default();
        assert_eq!(
            counts.timing(Timing::StartupFirstFrame).iter().sum::<u32>(),
            0
        );
        assert_eq!(
            counts
                .timing(Timing::StartupSessionRestored)
                .iter()
                .sum::<u32>(),
            0
        );

        let mut state = counting();
        state.frame_drawn();
        state.session_restored();
        let counts = state.recorder.take().unwrap_or_default();
        assert_eq!(
            counts.timing(Timing::StartupFirstFrame).iter().sum::<u32>(),
            1
        );
        assert_eq!(
            counts
                .timing(Timing::StartupSessionRestored)
                .iter()
                .sum::<u32>(),
            1
        );
    }
}

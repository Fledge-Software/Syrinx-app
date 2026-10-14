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
const SWITCH_NOTE: &str =
    "Nothing is sent anywhere. You can view, export or delete the counts at any time.";

/// The step that waits for the owner to confirm.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Confirming {
    TurnOff,
    Reset,
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
    startup_recorded: bool,
    /// When the owner asked for another tab, until that tab's page is shown.
    switch_started: Option<Instant>,
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
            startup_recorded: false,
            switch_started: None,
        }
    }

    /// Notes the first frame; recorded once the session says whether counting is on.
    pub(super) fn frame_drawn(&mut self) {
        if self.first_frame.is_none() {
            self.first_frame = Some(self.launched.elapsed());
            self.record_startup();
        }
    }

    /// The restored session is listed: the second start-up timing ends here.
    pub(super) fn session_restored(&mut self) {
        if self.session_loaded {
            return;
        }
        self.session_loaded = true;
        self.recorder
            .time(Timing::StartupSessionRestored, self.launched.elapsed());
        self.record_startup();
    }

    /// The worker loads the usage file before the first state, so the recorder is ready here.
    fn record_startup(&mut self) {
        let Some(first_frame) = self.first_frame else {
            return;
        };
        if !self.session_loaded || self.startup_recorded {
            return;
        }
        self.startup_recorded = true;
        self.recorder.time(Timing::StartupFirstFrame, first_frame);
    }

    /// Starts timing a tab selection; the clock is read only while counting is on.
    pub(super) fn switch_requested(&mut self) {
        self.switch_started = self.recorder.is_enabled().then(Instant::now);
    }

    /// A selection that ends on Home shows no page, so it has nothing to time.
    pub(super) fn switch_abandoned(&mut self) {
        self.switch_started = None;
    }

    /// A page was shown: the selection that asked for it is complete.
    pub(super) fn page_shown(&mut self) {
        if let Some(started) = self.switch_started.take() {
            self.recorder
                .time(Timing::TabSwitchToVisible, started.elapsed());
        }
    }
}

impl BrowserView {
    pub(super) fn count(&self, feature: Feature) {
        self.usage.recorder.count(feature);
    }

    /// The session's answer: at start-up, and after each switch or reset.
    pub(super) fn usage_status_received(&mut self, result: Result<UsageStatus, SessionError>) {
        self.usage.confirming = None;
        match result {
            Ok(status) => {
                self.usage.status = Some(status);
                self.usage.problem = None;
            }
            Err(error) => self.usage.problem = Some(error.to_string()),
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

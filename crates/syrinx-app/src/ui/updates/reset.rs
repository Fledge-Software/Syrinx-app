//! "Reset update records" (DW-66): shown only while `Updates\state` cannot be read, it explains
//! what it forgets, asks for a yes, then sets the record aside on its own thread.

use std::thread;

use futures::channel::oneshot;
use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Role, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use syrinx_ui::Palette;
use syrinx_ui::metrics::text;
use syrinx_update::install::{InstallError, StateReset};

use super::{BrowserView, Stage, UpdatePanel, pill};
use crate::controls::control;
use crate::theme::rgba;
use crate::updates::{RECORDS_UNREADABLE, reset_explanation, reset_message};

impl UpdatePanel {
    /// The owner pressed "Reset update records…": show the explanation, or say why not now.
    fn ask_to_reset(&mut self) -> Option<String> {
        if !self.records_unreadable {
            return None;
        }
        if let Some(refusal) = self.reset_refusal() {
            return Some(refusal);
        }
        self.confirming_reset = true;
        None
    }

    /// Why a reset cannot start now: an update step is running, or a staged update waits.
    fn reset_refusal(&self) -> Option<String> {
        match self.stage {
            Stage::Idle => None,
            Stage::Ready(_) | Stage::InstallOnQuit(_) => {
                Some(reset_message(&Err(InstallError::UpdateWaiting)))
            }
            _ => Some(reset_message(&Err(InstallError::Busy))),
        }
    }

    /// The owner confirmed: `Ok` when the reset may start on its thread.
    fn begin_reset(&mut self) -> Result<(), String> {
        let confirmed = std::mem::take(&mut self.confirming_reset);
        if !confirmed || !self.records_unreadable {
            return Err("Nothing was reset.".into());
        }
        if let Some(refusal) = self.reset_refusal() {
            return Err(refusal);
        }
        self.stage = Stage::ResettingRecords;
        Ok(())
    }

    /// Apply the reset's result; returns what to tell the owner.
    fn finish_reset(&mut self, result: Option<Result<StateReset, InstallError>>) -> String {
        if self.stage == Stage::ResettingRecords {
            self.stage = Stage::Idle;
        }
        let Some(result) = result else {
            return "Resetting the update records stopped unexpectedly.".into();
        };
        if result.is_ok() {
            self.records_unreadable = false;
        }
        reset_message(&result)
    }
}

impl BrowserView {
    fn ask_to_reset_update_records(&mut self) {
        if let Some(refusal) = self.updates.ask_to_reset() {
            self.show_update_status(refusal);
        }
    }

    /// Set the unreadable record aside on its own thread, holding the update lock there.
    fn reset_update_records(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        if let Err(refusal) = self.updates.begin_reset() {
            self.show_update_status(refusal);
            return;
        }
        let (sender, receiver) = oneshot::channel();
        let context = self.updates.context.clone();
        let spawned = thread::Builder::new()
            .name("syrinx-update-reset".to_owned())
            .spawn(move || {
                if sender.send(context.reset_records()).is_err() {
                    eprintln!("The update records were reset after their window closed.");
                }
            });
        if spawned.is_err() {
            let status = self.updates.finish_reset(None);
            self.show_update_status(status);
            return;
        }
        cx.spawn_in(window, async move |view, cx| {
            let result = receiver.await.ok();
            if view
                .update(cx, |view, cx| {
                    if let Some(Err(error)) = &result {
                        eprintln!("update records reset refused: {error}");
                    }
                    let status = view.updates.finish_reset(result);
                    view.show_update_status(status);
                    cx.notify();
                })
                .is_err()
            {
                eprintln!("The update records reset finished after the browser window closed.");
            }
        })
        .detach();
    }

    /// The unreadable-records line and its button, or the confirmation; `None` otherwise.
    pub(super) fn reset_row(&self, palette: &Palette, cx: &mut Context<Self>) -> Option<Div> {
        if !self.updates.records_unreadable {
            return None;
        }
        if self.updates.confirming_reset {
            return Some(self.reset_confirmation(palette, cx));
        }
        let ask = pill(
            palette,
            control(
                div().id("updates-reset-records"),
                Role::Button,
                "Reset update records",
            ),
        )
        .child("Reset update records…")
        .on_click(cx.listener(|this, _, _, cx| {
            this.ask_to_reset_update_records();
            cx.notify();
        }));
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(row_text(RECORDS_UNREADABLE.to_owned()))
                .child(ask),
        )
    }

    fn reset_confirmation(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let reset = pill(
            palette,
            control(
                div().id("updates-reset-confirm"),
                Role::Button,
                "Reset update records now",
            ),
        )
        .child("Reset")
        .on_click(cx.listener(|this, _, window, cx| {
            this.reset_update_records(window, cx);
            cx.notify();
        }));
        let keep = pill(
            palette,
            control(
                div().id("updates-reset-cancel"),
                Role::Button,
                "Keep the update records",
            ),
        )
        .child("Cancel")
        .on_click(cx.listener(|this, _, _, cx| {
            this.updates.confirming_reset = false;
            cx.notify();
        }));
        let explanation = reset_explanation();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(row_text(RECORDS_UNREADABLE.to_owned()))
            .child(
                div()
                    .id("updates-reset-explanation")
                    .role(Role::Label)
                    .aria_label(explanation.clone())
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_secondary))
                    .child(explanation),
            )
            .child(div().flex().gap(px(8.)).child(reset).child(keep))
    }
}

fn row_text(text: String) -> Stateful<Div> {
    div()
        .id("updates-records-unreadable")
        .flex_1()
        .min_w_0()
        .font_weight(FontWeight::MEDIUM)
        .child(text)
}

#[cfg(test)]
mod tests {
    use syrinx_update::Version;

    use super::*;
    use crate::updates::{StartupUpdates, UpdateContext};

    fn panel(records_unreadable: bool) -> UpdatePanel {
        let startup = StartupUpdates {
            records_unreadable,
            ..StartupUpdates::default()
        };
        UpdatePanel::new(UpdateContext::disabled(), startup)
    }

    #[test]
    fn a_readable_record_is_never_offered_a_reset() {
        let mut panel = panel(false);
        assert_eq!(panel.ask_to_reset(), None);
        assert!(!panel.confirming_reset);
        assert!(panel.begin_reset().is_err());
        assert_eq!(panel.stage, Stage::Idle);
    }

    #[test]
    fn a_reset_starts_only_after_the_owner_confirms() {
        let mut panel = panel(true);
        assert!(panel.begin_reset().is_err(), "no confirmation yet");
        assert_eq!(panel.stage, Stage::Idle);
        assert_eq!(panel.ask_to_reset(), None);
        assert!(panel.confirming_reset);
        assert_eq!(panel.begin_reset(), Ok(()));
        assert_eq!(panel.stage, Stage::ResettingRecords);
        assert!(!panel.confirming_reset);
        assert!(!panel.can_check(), "nothing else starts during the reset");
        let status = panel.finish_reset(Some(Ok(StateReset::Reset)));
        assert!(status.starts_with("Update records reset."), "{status}");
        assert_eq!(panel.stage, Stage::Idle);
        assert!(!panel.records_unreadable, "the action disappears");
    }

    #[test]
    fn a_reset_is_refused_while_an_update_runs_or_waits() {
        let staged = Version::new(1, 4, 0);
        for stage in [
            Stage::Checking,
            Stage::Staging,
            Stage::Downloading(staged),
            Stage::Ready(staged),
            Stage::InstallOnQuit(staged),
            Stage::HandedOver,
        ] {
            let mut panel = panel(true);
            panel.stage = stage;
            assert!(panel.ask_to_reset().is_some(), "{stage:?}");
            assert!(!panel.confirming_reset);
            // A confirmation shown before the step began is refused too.
            panel.confirming_reset = true;
            assert!(panel.begin_reset().is_err(), "{stage:?}");
            assert_eq!(panel.stage, stage);
        }
    }

    #[test]
    fn a_refused_reset_keeps_the_action() {
        let mut panel = panel(true);
        panel.confirming_reset = true;
        panel.begin_reset().expect("start");
        let status = panel.finish_reset(Some(Err(InstallError::Busy)));
        assert!(status.contains("not reset"), "{status}");
        assert!(panel.records_unreadable);
        assert_eq!(panel.stage, Stage::Idle);
        assert!(panel.finish_reset(None).contains("unexpectedly"));
        assert!(panel.records_unreadable);
    }
}

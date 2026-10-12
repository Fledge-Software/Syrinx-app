//! Shows a dated startup recovery offer without exposing storage ownership to the UI.

use crate::session::RecoveryPrompt;

use super::*;

pub(super) struct RecoveryCard {
    prompt: RecoveryPrompt,
    confirming: bool,
}

impl RecoveryCard {
    pub(super) fn new(prompt: RecoveryPrompt) -> Self {
        Self {
            prompt,
            confirming: false,
        }
    }

    fn confirm(&mut self, offer_id: u64) -> bool {
        if self.confirming || self.prompt.offer_id != offer_id {
            return false;
        }
        self.confirming = true;
        true
    }
}

impl BrowserView {
    pub(super) fn startup_panel(&self, cx: &mut Context<Self>) -> Div {
        let title = if self.failure.is_some() {
            "Your workspace could not be opened"
        } else if self.closing {
            "Closing Atlas…"
        } else if self.recovery.is_some() {
            "Restore your workspace from a backup?"
        } else {
            "Opening your workspace…"
        };
        let mut card = div()
            .flex()
            .flex_col()
            .gap_4()
            .max_w(px(620.))
            .child(div().text_2xl().child(title));
        if let Some(recovery) = &self.recovery {
            card = card
                .child(format!(
                    "Verified backup from {}",
                    backup_date(recovery.prompt.created_at)
                ))
                .child("Changes since this backup may be lost. Atlas will preserve the displaced database files.");
            if recovery.confirming && self.failure.is_none() && !self.session_stopped {
                card = card.child(
                    "Restoring the backup… Closing Atlas will wait for this operation to finish.",
                );
            } else if !self.closing && self.failure.is_none() {
                let offer_id = recovery.prompt.offer_id;
                card = card.child(button("restore-backup", "Restore this backup").on_click(
                    cx.listener(move |this, _, _, cx| this.confirm_recovery(offer_id, cx)),
                ));
            }
        }
        if let Some(failure) = &self.failure {
            card = card.child(div().text_color(rgb(0xf2b5b5)).child(failure.clone()));
        }
        if !self.closing {
            let label = if self
                .recovery
                .as_ref()
                .is_some_and(|recovery| !recovery.confirming)
                && self.failure.is_none()
            {
                "Cancel and exit"
            } else {
                "Close Atlas"
            };
            card = card.child(
                button("cancel-recovery", label)
                    .on_click(cx.listener(|this, _, _, cx| this.request_close(cx))),
            );
        }
        div()
            .size_full()
            .bg(rgb(0x141519))
            .text_color(rgb(0xecedf0))
            .text_sm()
            .font_family("Segoe UI Variable Text")
            .track_focus(&self.focus)
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(card)
    }

    fn confirm_recovery(&mut self, offer_id: u64, cx: &mut Context<Self>) {
        if self.closing || self.failure.is_some() {
            return;
        }
        let Some(recovery) = &mut self.recovery else {
            return;
        };
        if recovery.confirm(offer_id) {
            self.command(SessionCommand::ConfirmRecovery { offer_id });
            cx.notify();
        }
    }
}

fn backup_date(seconds: i64) -> String {
    // Convert Unix days to a Gregorian UTC date without a locale or new dependency.
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = month_index + if month_index < 10 { 3 } else { -9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let time = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        time / 3_600,
        time % 3_600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_dates_use_utc_across_epoch_and_leap_boundaries() {
        assert_eq!(backup_date(0), "1970-01-01 00:00 UTC");
        assert_eq!(backup_date(-1), "1969-12-31 23:59 UTC");
        assert_eq!(backup_date(951_825_600), "2000-02-29 12:00 UTC");
        assert_eq!(backup_date(4_107_542_400), "2100-03-01 00:00 UTC");
    }

    #[test]
    fn recovery_card_confirms_only_its_current_offer_once() {
        let mut card = RecoveryCard::new(RecoveryPrompt {
            offer_id: 7,
            created_at: 0,
        });
        assert!(!card.confirm(6));
        assert!(!card.confirming);
        assert!(card.confirm(7));
        assert!(!card.confirm(7));
    }
}

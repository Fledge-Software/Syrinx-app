//! Shows a dated startup recovery offer without exposing storage ownership to the UI.

use syrinx_ui::metrics::text;
use syrinx_ui::{Accent, Palette, PaletteId, palette};

use crate::session::RecoveryPrompt;
use crate::theme::rgba;

use super::*;

/// What the owner keeps and may lose, shown under the backup's date.
const PRESERVED_FILES: &str = "Changes made since this backup may be lost. \
Syrinx keeps the damaged database files instead of deleting them.";

/// The progress line shown once the owner has said yes.
const RESTORING: &str =
    "Restoring the backup… Closing Syrinx will wait for this operation to finish.";

pub(super) struct RecoveryCard {
    prompt: RecoveryPrompt,
    /// The backup's date line, formatted once when the offer arrives.
    backup_line: String,
    confirming: bool,
}

impl RecoveryCard {
    pub(super) fn new(prompt: RecoveryPrompt) -> Self {
        let local = local_backup_time(prompt.created_at);
        Self::dated(prompt, local.as_deref())
    }

    /// A card whose local date is already known, or unknown when Windows could not format it.
    fn dated(prompt: RecoveryPrompt, local: Option<&str>) -> Self {
        Self {
            backup_line: backup_line(prompt.created_at, local),
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

/// The palette this screen draws with. The owner's theme lives in the database that just
/// failed to open, so the screen uses the default Graphite rather than guess.
fn recovery_palette() -> Palette {
    palette(PaletteId::Graphite, Accent::default())
}

impl BrowserView {
    pub(super) fn startup_panel(&self, cx: &mut Context<Self>) -> Div {
        let colors = recovery_palette();
        let title = if self.failure.is_some() {
            "Your workspace could not be opened"
        } else if self.closing {
            "Closing Syrinx…"
        } else if self.recovery.is_some() {
            "Restore your workspace from a backup?"
        } else if self.history_repair.is_some() {
            "Clear all browsing history to continue?"
        } else {
            "Opening your workspace…"
        };
        let mut card = div().flex().flex_col().gap_4().max_w(px(620.)).child(
            div()
                .id("startup-title")
                .role(gpui::Role::Heading)
                .aria_level(1)
                .aria_label(title)
                .text_size(px(text::TITLE))
                .child(title),
        );
        if let Some(recovery) = &self.recovery {
            card = card
                .child(recovery.backup_line.clone())
                .child(PRESERVED_FILES);
            if recovery.confirming && self.failure.is_none() && !self.session_stopped {
                // A status node, like the footer's; GPUI exposes no live setting to raise.
                card = card.child(
                    div()
                        .id("restore-progress")
                        .role(gpui::Role::Status)
                        .aria_label(RESTORING)
                        .child(RESTORING),
                );
            } else if !self.closing && self.failure.is_none() {
                let offer_id = recovery.prompt.offer_id;
                card = card.child(button("restore-backup", "Restore this backup").on_click(
                    cx.listener(move |this, _, _, cx| this.confirm_recovery(offer_id, cx)),
                ));
            }
        }
        card = self.history_repair_section(card, cx);
        if let Some(failure) = &self.failure {
            card = card.child(
                div()
                    .text_color(rgba(colors.accent.ink))
                    .child(failure.clone()),
            );
        }
        if !self.closing {
            let undecided = self
                .recovery
                .as_ref()
                .is_some_and(|recovery| !recovery.confirming)
                || self.history_repair.is_some();
            let label = if undecided && self.failure.is_none() {
                "Cancel and exit"
            } else {
                "Close Syrinx"
            };
            card = card.child(
                button("cancel-recovery", label)
                    .on_click(cx.listener(|this, _, _, cx| this.request_close(cx))),
            );
        }
        div()
            .size_full()
            .bg(rgba(colors.ground.representative()))
            .text_color(rgba(colors.text))
            .text_size(px(text::BODY))
            .font_family(text::UI_FONT_FAMILIES[0])
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

/// The backup's local date and time, or `None` (reported on stderr) when Windows cannot
/// format it; the screen then shows the UTC date alone.
fn local_backup_time(created_at: i64) -> Option<String> {
    match syrinx_platform::calendar::local_date_time(created_at) {
        Ok(local) => Some(local),
        Err(error) => {
            eprintln!("Syrinx could not format the backup's local date: {error}");
            None
        }
    }
}

/// "Verified backup from <local date and time> (<UTC date and time>)."
fn backup_line(created_at: i64, local: Option<&str>) -> String {
    let utc = backup_date(created_at);
    match local {
        Some(local) => format!("Verified backup from {local} ({utc})."),
        None => format!("Verified backup from {utc}."),
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

    fn prompt(offer_id: u64, created_at: i64) -> RecoveryPrompt {
        RecoveryPrompt {
            offer_id,
            created_at,
        }
    }

    #[test]
    fn backup_dates_use_utc_across_epoch_and_leap_boundaries() {
        assert_eq!(backup_date(0), "1970-01-01 00:00 UTC");
        assert_eq!(backup_date(-1), "1969-12-31 23:59 UTC");
        assert_eq!(backup_date(951_825_600), "2000-02-29 12:00 UTC");
        assert_eq!(backup_date(4_107_542_400), "2100-03-01 00:00 UTC");
    }

    #[test]
    fn the_backup_line_gives_local_time_then_utc() {
        assert_eq!(
            backup_line(951_825_600, Some("Tuesday, February 29, 2000, 7:00 AM")),
            "Verified backup from Tuesday, February 29, 2000, 7:00 AM (2000-02-29 12:00 UTC)."
        );
    }

    #[test]
    fn the_backup_line_falls_back_to_utc_alone() {
        assert_eq!(
            backup_line(0, None),
            "Verified backup from 1970-01-01 00:00 UTC."
        );
    }

    #[test]
    fn a_new_card_reads_the_backup_time_in_local_time() {
        // The local part is in the owner's calendar, so only the UTC suffix is fixed text.
        let card = RecoveryCard::new(prompt(1, 1_784_116_800));
        let local = card
            .backup_line
            .strip_prefix("Verified backup from ")
            .and_then(|rest| rest.strip_suffix(" (2026-07-15 12:00 UTC)."));
        assert!(
            local.is_some_and(|local| !local.is_empty()),
            "{}",
            card.backup_line
        );
    }

    #[test]
    fn the_screen_says_damaged_files_are_kept_not_deleted() {
        assert!(PRESERVED_FILES.contains("keeps the damaged database files"));
        assert!(PRESERVED_FILES.contains("instead of deleting them"));
        // P1-04 keeps names and paths out of every message.
        assert!(!PRESERVED_FILES.contains('\\') && !PRESERVED_FILES.contains(".db"));
    }

    #[test]
    fn the_screen_draws_with_graphite_whatever_theme_is_live() {
        crate::theme::apply(palette(PaletteId::Paper, Accent::default()));
        let colors = recovery_palette();
        crate::theme::apply(palette(PaletteId::Graphite, Accent::default()));
        assert_eq!(colors.id, PaletteId::Graphite);
        assert_eq!(rgba(colors.ground.representative()), gpui::rgb(0x141519));
        assert_eq!(rgba(colors.text), gpui::rgb(0xecedf0));
    }

    #[test]
    fn recovery_card_confirms_only_its_current_offer_once() {
        let mut card = RecoveryCard::new(prompt(7, 0));
        assert!(!card.confirm(6));
        assert!(!card.confirming);
        assert!(card.confirm(7));
        assert!(!card.confirm(7));
    }

    #[test]
    fn a_renewed_offer_gets_an_undecided_card_with_its_own_date_and_offer() {
        let mut first = RecoveryCard::dated(prompt(7, 951_825_600), None);
        assert!(first.confirm(7));
        // The chosen backup failed validation; P1-04 renews the offer with another backup.
        let mut renewed = RecoveryCard::dated(prompt(8, 0), None);
        assert_ne!(renewed.backup_line, first.backup_line);
        assert_eq!(
            renewed.backup_line,
            "Verified backup from 1970-01-01 00:00 UTC."
        );
        assert!(
            !renewed.confirming,
            "the first card's yes does not carry over"
        );
        assert!(
            !renewed.confirm(7),
            "the old button cannot confirm the new offer"
        );
        assert!(renewed.confirm(8));
    }
}

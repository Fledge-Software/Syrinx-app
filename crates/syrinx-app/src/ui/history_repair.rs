//! Asks the owner to clear all history when Syrinx's record of cleared history is unusable.

use crate::session::{HistoryRepairPrompt, HistoryRepairReason};

use super::*;

pub(super) struct HistoryRepairCard {
    prompt: HistoryRepairPrompt,
    confirming: bool,
}

impl HistoryRepairCard {
    pub(super) fn new(prompt: HistoryRepairPrompt) -> Self {
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

    fn explanation(&self) -> &'static str {
        match self.prompt.reason {
            HistoryRepairReason::Damaged => {
                "Syrinx's record of the history you cleared is damaged."
            }
            HistoryRepairReason::Incomplete => {
                "Syrinx's record of the history you cleared is missing or older than your data."
            }
        }
    }
}

impl BrowserView {
    /// Adds the repair question, or its progress, to the startup card.
    pub(super) fn history_repair_section(&self, mut card: Div, cx: &mut Context<Self>) -> Div {
        let Some(repair) = &self.history_repair else {
            return card;
        };
        card = card.child(repair.explanation()).child(
            "Without it, history you cleared could come back from a backup. To continue, Syrinx \
             must clear all browsing history. Saved pages, open tabs and settings are kept. A copy \
             of the damaged record stays in Syrinx's data folder.",
        );
        if repair.confirming && self.failure.is_none() && !self.session_stopped {
            return card.child("Clearing all browsing history…");
        }
        if self.closing || self.failure.is_some() {
            return card;
        }
        let offer_id = repair.prompt.offer_id;
        card.child(
            button("repair-history", "Clear all history and continue").on_click(
                cx.listener(move |this, _, _, cx| this.confirm_history_repair(offer_id, cx)),
            ),
        )
    }

    fn confirm_history_repair(&mut self, offer_id: u64, cx: &mut Context<Self>) {
        if self.closing || self.failure.is_some() {
            return;
        }
        let Some(repair) = &mut self.history_repair else {
            return;
        };
        if repair.confirm(offer_id) {
            self.command(SessionCommand::ConfirmHistoryRepair { offer_id });
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_card_confirms_only_its_current_offer_once() {
        let mut card = HistoryRepairCard::new(HistoryRepairPrompt {
            offer_id: 3,
            reason: HistoryRepairReason::Damaged,
        });
        assert!(!card.confirm(2));
        assert!(card.confirm(3));
        assert!(!card.confirm(3));
    }
}

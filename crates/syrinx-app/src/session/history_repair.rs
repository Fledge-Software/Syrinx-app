//! Offers the owner a way past an unusable deletion journal: clear all history, then continue.

use syrinx_platform::JournalError;
use syrinx_store::deletion::DeletionError;

use super::*;

/// Why the journal cannot be trusted, which the prompt explains in plain words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryRepairReason {
    /// A complete record failed its checksum or is in an unknown format.
    Damaged,
    /// The database has applied clears the journal no longer records (it was lost or replaced).
    Incomplete,
    /// A record names a category only a newer Syrinx writes, which this build cannot honour (#121).
    Newer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryRepairPrompt {
    pub offer_id: u64,
    pub reason: HistoryRepairReason,
}

/// Recognizes the journal refusals the owner can resolve by clearing all history.
pub(super) fn repairable(error: &SessionError) -> Option<HistoryRepairReason> {
    match error {
        SessionError::Deletion(JournalError::Deletion(DeletionError::MissingGeneration)) => {
            Some(HistoryRepairReason::Incomplete)
        }
        SessionError::Deletion(JournalError::Deletion(
            DeletionError::DamagedRecord
            | DeletionError::UnsupportedFormat
            | DeletionError::TooLarge,
        )) => Some(HistoryRepairReason::Damaged),
        SessionError::Deletion(JournalError::Deletion(DeletionError::UnsupportedCategory)) => {
            Some(HistoryRepairReason::Newer)
        }
        _ => None,
    }
}

/// Asks the owner and waits; nothing is cleared or rewritten unless they confirm.
pub(super) fn confirm(
    reason: HistoryRepairReason,
    commands: &mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<bool, SessionError> {
    let prompt = HistoryRepairPrompt {
        offer_id: recovery::next_offer_id()?,
        reason,
    };
    emit(events, SessionEvent::HistoryRepairOffered(prompt))?;
    recovery::await_decision(
        commands,
        events,
        |command| matches!(command, SessionCommand::ConfirmHistoryRepair { offer_id } if *offer_id == prompt.offer_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(error: DeletionError) -> SessionError {
        SessionError::Deletion(JournalError::Deletion(error))
    }

    #[test]
    fn each_journal_refusal_names_the_reason_the_prompt_explains() {
        assert_eq!(
            repairable(&journal(DeletionError::DamagedRecord)),
            Some(HistoryRepairReason::Damaged)
        );
        assert_eq!(
            repairable(&journal(DeletionError::MissingGeneration)),
            Some(HistoryRepairReason::Incomplete)
        );
        // Only a newer Syrinx writes a category this build does not know (#121).
        assert_eq!(
            repairable(&journal(DeletionError::UnsupportedCategory)),
            Some(HistoryRepairReason::Newer)
        );
        assert_eq!(repairable(&journal(DeletionError::PendingClear)), None);
    }
}

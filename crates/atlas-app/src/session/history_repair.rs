//! Offers the owner a way past an unusable deletion journal: clear all history, then continue.

use atlas_platform::JournalError;
use atlas_store::deletion::DeletionError;

use super::*;

/// Why the journal cannot be trusted, which the prompt explains in plain words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryRepairReason {
    /// A complete record failed its checksum or is in an unknown format.
    Damaged,
    /// The database has applied clears the journal no longer records (it was lost or replaced).
    Incomplete,
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
            | DeletionError::UnsupportedCategory
            | DeletionError::TooLarge,
        )) => Some(HistoryRepairReason::Damaged),
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

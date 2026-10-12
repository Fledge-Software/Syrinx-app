//! Holds a guarded startup or failed-open recovery offer until explicit confirmation or cancellation.

use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use atlas_platform::{RecoveryError, RecoveryOffer, StartupStorage, offer_recovery};
use atlas_store::{StoreOperation, deletion::DeletionLog};

use super::*;

static NEXT_OFFER: AtomicU64 = AtomicU64::new(1);
const DECISION_POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryPrompt {
    pub offer_id: u64,
    pub created_at: i64,
}

pub(super) fn prepare_startup(directory: &Path) -> Result<StartupStorage, SessionError> {
    // The preview has no history/deletions; nonzero backup generations still require refusal.
    atlas_platform::prepare_startup(directory, &DeletionLog::default()).map_err(|error| match error
    {
        RecoveryError::Location(error) => SessionError::Location(error),
        error => SessionError::Recovery(error.to_string()),
    })
}

pub(super) fn confirm_failed_start(
    directory: &Path,
    startup: SessionError,
    commands: &mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<bool, SessionError> {
    if !can_probe(&startup) {
        return Err(startup);
    }
    // This preview collects no history/deletions; the API still rejects nonzero backup generations.
    let offer = offer_recovery(directory, &DeletionLog::default()).map_err(|error| {
        SessionError::RecoveryFailed {
            startup: Box::new(startup),
            recovery: error.to_string(),
        }
    })?;
    confirm_offer(offer, commands, events)
}

pub(super) fn confirm_offer(
    offer: RecoveryOffer,
    commands: &mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<bool, SessionError> {
    let offer_id = NEXT_OFFER
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .map_err(|_| SessionError::RecoveryUnavailable)?;
    let prompt = RecoveryPrompt {
        offer_id,
        created_at: offer.metadata().created_at,
    };
    emit(events, SessionEvent::RecoveryOffered(prompt))?;
    wait_for_confirmation(offer, prompt, commands, events)
}

fn can_probe(error: &SessionError) -> bool {
    // These errors permit inspection only. The platform, not this routing, proves damage.
    match error {
        SessionError::Location(LocationError::RecoveryPending) => true,
        SessionError::Store(StoreError::Database { operation, .. }) => matches!(
            operation,
            StoreOperation::Open
                | StoreOperation::Configure
                | StoreOperation::InspectSchema
                | StoreOperation::QuickCheck
                | StoreOperation::IntegrityCheck
                | StoreOperation::Migrate
                | StoreOperation::BeginRead
                | StoreOperation::Read
        ),
        SessionError::Store(
            StoreError::IntegrityCheckFailed
            | StoreError::InvalidEncoding(_)
            | StoreError::InvalidSnapshot(_),
        ) => true,
        _ => false,
    }
}

fn wait_for_confirmation(
    offer: RecoveryOffer,
    prompt: RecoveryPrompt,
    commands: &mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<bool, SessionError> {
    loop {
        // A dropped UI must release the exclusive offer even if its sender is still alive.
        if events.is_closed() {
            return Err(SessionError::EventReceiverClosed);
        }
        match commands.recv_timeout(DECISION_POLL) {
            Ok(SessionCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Ok(false);
            }
            Ok(SessionCommand::ConfirmRecovery { offer_id }) if offer_id == prompt.offer_id => {
                offer
                    .confirm()
                    .map_err(|error| SessionError::Recovery(error.to_string()))?;
                return Ok(true);
            }
            Ok(_) => reject_command(events)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

pub(super) fn reject_command(events: &UnboundedSender<SessionEvent>) -> Result<(), SessionError> {
    emit(
        events,
        SessionEvent::Failure {
            error: SessionError::RecoveryUnavailable,
            fatal: false,
        },
    )
}

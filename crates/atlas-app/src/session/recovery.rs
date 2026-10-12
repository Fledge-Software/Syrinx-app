//! Holds a guarded startup or failed-open recovery offer until explicit confirmation or cancellation.

use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use atlas_platform::{
    JournalError, RecoveryError, RecoveryOffer, StartupStorage, offer_recovery, read_deletion_log,
};
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
    // A recovery offer replays this journal onto the restored copy (D47).
    let deletions = startup_log(directory)?;
    atlas_platform::prepare_startup(directory, &deletions).map_err(|error| match error {
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
    // The journal is read again here: the failed open may have followed a clear.
    let offer = startup_log(directory)
        .map_err(|error| error.to_string())
        .and_then(|deletions| {
            offer_recovery(directory, &deletions).map_err(|error| error.to_string())
        })
        .map_err(|recovery| SessionError::RecoveryFailed {
            startup: Box::new(startup),
            recovery,
        })?;
    confirm_offer(offer, commands, events)
}

/// The journal a recovery offer replays. A damaged one is treated as empty here: only
/// generation-0 backups can then be offered, and opening the restored database still meets
/// the damaged journal, which forces the owner-confirmed history repair before any history
/// is shown.
fn startup_log(directory: &Path) -> Result<DeletionLog, SessionError> {
    match read_deletion_log(directory) {
        Ok(log) => Ok(log),
        Err(JournalError::Deletion(_)) => Ok(DeletionLog::default()),
        Err(error) => Err(SessionError::Deletion(error)),
    }
}

/// A fresh identity for one startup decision, so a stale click cannot confirm a later one.
pub(super) fn next_offer_id() -> Result<u64, SessionError> {
    NEXT_OFFER
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .map_err(|_| SessionError::RecoveryUnavailable)
}

pub(super) fn confirm_offer(
    offer: RecoveryOffer,
    commands: &mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
) -> Result<bool, SessionError> {
    let offer_id = next_offer_id()?;
    let prompt = RecoveryPrompt {
        offer_id,
        created_at: offer.metadata().created_at,
    };
    emit(events, SessionEvent::RecoveryOffered(prompt))?;
    // The offer keeps exclusive storage ownership while the owner decides.
    let confirmed = await_decision(
        commands,
        events,
        |command| matches!(command, SessionCommand::ConfirmRecovery { offer_id } if *offer_id == prompt.offer_id),
    )?;
    if !confirmed {
        return Ok(false);
    }
    offer
        .confirm()
        .map_err(|error| SessionError::Recovery(error.to_string()))?;
    Ok(true)
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

/// Waits for the matching confirmation; shutdown or a closed command channel declines.
pub(super) fn await_decision(
    commands: &mpsc::Receiver<SessionCommand>,
    events: &UnboundedSender<SessionEvent>,
    is_confirmation: impl Fn(&SessionCommand) -> bool,
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
            Ok(command) if is_confirmation(&command) => return Ok(true),
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

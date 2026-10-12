//! Persists appearance settings and tile edits on the session worker, beside the tab model.

use atlas_core::QuickDestinations;
use atlas_store::{Store, StoredPreferences};
use futures::channel::mpsc::UnboundedSender;

use super::{SessionCommand, SessionError, SessionEvent, emit};

/// Loads the stored preferences once, at startup.
pub(super) fn restore(store: &Store) -> Result<StoredPreferences, SessionError> {
    store.load_preferences().map_err(SessionError::Store)
}

/// Handles a preference command, or hands any other command back to the tab model.
pub(super) fn intercept(
    store: &Store,
    destinations: &mut QuickDestinations,
    command: SessionCommand,
    events: &UnboundedSender<SessionEvent>,
) -> Result<Option<SessionCommand>, SessionError> {
    match command {
        SessionCommand::SaveSettings(entries) => {
            // The window already shows the new appearance; a failed save stops like a tab save.
            store.save_settings(entries).map_err(SessionError::Store)?;
            Ok(None)
        }
        SessionCommand::EditDestinations { surface, edit } => {
            // Resolve each edit against the latest lists, then persist only the changed surface.
            let mut edited = destinations.clone();
            if let Err(error) = edited.apply(surface, edit) {
                // A refused edit is a recoverable UI error, like an invalid tab command.
                emit(
                    events,
                    SessionEvent::Failure {
                        error: SessionError::Destination(error),
                        fatal: false,
                    },
                )?;
                return Ok(None);
            }
            store
                .save_destinations(surface, edited.list(surface).to_vec())
                .map_err(SessionError::Store)?;
            *destinations = edited;
            emit(events, SessionEvent::Destinations(destinations.clone()))?;
            Ok(None)
        }
        other => Ok(Some(other)),
    }
}

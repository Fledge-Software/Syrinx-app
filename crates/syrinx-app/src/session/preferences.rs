//! Persists appearance settings, tile edits and site zoom levels on the session worker.

use futures::channel::mpsc::UnboundedSender;
use syrinx_core::QuickDestinations;
use syrinx_store::{Store, StoredPreferences};

use super::{SessionCommand, SessionError, SessionEvent, SiteZoomLevels, emit, utc_seconds};

/// Loads the stored preferences once, at startup.
pub(super) fn restore(store: &Store) -> Result<StoredPreferences, SessionError> {
    store.load_preferences().map_err(SessionError::Store)
}

/// Loads each site's saved zoom level, at startup and after a clear (#90).
pub(super) fn restore_site_zooms(store: &Store) -> Result<SiteZoomLevels, SessionError> {
    store
        .site_zooms()
        .map(SiteZoomLevels)
        .map_err(SessionError::Store)
}

/// Saves one site's level, stamped now so a ranged clear can find it.
fn save_site_zoom(store: &Store, host: String, factor: Option<f64>) -> Result<(), SessionError> {
    let now = utc_seconds()?;
    store
        .save_site_zoom(host, factor, now)
        .map_err(SessionError::Store)
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
            // The window already shows the new appearance; the loop decides whether a refusal
            // stops the worker (a damaged database) or is reported and skipped (#9).
            store.save_settings(entries).map_err(SessionError::Store)?;
            Ok(None)
        }
        SessionCommand::SiteZoom { host, factor } => {
            // The page already shows the level; a failed save is reported but never stops Syrinx.
            if let Err(error) = save_site_zoom(store, host, factor) {
                emit(
                    events,
                    SessionEvent::Failure {
                        error,
                        fatal: false,
                    },
                )?;
            }
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

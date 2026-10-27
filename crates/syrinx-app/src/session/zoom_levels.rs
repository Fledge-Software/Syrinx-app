//! Resends the window its site zoom levels when telling it failed after a clear applied (#196).

use futures::channel::mpsc::UnboundedSender;
use syrinx_store::Store;

use super::{SessionError, SessionEvent, SiteZoomLevels, emit, preferences};

/// Failed reports (the first send and its idle retries) before the worker gives up and says so.
pub(super) const MAX_ATTEMPTS: u32 = 4;

/// Whether the window's copy of the zoom levels is behind the database's, and how often
/// reporting them has failed since the window last heard them.
#[derive(Debug, Default)]
pub(super) struct ZoomLevelsReport {
    /// Failed reports since the window last heard the levels; zero means it is level.
    failures: u32,
    /// Reads to refuse before the next real one, so a test can make one fail (#196).
    #[cfg(test)]
    refused_reads: u32,
}

impl ZoomLevelsReport {
    /// Whether the window may still be showing levels the database no longer holds.
    pub(super) fn is_stale(&self) -> bool {
        self.failures > 0
    }

    /// Reads the levels the database now holds and sends them to the window. A failure marks
    /// the window stale so the next idle tick tries again, and is returned to the caller.
    pub(super) fn send(
        &mut self,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let sent = self
            .read(store)
            .and_then(|levels| emit(events, SessionEvent::SiteZoomsReplaced(levels)));
        match &sent {
            Ok(()) => self.failures = 0,
            Err(_) => self.failures = self.failures.saturating_add(1),
        }
        sent
    }

    /// The idle tick: resends the levels if an earlier report failed. Each failed retry is
    /// logged; after `MAX_ATTEMPTS` failures the worker gives up and returns the error.
    pub(super) fn retry(
        &mut self,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        if !self.is_stale() {
            return Ok(());
        }
        let Err(error) = self.send(store, events) else {
            return Ok(());
        };
        if self.failures < MAX_ATTEMPTS {
            eprintln!("{}", retry_line(&error, self.failures));
            return Ok(());
        }
        eprintln!("{}", give_up_line(&error));
        self.failures = 0;
        Err(error)
    }

    /// Reads the saved levels, or fails as a busy database would when a test asked it to.
    fn read(&mut self, store: &Store) -> Result<SiteZoomLevels, SessionError> {
        #[cfg(test)]
        if self.refused_reads > 0 {
            self.refused_reads -= 1;
            return Err(SessionError::Store(syrinx_store::StoreError::Database {
                operation: syrinx_store::StoreOperation::Read,
                code: Some(5),
            }));
        }
        preferences::restore_site_zooms(store)
    }

    /// Makes the next `count` reads fail, as a busy database would.
    #[cfg(test)]
    pub(super) fn refuse_reads(&mut self, count: u32) {
        self.refused_reads = count;
    }
}

/// The diagnostic line for a resend that failed and will be tried again. It names no site.
fn retry_line(error: &SessionError, failures: u32) -> String {
    format!("Resending the window its zoom levels failed ({failures} of {MAX_ATTEMPTS}): {error}")
}

/// The diagnostic line for a resend given up on, so the window may keep stale levels.
fn give_up_line(error: &SessionError) -> String {
    format!("The window may show out-of-date zoom levels; giving up resending them: {error}")
}

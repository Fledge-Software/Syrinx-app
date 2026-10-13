//! Retains session ownership until every SQLite connection and its worker have closed.

use std::sync::Arc;

use crate::{DirectoryLease, ShutdownState, StorageSession, StoreError};

pub(super) struct HeldSession {
    // Drop the token before its backing lease on failed startup and fallback cleanup.
    token: Option<Box<dyn StorageSession>>,
    lease: Arc<dyn DirectoryLease>,
}

impl HeldSession {
    pub(super) fn begin(lease: Arc<dyn DirectoryLease>) -> Result<Self, StoreError> {
        let token = match lease.session_control() {
            Some(control) => Some(control.begin()?),
            None => None,
        };
        Ok(Self { token, lease })
    }

    pub(super) fn lease(&self) -> &Arc<dyn DirectoryLease> {
        &self.lease
    }

    pub(super) fn previous_shutdown(&self) -> ShutdownState {
        match &self.token {
            Some(token) => token.previous_shutdown(),
            None => ShutdownState::Unclean,
        }
    }

    pub(super) fn complete(self) -> Result<(), StoreError> {
        // Keep the strong lease through finalization, including a failed clean-marker write.
        let Self { token, lease } = self;
        let result = match token {
            Some(token) => token.complete().map_err(StoreError::from),
            None => Ok(()),
        };
        drop(lease);
        result
    }
}

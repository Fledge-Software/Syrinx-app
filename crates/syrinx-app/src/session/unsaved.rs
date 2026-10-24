//! Holds back a tab model a busy database refused, and decides when to try again and when to
//! give up and stop the worker with the reason (#9).

use std::time::{Duration, Instant};

use syrinx_store::StoreError;

use super::{SessionError, stop};

/// How long after a refused tab save the next try waits; commands meanwhile only change memory.
pub(super) const RETRY_DELAY: Duration = Duration::from_secs(30);
/// Consecutive refused tab saves after which the worker stops instead of trying again.
pub(super) const MAX_FAILED_SAVES: u32 = 3;

/// Refused tab saves since the last one that succeeded, and when the next try may run.
#[derive(Debug, Default)]
pub(super) struct Unsaved {
    failures: u32,
    retry_at: Option<Instant>,
}

impl Unsaved {
    /// Whether the model on disk is behind the one in memory.
    pub(super) fn pending(&self) -> bool {
        self.failures > 0
    }

    /// Whether a save may run at `now`: always when nothing was refused, else after the delay.
    pub(super) fn may_save(&self, now: Instant) -> bool {
        self.retry_at.is_none_or(|at| now >= at)
    }

    /// How long until the next try, while a refused model waits.
    pub(super) fn wait(&self, now: Instant) -> Option<Duration> {
        self.retry_at.map(|at| at.saturating_duration_since(now))
    }

    pub(super) fn saved(&mut self) {
        *self = Self::default();
    }

    /// Records a refused save and returns the error to handle. A failure the loop can survive
    /// becomes a stop once it is certain to recur or has recurred `MAX_FAILED_SAVES` times.
    pub(super) fn refused(&mut self, error: StoreError, now: Instant) -> SessionError {
        self.failures = self.failures.saturating_add(1);
        self.retry_at = Some(now + RETRY_DELAY);
        let gives_up = !stop::transient_save_failure(&error) || self.failures >= MAX_FAILED_SAVES;
        let error = SessionError::Store(error);
        if !stop::survivable(&error) || !gives_up {
            return error;
        }
        SessionError::TabsNotSaved {
            attempts: self.failures,
            error: Box::new(error),
        }
    }

    /// Lets the next try run at once, as a test that has waited out the delay would.
    #[cfg(test)]
    pub(super) fn retry_now(&mut self) {
        if self.retry_at.is_some() {
            self.retry_at = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use syrinx_store::StoreOperation;

    use super::*;

    fn busy() -> StoreError {
        StoreError::Database {
            operation: StoreOperation::BeginWrite,
            code: Some(5),
        }
    }

    #[test]
    fn a_busy_database_is_retried_after_the_delay_then_stops_the_worker() {
        let start = Instant::now();
        let mut unsaved = Unsaved::default();
        assert!(unsaved.may_save(start));
        for attempt in 1..MAX_FAILED_SAVES {
            assert_eq!(
                unsaved.refused(busy(), start),
                SessionError::Store(busy()),
                "attempt {attempt}"
            );
            assert!(unsaved.pending());
            assert!(!unsaved.may_save(start));
            assert!(unsaved.may_save(start + RETRY_DELAY));
        }
        assert_eq!(
            unsaved.refused(busy(), start),
            SessionError::TabsNotSaved {
                attempts: MAX_FAILED_SAVES,
                error: Box::new(SessionError::Store(busy())),
            }
        );
        assert!(!stop::survivable(&unsaved.refused(busy(), start)));
        unsaved.saved();
        assert!(!unsaved.pending());
        assert!(unsaved.may_save(start));
    }

    #[test]
    fn a_refused_row_stops_the_worker_at_once_and_lost_storage_keeps_its_own_error() {
        let constraint = StoreError::Database {
            operation: StoreOperation::Write,
            code: Some(275),
        };
        for error in [constraint, StoreError::TooManyRecords] {
            let stopped = Unsaved::default().refused(error.clone(), Instant::now());
            assert_eq!(
                stopped,
                SessionError::TabsNotSaved {
                    attempts: 1,
                    error: Box::new(SessionError::Store(error)),
                }
            );
        }
        let stale = StoreError::StaleRevision {
            expected: 4,
            actual: 5,
        };
        assert_eq!(
            Unsaved::default().refused(stale.clone(), Instant::now()),
            SessionError::Store(stale)
        );
    }
}

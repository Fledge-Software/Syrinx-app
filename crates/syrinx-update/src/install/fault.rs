//! Named points where tests stop staging or the helper, as a crash or power cut would.
//!
//! The hook exists only in test builds; shipped code compiles every checkpoint to nothing.

/// A point between two durable changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Staging removed an earlier staged copy and its records.
    StagingCleared,
    /// The package files are in `incoming`, not yet verified.
    CopiedToIncoming,
    /// The first program file is in `Syrinx.new`.
    FirstStagedFile,
    /// Every program file is in `Syrinx.new`.
    StagedCopied,
    /// The helper and manifest are in `Updates\apply`.
    HelperCopied,
    /// `incoming` is gone and `pending` is not yet written.
    IncomingRemoved,
    /// The old Syrinx has exited and the helper holds the lock.
    ExitObserved,
    /// The staged copy passed its second verification.
    Reverified,
    /// The older `Syrinx.previous` was removed.
    PreviousRemoved,
    /// `Syrinx` became `Syrinx.previous`; `Syrinx.new` is not yet in place.
    ProgramMovedAside,
    /// `Syrinx.new` became `Syrinx`.
    Swapped,
    /// The new Syrinx was started and has not yet reported a clean start.
    Launched,
    /// Rollback moved the failed version aside; the previous one is not yet back.
    FailedMovedAside,
}

#[cfg(not(test))]
pub(crate) fn checkpoint(_step: Step) -> Result<(), super::InstallError> {
    Ok(())
}

#[cfg(test)]
pub(crate) use hook::checkpoint;

#[cfg(test)]
pub(crate) mod hook {
    use std::cell::Cell;

    use super::Step;
    use crate::install::InstallError;

    thread_local! {
        static STOP_AT: Cell<Option<Step>> = const { Cell::new(None) };
    }

    /// Stop the next time this thread reaches `step`; `None` runs to completion.
    pub(crate) fn stop_at(step: Option<Step>) {
        STOP_AT.with(|stop| stop.set(step));
    }

    pub(crate) fn checkpoint(step: Step) -> Result<(), InstallError> {
        if STOP_AT.with(Cell::get) == Some(step) {
            STOP_AT.with(|stop| stop.set(None));
            return Err(InstallError::Interrupted(step));
        }
        Ok(())
    }
}

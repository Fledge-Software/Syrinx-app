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
    /// A discard wrote the staging mark; `pending` is not yet removed.
    DiscardMarked,
    /// A discard marked `Syrinx.new` and removed `pending`; `Syrinx.new` is not yet removed.
    DiscardBegun,
    /// The helper recorded a failure before the swap and still holds the update lock.
    FailureRecorded,
}

#[cfg(not(test))]
pub(crate) fn checkpoint(_step: Step) -> Result<(), super::InstallError> {
    Ok(())
}

#[cfg(test)]
pub(crate) use hook::checkpoint;

#[cfg(test)]
pub(crate) mod hook {
    use std::cell::{Cell, RefCell};

    use super::Step;
    use crate::install::InstallError;

    /// Work a test runs at a step, such as a start racing the helper at a fixed point.
    type Action = Box<dyn FnOnce()>;

    thread_local! {
        static STOP_AT: Cell<Option<Step>> = const { Cell::new(None) };
        static RUN_AT: RefCell<Option<(Step, Action)>> = const { RefCell::new(None) };
    }

    /// Stop the next time this thread reaches `step`; `None` runs to completion.
    pub(crate) fn stop_at(step: Option<Step>) {
        STOP_AT.with(|stop| stop.set(step));
    }

    /// Run `action` once, the next time this thread reaches `step`, then carry on.
    pub(crate) fn run_at(step: Step, action: impl FnOnce() + 'static) {
        RUN_AT.with(|run| *run.borrow_mut() = Some((step, Box::new(action))));
    }

    pub(crate) fn checkpoint(step: Step) -> Result<(), InstallError> {
        run_action_at(step);
        if STOP_AT.with(Cell::get) == Some(step) {
            STOP_AT.with(|stop| stop.set(None));
            return Err(InstallError::Interrupted(step));
        }
        Ok(())
    }

    /// Take the action registered for `step`, if any, before running it, so the action may
    /// itself pass checkpoints.
    fn run_action_at(step: Step) {
        let due = RUN_AT.with(|run| {
            let mut run = run.borrow_mut();
            let registered = run.as_ref().is_some_and(|(at, _)| *at == step);
            if registered { run.take() } else { None }
        });
        if let Some((_, action)) = due {
            action();
        }
    }
}

//! Bounds synthetic smoke startup and shutdown even when the GUI cannot service its timer.

use std::{
    fmt,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

const HARD_DEADLINE: Duration = Duration::from_secs(30);

pub(crate) struct SmokeWatchdog {
    cancel: Sender<()>,
    worker: JoinHandle<()>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum WatchdogError {
    Start {
        code: Option<i32>,
    },
    Stop {
        cancellation_failed: bool,
        worker_panicked: bool,
    },
}

impl fmt::Display for WatchdogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start { code } => {
                write!(formatter, "Cannot start the smoke watchdog ({code:?}).")
            }
            Self::Stop {
                cancellation_failed,
                worker_panicked,
            } => write!(
                formatter,
                "Cannot stop the smoke watchdog (cancellation failed: {cancellation_failed}; worker panicked: {worker_panicked})."
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchdogFailure {
    Deadline,
    ControlDisconnected,
}

impl SmokeWatchdog {
    pub(crate) fn start() -> Result<Self, WatchdogError> {
        Self::start_with(HARD_DEADLINE, terminate_smoke)
    }

    fn start_with(
        deadline: Duration,
        on_failure: impl FnOnce(WatchdogFailure) + Send + 'static,
    ) -> Result<Self, WatchdogError> {
        let (cancel, cancellation) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("atlas-smoke-watchdog".to_owned())
            .spawn(move || {
                if let Some(failure) = wait_for_cancellation(cancellation, deadline) {
                    on_failure(failure);
                }
            })
            .map_err(|error| WatchdogError::Start {
                code: error.raw_os_error(),
            })?;
        Ok(Self { cancel, worker })
    }

    pub(crate) fn cancel(self) -> Result<(), WatchdogError> {
        // Join even if sending fails, so a stopped or panicked worker is never detached.
        let cancellation_failed = self.cancel.send(()).is_err();
        let worker_panicked = self.worker.join().is_err();
        if cancellation_failed || worker_panicked {
            return Err(WatchdogError::Stop {
                cancellation_failed,
                worker_panicked,
            });
        }
        Ok(())
    }
}

fn wait_for_cancellation(
    cancellation: Receiver<()>,
    deadline: Duration,
) -> Option<WatchdogFailure> {
    match cancellation.recv_timeout(deadline) {
        Ok(()) => None,
        Err(mpsc::RecvTimeoutError::Timeout) => Some(WatchdogFailure::Deadline),
        Err(mpsc::RecvTimeoutError::Disconnected) => Some(WatchdogFailure::ControlDisconnected),
    }
}

fn terminate_smoke(failure: WatchdogFailure) {
    match failure {
        WatchdogFailure::Deadline => {
            eprintln!("Synthetic smoke startup or shutdown exceeded its 30-second hard deadline.");
        }
        WatchdogFailure::ControlDisconnected => {
            eprintln!("The synthetic smoke watchdog lost its cancellation channel.");
        }
    }
    // Only --smoke's fresh synthetic profile reaches this last resort; normal runs never start it.
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;

    #[test]
    fn cancellation_joins_without_invoking_the_failure_action() {
        let failed = Arc::new(AtomicBool::new(false));
        let observed = failed.clone();
        let watchdog = SmokeWatchdog::start_with(Duration::from_secs(30), move |_| {
            observed.store(true, Ordering::Release);
        })
        .expect("start synthetic watchdog");
        watchdog.cancel().expect("cancel and join watchdog");
        assert!(!failed.load(Ordering::Acquire));
    }

    #[test]
    fn expiry_runs_without_gui_or_cancellation_activity() {
        let (failures, observed) = mpsc::channel();
        let SmokeWatchdog { cancel, worker } =
            SmokeWatchdog::start_with(Duration::ZERO, move |failure| {
                failures.send(failure).expect("report synthetic expiration");
            })
            .expect("start expiring synthetic watchdog");
        worker.join().expect("join expired watchdog");
        assert_eq!(
            observed.recv().expect("observe expiration"),
            WatchdogFailure::Deadline
        );
        // The sender remains alive until after expiry, proving this was not disconnection.
        drop(cancel);
    }

    #[test]
    fn dropped_control_is_a_failure_not_successful_cancellation() {
        let (failures, observed) = mpsc::channel();
        let SmokeWatchdog { cancel, worker } =
            SmokeWatchdog::start_with(Duration::from_secs(30), move |failure| {
                failures.send(failure).expect("report synthetic disconnect");
            })
            .expect("start synthetic watchdog");
        drop(cancel);
        worker.join().expect("join disconnected watchdog");
        assert_eq!(
            observed.recv().expect("observe disconnect"),
            WatchdogFailure::ControlDisconnected
        );
    }
}

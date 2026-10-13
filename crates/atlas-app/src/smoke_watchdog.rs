//! Bounds synthetic smoke startup and shutdown even when the GUI cannot service its timer.

use std::{
    fmt,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

const HARD_DEADLINE: Duration = Duration::from_secs(30);
/// P1-33's acceptance run seeds 100,000 rows and types several hundred keys, so it waits longer.
const ACCEPTANCE_DEADLINE: Duration = Duration::from_secs(600);

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

    /// Bounds P1-33's palette acceptance run, which also uses a fresh synthetic profile.
    pub(crate) fn start_acceptance() -> Result<Self, WatchdogError> {
        Self::start_with(ACCEPTANCE_DEADLINE, terminate_smoke)
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
    // Output can block behind another thread; the exit code alone reports this last-resort failure.
    let code = match failure {
        WatchdogFailure::Deadline => 124,
        WatchdogFailure::ControlDisconnected => 125,
    };
    // Only the smoke and acceptance runs' fresh synthetic profiles reach this last resort.
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use std::{
        env, io,
        process::{Child, Command, ExitStatus, Stdio},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Instant,
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

    const CHILD_MODE: &str = "ATLAS_WATCHDOG_LOCKED_STDERR_TEST_CHILD";
    const CHILD_TEST: &str = "smoke_watchdog::tests::terminal_failure_exits_while_stderr_is_locked";

    #[test]
    fn terminal_failure_exits_while_stderr_is_locked() {
        if let Some(mode) = env::var_os(CHILD_MODE) {
            let disconnected = match mode.to_str() {
                Some("deadline") => false,
                Some("disconnected") => true,
                _ => panic!("invalid synthetic watchdog child mode"),
            };
            hold_stderr_until_watchdog_exits(disconnected);
        }

        for (mode, code) in [("deadline", 124), ("disconnected", 125)] {
            let mut child =
                Command::new(env::current_exe().expect("locate watchdog test executable"))
                    .args(["--exact", CHILD_TEST, "--nocapture"])
                    .env(CHILD_MODE, mode)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("start synthetic watchdog subprocess");
            let status =
                wait_for_child(&mut child).expect("bounded watchdog subprocess completion");
            assert_eq!(status.code(), Some(code), "watchdog mode {mode}");
        }
    }

    fn hold_stderr_until_watchdog_exits(disconnected: bool) -> ! {
        // --nocapture ensures eprintln would contend for this real stderr lock.
        let stderr = io::stderr();
        let _locked = stderr.lock();
        let SmokeWatchdog { cancel, worker } =
            SmokeWatchdog::start_with(Duration::from_millis(100), terminate_smoke)
                .expect("start real synthetic watchdog termination");
        let _retained_sender = if disconnected {
            drop(cancel);
            None
        } else {
            Some(cancel)
        };
        let _retained_worker = worker;
        loop {
            thread::park();
        }
    }

    fn wait_for_child(child: &mut Child) -> Result<ExitStatus, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let failure = loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) => break "Watchdog subprocess exceeded its external deadline.".to_owned(),
                Err(error) => break format!("Cannot poll watchdog subprocess: {error}."),
            }
        };
        // Terminate and reap only this test's child, including after a polling failure.
        match stop_child(child) {
            Ok(()) => Err(failure),
            Err(cleanup) => Err(format!("{failure} Cleanup failed: {cleanup}")),
        }
    }

    fn stop_child(child: &mut Child) -> Result<(), String> {
        let termination = child
            .kill()
            .map_err(|error| format!("Cannot terminate watchdog subprocess: {error}."));
        let reaping = reap_child(child);
        match (termination, reaping) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(termination), Err(reaping)) => Err(format!("{termination} {reaping}")),
        }
    }

    fn reap_child(child: &mut Child) -> Result<(), String> {
        // Observe exit even when termination failed, without an unbounded wait during cleanup.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    return Err("Watchdog subprocess did not exit during cleanup.".to_owned());
                }
                Err(error) => return Err(format!("Cannot reap watchdog subprocess: {error}.")),
            }
        }
    }
}

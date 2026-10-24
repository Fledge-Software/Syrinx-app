//! Lets the WebView2 browser process save the profile (cookies included) before Syrinx exits (#96).

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use webview2_com::{
    BrowserProcessExitedEventHandler,
    Microsoft::Web::WebView2::Win32::{ICoreWebView2Environment, ICoreWebView2Environment5},
};
use windows::{
    Win32::{
        Foundation::{HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        UI::WindowsAndMessaging::{
            MSG, MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS, MWMO_INPUTAVAILABLE,
            MsgWaitForMultipleObjectsEx, PM_NOREMOVE, PM_QS_SENDMESSAGE, PM_REMOVE, PeekMessageW,
            QS_ALLINPUT, QS_SENDMESSAGE, WM_QUIT,
        },
    },
    core::Interface,
};

use super::{BrowserError, BrowserState, dispatch};

/// How long shutdown waits for the browser process to save the profile and exit. It fits,
/// with the page host's own teardown, inside the app's 4.5 s session-end budget (#96).
pub(super) const EXIT_BOUND: Duration = Duration::from_secs(3);
/// Messages dispatched per wake, so a flood cannot starve the deadline check.
const MESSAGES_PER_WAKE: usize = 256;

/// Tracks whether the environment's browser process may still hold unsaved profile data.
pub(super) struct EngineWatch {
    running: Arc<AtomicBool>,
    environment: ICoreWebView2Environment5,
    token: i64,
}

impl EngineWatch {
    /// Subscribes to `BrowserProcessExited`; the process counts as running only once it serves a page.
    fn register(environment: &ICoreWebView2Environment) -> Result<Self, BrowserError> {
        let environment = environment
            .cast::<ICoreWebView2Environment5>()
            .map_err(|_| BrowserError::EventRegistration)?;
        let running = Arc::new(AtomicBool::new(false));
        let observed = running.clone();
        let handler = BrowserProcessExitedEventHandler::create(Box::new(move |_, _| {
            observed.store(false, Ordering::Release);
            Ok(())
        }));
        let mut token = 0;
        unsafe { environment.add_BrowserProcessExited(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)?;
        Ok(Self {
            running,
            environment,
            token,
        })
    }

    /// A page was created, so a browser process now holds profile data.
    pub(super) fn serving(&self) {
        self.running.store(true, Ordering::Release);
    }

    fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

impl Drop for EngineWatch {
    fn drop(&mut self) {
        // Unsubscribe before the environment goes, so no late exit reaches a retired watch.
        if unsafe { self.environment.remove_BrowserProcessExited(self.token) }.is_err() {
            eprintln!("Syrinx could not stop watching its page engine's exit.");
        }
    }
}

/// What the shutdown wait for the browser process observed.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum ExitWait {
    /// No page was served since the last exit, so there was nothing to save.
    Idle,
    Exited(Duration),
    TimedOut,
    Failed(BrowserError),
}

impl ExitWait {
    /// One structural line for the log; it never names a page.
    pub(super) fn describe(&self) -> Option<String> {
        match self {
            Self::Idle => None,
            Self::Exited(elapsed) => Some(format!(
                "Syrinx's page engine saved and exited {} ms after shutdown.",
                elapsed.as_millis()
            )),
            Self::TimedOut => Some(format!(
                "Syrinx's page engine did not exit within {} s of shutdown; recent cookies may not be saved.",
                EXIT_BOUND.as_secs()
            )),
            Self::Failed(error) => Some(format!(
                "Syrinx stopped waiting for its page engine to exit: {error}"
            )),
        }
    }
}

impl BrowserState {
    /// Installs an environment and watches its browser process; browsing works without the watch.
    pub(super) fn install_environment(&mut self, environment: ICoreWebView2Environment) {
        self.engine = match EngineWatch::register(&environment) {
            Ok(watch) => Some(watch),
            Err(error) => {
                eprintln!(
                    "Syrinx cannot watch its page engine, so shutdown will not wait: {error}"
                );
                None
            }
        };
        self.environment = Some(environment);
    }

    /// Closes every page, then pumps messages until the browser process has exited or the bound passes.
    pub(super) fn release_engine(&mut self) -> ExitWait {
        let watch = self.engine.take();
        let running =
            self.environment.is_some() && watch.as_ref().is_some_and(EngineWatch::running);
        // Closing the last controller starts the browser process's orderly, saving exit.
        self.popups.clear();
        self.pages.clear();
        let Some(watch) = watch.filter(|_| running) else {
            return ExitWait::Idle;
        };
        wait_for_exit(EXIT_BOUND, || !watch.running(), pump_messages, Instant::now)
    }
}

/// Pumps until `exited` holds or `bound` passes; the clock is a parameter so tests can drive it.
pub(super) fn wait_for_exit(
    bound: Duration,
    mut exited: impl FnMut() -> bool,
    mut pump: impl FnMut(Duration) -> Result<(), BrowserError>,
    mut now: impl FnMut() -> Instant,
) -> ExitWait {
    let started = now();
    loop {
        let elapsed = now().saturating_duration_since(started);
        if exited() {
            return ExitWait::Exited(elapsed);
        }
        let remaining = bound.saturating_sub(elapsed);
        if remaining.is_zero() {
            return ExitWait::TimedOut;
        }
        if let Err(error) = pump(remaining) {
            return ExitWait::Failed(error);
        }
    }
}

/// Sleeps until a message arrives or `remaining` passes, then dispatches what arrived.
fn pump_messages(remaining: Duration) -> Result<(), BrowserError> {
    let woke = unsafe {
        MsgWaitForMultipleObjectsEx(
            None,
            whole_millis(remaining),
            QS_ALLINPUT,
            MWMO_INPUTAVAILABLE,
        )
    };
    if woke == WAIT_FAILED {
        return Err(BrowserError::MessageLoop);
    }
    let mut message = MSG::default();
    for _ in 0..MESSAGES_PER_WAKE {
        if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            break;
        }
        // A quit only says the owner is gone; the engine still needs its time to save.
        if message.message != WM_QUIT {
            dispatch(message);
        }
    }
    Ok(())
}

/// Waits for `thread` to exit while servicing only messages other threads send to this one,
/// so a worker whose windows have parents here can still finish. True when it exited.
pub(super) fn wait_for_thread(thread: HANDLE, bound: Duration) -> Result<bool, BrowserError> {
    let deadline = Instant::now() + bound;
    loop {
        // Deliver messages already sent, which the wait below would not count as new input.
        service_sent_messages();
        let remaining = deadline.saturating_duration_since(Instant::now());
        let woke = unsafe {
            MsgWaitForMultipleObjectsEx(
                Some(&[thread]),
                whole_millis(remaining),
                QS_SENDMESSAGE,
                MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS(0),
            )
        };
        if woke == WAIT_OBJECT_0 {
            return Ok(true);
        }
        if woke == WAIT_TIMEOUT {
            return Ok(false);
        }
        if woke.0 == WAIT_OBJECT_0.0 + 1 {
            continue;
        }
        return Err(BrowserError::MessageLoop);
    }
}

/// Delivers pending sent messages without removing or dispatching posted ones.
fn service_sent_messages() {
    let mut message = MSG::default();
    let _posted_waiting =
        unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE) };
}

/// Rounds up, so a sub-millisecond remainder still sleeps instead of spinning.
fn whole_millis(duration: Duration) -> u32 {
    let millis = duration.as_nanos().div_ceil(1_000_000);
    u32::try_from(millis).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// A clock that moves only when the fake pump says it slept.
    struct FakeClock {
        start: Instant,
        offset: Cell<Duration>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                start: Instant::now(),
                offset: Cell::new(Duration::ZERO),
            }
        }
        fn now(&self) -> Instant {
            self.start + self.offset.get()
        }
        fn advance(&self, by: Duration) {
            self.offset.set(self.offset.get() + by);
        }
    }

    #[test]
    fn an_exit_during_the_wait_ends_it_with_the_elapsed_time() {
        let clock = FakeClock::new();
        let pumps = Cell::new(0);
        let wait = wait_for_exit(
            EXIT_BOUND,
            || pumps.get() == 3,
            |_| {
                pumps.set(pumps.get() + 1);
                clock.advance(Duration::from_millis(100));
                Ok(())
            },
            || clock.now(),
        );
        assert_eq!(wait, ExitWait::Exited(Duration::from_millis(300)));
    }

    #[test]
    fn an_engine_that_never_exits_times_out_at_the_bound_without_oversleeping() {
        let clock = FakeClock::new();
        let mut requested = Vec::new();
        let wait = wait_for_exit(
            Duration::from_millis(250),
            || false,
            |remaining| {
                requested.push(remaining);
                // The pump sleeps for at most what it was offered.
                clock.advance(remaining.min(Duration::from_millis(100)));
                Ok(())
            },
            || clock.now(),
        );
        assert_eq!(wait, ExitWait::TimedOut);
        assert_eq!(
            requested,
            [250, 150, 50].map(Duration::from_millis),
            "each sleep is bounded by the time left"
        );
    }

    #[test]
    fn an_engine_already_gone_is_not_pumped() {
        let clock = FakeClock::new();
        let wait = wait_for_exit(
            EXIT_BOUND,
            || true,
            |_| panic!("an exited engine needs no messages"),
            || clock.now(),
        );
        assert_eq!(wait, ExitWait::Exited(Duration::ZERO));
    }

    #[test]
    fn a_failed_message_wait_stops_waiting_and_says_why() {
        let clock = FakeClock::new();
        let wait = wait_for_exit(
            EXIT_BOUND,
            || false,
            |_| Err(BrowserError::MessageLoop),
            || clock.now(),
        );
        assert_eq!(wait, ExitWait::Failed(BrowserError::MessageLoop));
        assert!(
            wait.describe()
                .is_some_and(|line| line.contains("stopped waiting"))
        );
    }

    #[test]
    fn only_a_served_engine_is_reported_and_no_report_names_a_page() {
        assert_eq!(ExitWait::Idle.describe(), None);
        let timed_out = ExitWait::TimedOut.describe().expect("timeouts are logged");
        assert!(timed_out.contains("did not exit within 3 s"));
        let exited = ExitWait::Exited(Duration::from_millis(42))
            .describe()
            .expect("exits are logged");
        assert!(exited.contains("42 ms"));
        assert!(!exited.contains("://"));
    }

    #[test]
    fn millisecond_rounding_never_turns_a_remainder_into_a_spin() {
        assert_eq!(whole_millis(Duration::from_nanos(1)), 1);
        assert_eq!(whole_millis(Duration::from_millis(5)), 5);
        assert_eq!(whole_millis(Duration::ZERO), 0);
        assert_eq!(whole_millis(Duration::from_secs(u64::MAX)), u32::MAX);
    }
}

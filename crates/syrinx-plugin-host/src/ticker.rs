//! The one epoch ticker per engine: it advances the epoch only while a plugin call is in
//! flight and waits on a condition variable otherwise, so an idle Syrinx never wakes for it (D6).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use wasmtime::Engine;

use crate::limits::EPOCH_TICK;

/// Owns the ticker thread; dropping it stops and joins the thread.
pub(crate) struct EpochTicker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    engine: Engine,
    state: Mutex<State>,
    wake: Condvar,
    ticks: AtomicU64,
}

#[derive(Default)]
struct State {
    in_flight: usize,
    /// When the next tick is due; `None` while no call is in flight.
    next_tick: Option<Instant>,
    stop: bool,
}

/// Held for the length of one call; the ticker runs while any is held.
pub(crate) struct InFlight<'ticker> {
    ticker: &'ticker EpochTicker,
}

impl EpochTicker {
    /// Start the parked ticker thread. Without it no deadline could expire, so failing to
    /// start it fails the runtime.
    pub(crate) fn start(engine: &Engine) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            engine: engine.clone(),
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            ticks: AtomicU64::new(0),
        });
        let worker = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("syrinx-plugin-epoch".into())
            .spawn(move || worker.run())?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Mark a call in flight until the guard drops, starting the ticker if it was idle.
    pub(crate) fn in_flight(&self) -> InFlight<'_> {
        let mut state = self.shared.lock();
        state.in_flight += 1;
        if state.in_flight == 1 {
            state.next_tick = Some(Instant::now() + EPOCH_TICK);
            self.shared.wake.notify_all();
        }
        InFlight { ticker: self }
    }

    /// How many times the epoch has advanced; flat while every plugin is idle.
    pub(crate) fn ticks(&self) -> u64 {
        self.shared.ticks.load(Ordering::Relaxed)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut state = self.ticker.shared.lock();
        state.in_flight -= 1;
        if state.in_flight == 0 {
            state.next_tick = None;
            self.ticker.shared.wake.notify_all();
        }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_all();
        let Some(thread) = self.thread.take() else {
            return;
        };
        if thread.join().is_err() {
            eprintln!("the plugin epoch ticker thread panicked");
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Tick on time while a call is in flight; otherwise wait without a timeout.
    fn run(&self) {
        let mut state = self.lock();
        loop {
            if state.stop {
                return;
            }
            let Some(next_tick) = state.next_tick else {
                state = self
                    .wake
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
                continue;
            };
            let now = Instant::now();
            if now < next_tick {
                state = self
                    .wake
                    .wait_timeout(state, next_tick - now)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
                continue;
            }
            self.engine.increment_epoch();
            self.ticks.fetch_add(1, Ordering::Relaxed);
            state.next_tick = Some(now + EPOCH_TICK);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_ticker_ticks_only_while_a_call_is_in_flight() {
        let engine = Engine::default();
        let ticker = EpochTicker::start(&engine).expect("the ticker starts");
        thread::sleep(EPOCH_TICK * 5);
        assert_eq!(ticker.ticks(), 0, "an idle ticker never ticks");
        {
            let _call = ticker.in_flight();
            thread::sleep(EPOCH_TICK * 5);
        }
        let after_call = ticker.ticks();
        assert!(after_call >= 2, "ticked {after_call} times during the call");
        thread::sleep(EPOCH_TICK * 5);
        assert_eq!(ticker.ticks(), after_call, "the ticker parks again");
    }

    #[test]
    fn overlapping_calls_keep_one_tick_rate() {
        let engine = Engine::default();
        let ticker = EpochTicker::start(&engine).expect("the ticker starts");
        let started = Instant::now();
        {
            let _first = ticker.in_flight();
            let _second = ticker.in_flight();
            thread::sleep(Duration::from_millis(100));
        }
        let elapsed_ticks = started.elapsed().as_millis() / EPOCH_TICK.as_millis();
        assert!(u128::from(ticker.ticks()) <= elapsed_ticks + 1);
    }
}

//! The session loop's side of pulling: it moves pulls on between the owner's commands, never
//! because the owner typed in the palette (D9), so a slow plugin never holds up browsing.

use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use syrinx_store::Store;

use super::{FeedSource, Sinks};
use crate::session::{
    BACKUP_CHECK_INTERVAL, SessionCommand, SessionError, SessionEvent, Worker, emit_recoverable,
    utc_seconds,
};

/// How often the session looks for a plugin's answer while a feed is being pulled.
const ANSWER_POLL: Duration = Duration::from_millis(20);

/// How long pulling may hold the session's idle work off: the pace that work keeps on its own
/// once the session is quiet, so a busy plugin never makes it run less often than that.
const IDLE_BOUND: Duration = BACKUP_CHECK_INTERVAL;

/// Since when pulling has held the session's idle work off, if it has.
#[derive(Default)]
pub(in crate::session) struct IdleClock {
    busy_since: Option<Instant>,
}

impl IdleClock {
    /// Whether the idle work has waited `IDLE_BOUND` behind pulling; if so, the wait starts
    /// again from `now`. A plugin that always has more cannot hold it off for longer.
    fn overdue(&mut self, now: Instant) -> bool {
        let since = *self.busy_since.get_or_insert(now);
        if now.saturating_duration_since(since) < IDLE_BOUND {
            return false;
        }
        self.busy_since = Some(now);
        true
    }

    /// Pulling has nothing left to do, so it holds no idle work off.
    fn release(&mut self) {
        self.busy_since = None;
    }
}

/// Whether a command is the owner typing in the palette: no pull starts or moves on after it.
pub(in crate::session) fn is_typing(command: &SessionCommand) -> bool {
    matches!(command, SessionCommand::Query { .. })
}

impl Worker {
    /// After one of the owner's commands, pulling moves on, unless the command was typing.
    pub(in crate::session) fn pull_after_command(
        &mut self,
        typed: bool,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        self.with_runtime(|worker, runtime| {
            worker.pull_after_command_from(typed, runtime, store, events)
        })
    }

    /// `pull_after_command` with any source, for the session's own tests, which stand in for
    /// the command arm by passing `is_typing` of the command they sent.
    pub(in crate::session) fn pull_after_command_from(
        &mut self,
        typed: bool,
        source: &mut dyn FeedSource,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        if typed {
            self.plugin_feeds.note_typing(Instant::now());
            return Ok(());
        }
        self.pump_feeds_from(source, store, events)
    }

    /// Hears the runtime's events and moves pulling on by one batch, with the plugin runtime
    /// the session started; nothing happens without one.
    pub(in crate::session) fn pump_plugin_feeds(
        &mut self,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        self.with_runtime(|worker, runtime| worker.pump_feeds_from(runtime, store, events))
    }

    /// The idle wake-up's part for plugin feeds: retries text a removal left in the index,
    /// then moves pulling on.
    pub(in crate::session) fn pull_at_idle(
        &mut self,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        self.plugin_feeds.retry_removals();
        self.pump_plugin_feeds(store, events)
    }

    /// The loop's wake-up while a pull is under way: moves pulling on by one batch, and first
    /// runs the idle work once it has waited `IDLE_BOUND` behind pulling, so a plugin that
    /// always has more never stops page-text expiry, a removal's retry or the backup.
    pub(in crate::session) fn pull_while_busy(
        &mut self,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        self.with_runtime(|worker, runtime| {
            worker.pull_while_busy_from(runtime, store, events, Instant::now())
        })
    }

    /// `pull_while_busy` with any source and clock, for the session's own tests.
    pub(in crate::session) fn pull_while_busy_from(
        &mut self,
        source: &mut dyn FeedSource,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
        now: Instant,
    ) -> Result<(), SessionError> {
        if self.plugin_feeds.idle_clock.overdue(now) {
            self.idle_housekeeping(store, events)?;
            self.plugin_feeds.retry_removals();
        }
        self.pump_feeds_from(source, store, events)
    }

    /// Runs `step` with the session's plugin runtime as the feed source; nothing without one.
    /// The runtime is taken out and put back, so it and the worker can be borrowed together.
    fn with_runtime(
        &mut self,
        step: impl FnOnce(&mut Self, &mut dyn FeedSource) -> Result<(), SessionError>,
    ) -> Result<(), SessionError> {
        let Some(mut plugins) = self.plugins.take() else {
            return Ok(());
        };
        let stepped = step(self, &mut plugins.runtime);
        self.plugins = Some(plugins);
        stepped
    }

    /// `pump_plugin_feeds` with any source, for the session's own tests. A failed pull is
    /// shown to the owner and the loop goes on; the feed is pulled again when next announced.
    pub(in crate::session) fn pump_feeds_from(
        &mut self,
        source: &mut dyn FeedSource,
        store: &Store,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        self.survival.operation = "pulling plugin search feeds";
        self.report_plugin_events(source, events)?;
        let now = utc_seconds()?;
        let mut sinks = Sinks {
            store,
            index: self.page_text.index_mut().map(|index| index as _),
            catalogue: &mut self.catalogue,
            now,
            at: Instant::now(),
        };
        let pumped = self.plugin_feeds.pump(source, &mut sinks);
        if !self.plugin_feeds.busy(self.page_text.index_open()) {
            self.plugin_feeds.idle_clock.release();
        }
        let Err(error) = pumped else {
            return Ok(());
        };
        emit_recoverable(events, SessionError::PluginFeed(error))
    }

    /// Hands the runtime's events to the feeds; a failure among the rest is shown in the
    /// window, the others only logged. No reader in the session takes them yet; plugin tabs
    /// (#29) will. Each names its plugin and kind only, never a plugin's text.
    fn report_plugin_events(
        &mut self,
        source: &mut dyn FeedSource,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        for unread in self.plugin_feeds.notice(source.take_events()) {
            if unread.is_failure() {
                emit_recoverable(events, SessionError::PluginEvent(unread.to_string()))?;
            } else {
                eprintln!("Syrinx: plugin event without a reader: {unread}");
            }
        }
        Ok(())
    }

    /// Whether a pull should move on at the loop's next wake: one is under way, and the owner
    /// has not typed in the palette in the last moment.
    pub(in crate::session) fn plugin_feeds_busy(&self) -> bool {
        self.pull_waiting() && self.plugin_feeds.typing_quiet_for(Instant::now()).is_none()
    }

    /// The loop's wait, cut short while a pull is under way, or until typing has paused.
    pub(in crate::session) fn plugin_feed_wait(&self, wait: Duration) -> Duration {
        if !self.pull_waiting() {
            return wait;
        }
        let quiet = self.plugin_feeds.typing_quiet_for(Instant::now());
        wait.min(quiet.unwrap_or(ANSWER_POLL))
    }

    /// Whether pulling has work, whether or not the owner is typing.
    fn pull_waiting(&self) -> bool {
        let index_open = self.page_text.index_open();
        self.plugins.is_some() && self.plugin_feeds.busy(index_open)
    }
}

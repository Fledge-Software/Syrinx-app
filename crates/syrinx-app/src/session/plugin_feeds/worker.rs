//! The session loop's side of pulling: it moves pulls on between the owner's commands, never
//! because the owner typed in the palette (D9), so a slow plugin never holds up browsing.

use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use syrinx_store::Store;

use super::{FeedSource, Sinks};
use crate::session::{
    SessionCommand, SessionError, SessionEvent, Worker, emit_recoverable, utc_seconds,
};

/// How often the session looks for a plugin's answer while a feed is being pulled.
const ANSWER_POLL: Duration = Duration::from_millis(20);

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

    /// `pull_after_command` with any source, for the session's own tests.
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
        for unread in self.plugin_feeds.notice(source.take_events()) {
            // No reader in the session takes these yet; plugin tabs (#29) will. Each is
            // still reported, by plugin and kind only, never with a plugin's text.
            eprintln!("Syrinx: plugin event without a reader: {unread}");
        }
        let now = utc_seconds()?;
        let mut sinks = Sinks {
            store,
            index: self.page_text.index_mut().map(|index| index as _),
            catalogue: &mut self.catalogue,
            now,
        };
        let pumped = self.plugin_feeds.pump(source, &mut sinks);
        let Err(error) = pumped else {
            return Ok(());
        };
        emit_recoverable(events, SessionError::PluginFeed(error))
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

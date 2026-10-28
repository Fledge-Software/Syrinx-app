//! Keeps each tab's back and forward list in the worker's tab model and tells the window where
//! Back and Forward can reach (#126, owner 2026-10-28).

use futures::channel::mpsc::UnboundedSender;
use syrinx_core::{HistoryDirection, HistoryReport, TabId};

use super::{SessionError, SessionEvent, Worker, emit};

impl Worker {
    /// Follows a page's history report. A report for a tab that is no longer open is stale and
    /// changes nothing. The window hears only when the tab's reach changes.
    pub(super) fn observe_history(
        &mut self,
        id: TabId,
        report: &HistoryReport,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let before = self.model.history_reach(id);
        if self.model.observe_history(id, report).is_err() {
            return Ok(());
        }
        let reach = self.model.history_reach(id);
        if reach == before {
            return Ok(());
        }
        emit(events, SessionEvent::HistoryReach { id, reach })
    }

    /// Records where a Back, Forward or row jump the window is sending should land. The window
    /// sends the step whatever the list knows, so an unknown list never stops Back working.
    pub(super) fn start_history_jump(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
        steps: usize,
    ) {
        self.model.start_history_jump(id, direction, steps);
    }
}

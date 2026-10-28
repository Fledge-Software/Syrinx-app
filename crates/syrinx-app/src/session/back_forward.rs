//! Keeps each tab's back and forward list in the worker's tab model and tells the window where
//! Back and Forward can reach (#126, owner 2026-10-28).

use std::fmt;

use futures::channel::mpsc::UnboundedSender;
use syrinx_core::{
    HISTORY_ROW_LIMIT, HistoryDirection, HistoryReach, HistoryReport, TabError, TabId,
};

use super::{SessionError, SessionEvent, Worker, emit};

/// One row of a Back or Forward menu, copied out of the tab model for the window to draw.
#[derive(Clone, Eq, PartialEq)]
pub struct HistoryListRow {
    /// How many Back or Forward presses the row is.
    pub steps: usize,
    pub address: String,
    pub title: String,
}

impl fmt::Debug for HistoryListRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The address and title are the owner's browsing; never print them.
        formatter
            .debug_struct("HistoryListRow")
            .field("steps", &self.steps)
            .finish_non_exhaustive()
    }
}

impl Worker {
    /// Follows a page's history report. A report for a tab that is no longer open is stale and
    /// changes nothing. The window hears only when the tab's reach, or its engine's, changes.
    pub(super) fn observe_history(
        &mut self,
        id: TabId,
        report: &HistoryReport,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let before = self.reaches(id);
        match self.model.observe_history(id, report) {
            Ok(_) => {}
            // The tab closed after its page reported; the report is stale.
            Err(TabError::UnknownTab(_)) => return Ok(()),
            Err(error) => return Err(SessionError::Model(error)),
        }
        let (reach, engine) = self.reaches(id);
        if (reach, engine) == before {
            return Ok(());
        }
        emit(events, SessionEvent::HistoryReach { id, reach, engine })
    }

    /// Where tab `id`'s list reaches, then where its engine does.
    fn reaches(&self, id: TabId) -> (HistoryReach, HistoryReach) {
        (
            self.model.history_reach(id),
            self.model.history_engine_reach(id),
        )
    }

    /// Records where a Back, Forward or row jump the window is sending should land. The window
    /// sends the step whatever the list knows, so an unknown list never stops Back working.
    pub(super) fn start_history_jump(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
        steps: u32,
    ) {
        // A count past the address space has no row, so it plans nothing.
        let Ok(steps) = usize::try_from(steps) else {
            return;
        };
        self.model.start_history_jump(id, direction, steps);
    }

    /// Answers a menu's request with tab `id`'s rows in `direction`; a closed tab has none.
    pub(super) fn history_rows(
        &self,
        request_id: u64,
        id: TabId,
        direction: HistoryDirection,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let rows = self
            .model
            .history_rows(id, direction, HISTORY_ROW_LIMIT)
            .into_iter()
            .map(|row| HistoryListRow {
                steps: row.steps,
                address: row.address.to_owned(),
                title: row.title.to_owned(),
            })
            .collect();
        emit(events, SessionEvent::HistoryRows { request_id, rows })
    }
}

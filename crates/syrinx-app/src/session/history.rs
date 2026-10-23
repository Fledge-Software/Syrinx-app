//! Turns observed page navigation into durable visits and their search rows (P1-20).

mod tracker;

use std::time::Instant;

use syrinx_core::{TabId, TabKind, TabModelSnapshot};
use syrinx_search::{Catalogue, Entry, Target, Usage};
use syrinx_store::{HistoryEntry, Store};

use super::SessionError;
use tracker::{Action, VisitTracker};

pub(super) use tracker::TITLE_SETTLE;

/// One page callback's metadata, as the worker received it.
pub(super) struct Observation {
    pub(super) id: TabId,
    pub(super) title: Option<String>,
    pub(super) address: Option<String>,
}

/// Applies the tracker's decisions to the store and the tier-1 catalogue.
#[derive(Default)]
pub(super) struct HistoryRecorder {
    tracker: VisitTracker,
}

impl HistoryRecorder {
    pub(super) fn restored(model: &TabModelSnapshot) -> Self {
        Self {
            tracker: VisitTracker::restored(model),
        }
    }

    pub(super) fn retain(&mut self, model: &TabModelSnapshot) {
        self.tracker.retain(model);
    }

    pub(super) fn has_pending_titles(&self) -> bool {
        self.tracker.has_pending_titles()
    }

    /// Holds a settled title for an already stored visit, so a test can choose the entry
    /// its retitle names, such as one the store refuses.
    #[cfg(test)]
    pub(super) fn hold_settled_title(
        &mut self,
        id: TabId,
        recorded: &syrinx_store::RecordedVisit,
        title: String,
        at: Instant,
    ) {
        self.tracker.title(id, title, at);
        self.tracker.recorded(id, recorded, at);
    }

    /// A clear removed these addresses and may have removed any recent visit.
    pub(super) fn history_cleared(&mut self, removed: &[std::num::NonZeroU64]) {
        self.tracker.history_cleared(removed);
    }

    /// Records a visit when a live web tab reports a new document address, and holds titles.
    pub(super) fn observe(
        &mut self,
        store: &Store,
        catalogue: &mut Catalogue,
        model: &TabModelSnapshot,
        observation: Observation,
        at: Instant,
        now: impl Fn() -> Result<i64, SessionError>,
    ) -> Result<(), SessionError> {
        let Observation { id, title, address } = observation;
        if !is_live_web(model, id) {
            return Ok(());
        }
        let Some(address) = address else {
            if let Some(title) = title {
                self.tracker.title(id, title, at);
            }
            return Ok(());
        };
        let actions = self.tracker.address(id, address, title, at);
        self.apply(store, catalogue, id, actions, at, &now)
    }

    /// A failed navigation retracts the visit its address change recorded.
    pub(super) fn navigation(
        &mut self,
        store: &Store,
        catalogue: &mut Catalogue,
        id: TabId,
        success: bool,
        at: Instant,
        now: impl Fn() -> Result<i64, SessionError>,
    ) -> Result<(), SessionError> {
        let actions = self.tracker.navigation(id, success).into_iter().collect();
        self.apply(store, catalogue, id, actions, at, &now)
    }

    /// Writes titles that have settled, or all held titles when `force` is set.
    pub(super) fn settle(
        &mut self,
        store: &Store,
        catalogue: &mut Catalogue,
        at: Instant,
        force: bool,
        now: impl Fn() -> Result<i64, SessionError>,
    ) -> Result<(), SessionError> {
        for action in self.tracker.settle(at, force) {
            self.apply_one(store, catalogue, None, action, at, &now)?;
        }
        Ok(())
    }

    fn apply(
        &mut self,
        store: &Store,
        catalogue: &mut Catalogue,
        id: TabId,
        actions: Vec<Action>,
        at: Instant,
        now: &dyn Fn() -> Result<i64, SessionError>,
    ) -> Result<(), SessionError> {
        for action in actions {
            self.apply_one(store, catalogue, Some(id), action, at, now)?;
        }
        Ok(())
    }

    fn apply_one(
        &mut self,
        store: &Store,
        catalogue: &mut Catalogue,
        tab: Option<TabId>,
        action: Action,
        at: Instant,
        now: &dyn Fn() -> Result<i64, SessionError>,
    ) -> Result<(), SessionError> {
        match action {
            Action::Record { address, title } => {
                let recorded = store
                    .record_visit(address, title, now()?)
                    .map_err(SessionError::Store)?;
                if let Some(tab) = tab {
                    self.tracker.recorded(tab, &recorded, at);
                }
                upsert(catalogue, &recorded.entry)
            }
            Action::Retitle { entry, title } => {
                // A duplicate title writes nothing.
                let unchanged = catalogue
                    .get(Target::History(entry))
                    .is_some_and(|row| row.title == title);
                if unchanged {
                    return Ok(());
                }
                let changed = store
                    .retitle_history(entry, title)
                    .map_err(SessionError::Store)?;
                self.update_row(catalogue, entry, changed)
            }
            Action::Retract { entry, visit } => {
                let remaining = store.remove_visit(visit).map_err(SessionError::Store)?;
                self.update_row(catalogue, entry, remaining)
            }
        }
    }

    fn update_row(
        &mut self,
        catalogue: &mut Catalogue,
        entry: std::num::NonZeroU64,
        current: Option<HistoryEntry>,
    ) -> Result<(), SessionError> {
        match current {
            Some(current) => upsert(catalogue, &current),
            None => {
                self.tracker.entry_removed(entry);
                catalogue
                    .remove(Target::History(entry))
                    .map(drop)
                    .map_err(SessionError::Search)
            }
        }
    }
}

fn is_live_web(model: &TabModelSnapshot, id: TabId) -> bool {
    model
        .tabs
        .iter()
        .any(|tab| tab.id == id && tab.kind == TabKind::Web)
}

fn upsert(catalogue: &mut Catalogue, entry: &HistoryEntry) -> Result<(), SessionError> {
    catalogue
        .upsert(history_entry(entry))
        .map_err(SessionError::Search)
}

/// The tier-1 search row for one remembered address.
pub(super) fn history_entry(entry: &HistoryEntry) -> Entry {
    Entry {
        target: Target::History(entry.id),
        title: entry.title.clone(),
        address: entry.address.clone(),
        usage: Usage {
            // Ranking only needs a bounded frequency; larger counts saturate.
            visits: u32::try_from(entry.visit_count).unwrap_or(u32::MAX),
            last_used: Some(entry.last_visited),
        },
        open: false,
    }
}

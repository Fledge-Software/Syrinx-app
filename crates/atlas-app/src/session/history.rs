//! Turns observed page navigation into durable visits and their search rows (P1-20).

use std::{collections::HashMap, num::NonZeroU64};

use atlas_core::{TabId, TabKind, TabModelSnapshot};
use atlas_search::{Catalogue, Entry, Target, Usage};
use atlas_store::{HistoryEntry, Store};

use super::SessionError;

/// The address each live web tab last reported, and the history entry it was recorded as.
struct Cursor {
    address: String,
    entry: Option<NonZeroU64>,
}

/// One page callback's metadata, as the worker received it.
pub(super) struct Observation {
    pub(super) id: TabId,
    pub(super) title: Option<String>,
    pub(super) address: Option<String>,
}

#[derive(Default)]
pub(super) struct HistoryRecorder {
    cursors: HashMap<TabId, Cursor>,
}

impl HistoryRecorder {
    /// Restored tabs reload their saved address at startup; that reload is not a new visit.
    pub(super) fn restored(model: &TabModelSnapshot) -> Self {
        let cursors = model
            .tabs
            .iter()
            .filter(|tab| tab.kind == TabKind::Web)
            .map(|tab| {
                let cursor = Cursor {
                    address: tab.metadata.address.clone(),
                    entry: None,
                };
                (tab.id, cursor)
            })
            .collect();
        Self { cursors }
    }

    /// Forgets closed tabs, so their late callbacks cannot record anything.
    pub(super) fn retain(&mut self, model: &TabModelSnapshot) {
        self.cursors
            .retain(|id, _| model.tabs.iter().any(|tab| tab.id == *id));
    }

    /// Records a visit when a live web tab reports a new address, or retitles its last visit.
    pub(super) fn observe(
        &mut self,
        store: &Store,
        catalogue: &mut Catalogue,
        model: &TabModelSnapshot,
        observation: Observation,
        now: impl FnOnce() -> Result<i64, SessionError>,
    ) -> Result<(), SessionError> {
        let Observation { id, title, address } = observation;
        let live_web = model
            .tabs
            .iter()
            .any(|tab| tab.id == id && tab.kind == TabKind::Web);
        if !live_web {
            return Ok(());
        }
        if let Some(address) = address
            && self.is_new_address(id, &address)
        {
            return self.record(store, catalogue, id, address, title, now);
        }
        let Some(title) = title else {
            return Ok(());
        };
        self.retitle(store, catalogue, id, title)
    }

    fn is_new_address(&self, id: TabId, address: &str) -> bool {
        // Repeated or replayed callbacks for the current address are the same visit.
        self.cursors
            .get(&id)
            .is_none_or(|cursor| cursor.address != address)
    }

    fn record(
        &mut self,
        store: &Store,
        catalogue: &mut Catalogue,
        id: TabId,
        address: String,
        title: Option<String>,
        now: impl FnOnce() -> Result<i64, SessionError>,
    ) -> Result<(), SessionError> {
        // Home and other non-web addresses move the cursor but are not history.
        let cursor = self.cursors.entry(id).or_insert(Cursor {
            address: String::new(),
            entry: None,
        });
        cursor.address.clone_from(&address);
        cursor.entry = None;
        if !is_history_address(&address) {
            return Ok(());
        }
        let entry = store
            .record_visit(address, title.unwrap_or_default(), now()?)
            .map_err(SessionError::Store)?;
        cursor.entry = Some(entry.id);
        catalogue
            .upsert(history_entry(&entry))
            .map_err(SessionError::Search)
    }

    fn retitle(
        &mut self,
        store: &Store,
        catalogue: &mut Catalogue,
        id: TabId,
        title: String,
    ) -> Result<(), SessionError> {
        let Some(cursor) = self.cursors.get_mut(&id) else {
            return Ok(());
        };
        let Some(entry_id) = cursor.entry else {
            return Ok(());
        };
        // Skip unchanged and empty titles; a duplicate callback writes nothing.
        let unchanged = catalogue
            .get(Target::History(entry_id))
            .is_some_and(|entry| entry.title == title);
        if title.is_empty() || unchanged {
            return Ok(());
        }
        match store
            .retitle_history(entry_id, title)
            .map_err(SessionError::Store)?
        {
            Some(entry) => catalogue
                .upsert(history_entry(&entry))
                .map_err(SessionError::Search),
            None => {
                cursor.entry = None;
                Ok(())
            }
        }
    }
}

/// History keeps only pages loaded from the web, never Home or local schemes.
fn is_history_address(address: &str) -> bool {
    address.starts_with("https://") || address.starts_with("http://")
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

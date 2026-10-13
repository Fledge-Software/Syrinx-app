//! Decides, without storage, which page callbacks are visits and which address a title names.

use std::{
    collections::HashMap,
    num::NonZeroU64,
    time::{Duration, Instant},
};

use syrinx_core::{TabId, TabKind, TabModelSnapshot};
use syrinx_store::RecordedVisit;

/// A title is held this long before it is trusted to name the tab's current address,
/// because a page can report its next title just before its next address.
pub(in crate::session) const TITLE_SETTLE: Duration = Duration::from_millis(300);

/// Returning to an address within this window by script (`pushState`) is the same visit.
pub(super) const REVISIT_WINDOW: Duration = Duration::from_secs(10);

/// One storage change the recorder must make.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Action {
    Record {
        address: String,
        title: String,
    },
    Retitle {
        entry: NonZeroU64,
        title: String,
    },
    Retract {
        entry: NonZeroU64,
        visit: NonZeroU64,
    },
}

struct PendingTitle {
    text: String,
    at: Instant,
}

struct RecentAddress {
    /// The address without its fragment.
    key: String,
    entry: NonZeroU64,
    at: Instant,
}

/// What one live web tab last reported, and the visit that report became.
#[derive(Default)]
struct Cursor {
    address: String,
    entry: Option<NonZeroU64>,
    /// The newest visit, retractable until its navigation is reported successful.
    visit: Option<NonZeroU64>,
    title: Option<PendingTitle>,
    recent: Vec<RecentAddress>,
}

#[derive(Default)]
pub(super) struct VisitTracker {
    cursors: HashMap<TabId, Cursor>,
}

impl VisitTracker {
    /// Restored tabs reload their saved address at startup; that reload is not a new visit.
    pub(super) fn restored(model: &TabModelSnapshot) -> Self {
        let cursors = model
            .tabs
            .iter()
            .filter(|tab| tab.kind == TabKind::Web)
            .map(|tab| {
                let cursor = Cursor {
                    address: tab.metadata.address.clone(),
                    ..Cursor::default()
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

    pub(super) fn has_pending_titles(&self) -> bool {
        self.cursors.values().any(|cursor| cursor.title.is_some())
    }

    /// Holds a reported title until it settles or the tab's next address claims it.
    pub(super) fn title(&mut self, id: TabId, text: String, at: Instant) {
        self.cursors.entry(id).or_default().title = Some(PendingTitle { text, at });
    }

    /// Handles a reported address, with any title reported in the same callback.
    pub(super) fn address(
        &mut self,
        id: TabId,
        address: String,
        title: Option<String>,
        at: Instant,
    ) -> Vec<Action> {
        let cursor = self.cursors.entry(id).or_default();
        if cursor.address == address {
            // A replayed address is the same visit; only its title may be new.
            if let Some(text) = title {
                cursor.title = Some(PendingTitle { text, at });
            }
            return Vec::new();
        }
        let (mut actions, carried) = cursor.claim_title(at);
        let title = title.or(carried);
        let same_document = without_fragment(&cursor.address) == without_fragment(&address);
        cursor.address = address;
        if same_document {
            // A fragment change stays on the current visit.
            cursor.title = title.map(|text| PendingTitle { text, at });
            return actions;
        }
        cursor.entry = None;
        cursor.visit = None;
        if !is_history_address(&cursor.address) {
            return actions;
        }
        if let Some(entry) = cursor.recent_entry(at) {
            // A scripted return to a recent address is not another visit.
            cursor.entry = Some(entry);
            cursor.title = title.map(|text| PendingTitle { text, at });
            return actions;
        }
        actions.push(Action::Record {
            address: cursor.address.clone(),
            title: title.unwrap_or_default(),
        });
        actions
    }

    /// Notes the stored visit so later titles and a failed navigation reach it.
    pub(super) fn recorded(&mut self, id: TabId, recorded: &RecordedVisit, at: Instant) {
        let Some(cursor) = self.cursors.get_mut(&id) else {
            return;
        };
        cursor.entry = Some(recorded.entry.id);
        cursor.visit = Some(recorded.visit);
        cursor.recent.push(RecentAddress {
            key: without_fragment(&cursor.address).to_owned(),
            entry: recorded.entry.id,
            at,
        });
    }

    /// A completed navigation confirms the newest visit; a failed one retracts it.
    pub(super) fn navigation(&mut self, id: TabId, success: bool) -> Option<Action> {
        let cursor = self.cursors.get_mut(&id)?;
        let visit = cursor.visit.take()?;
        if success {
            return None;
        }
        let entry = cursor.entry.take()?;
        cursor.title = None;
        cursor.recent.retain(|recent| recent.entry != entry);
        Some(Action::Retract { entry, visit })
    }

    /// Applies titles that have settled, or every held title when `force` is set.
    pub(super) fn settle(&mut self, at: Instant, force: bool) -> Vec<Action> {
        let mut actions = Vec::new();
        for cursor in self.cursors.values_mut() {
            let settled = cursor.title.as_ref().is_some_and(|title| {
                force || at.saturating_duration_since(title.at) >= TITLE_SETTLE
            });
            if !settled {
                continue;
            }
            let title = cursor.title.take();
            actions.extend(cursor.retitle(title));
        }
        actions
    }

    /// After a clear no held visit can be retracted, since it may be gone, and removed
    /// entries can no longer take titles or absorb a scripted return.
    pub(super) fn history_cleared(&mut self, removed: &[NonZeroU64]) {
        for cursor in self.cursors.values_mut() {
            cursor.visit = None;
            cursor.recent.clear();
            if cursor.entry.is_some_and(|entry| removed.contains(&entry)) {
                cursor.entry = None;
            }
        }
    }

    /// The entry no longer exists, so later titles have nothing to name.
    pub(super) fn entry_removed(&mut self, entry: NonZeroU64) {
        for cursor in self.cursors.values_mut() {
            if cursor.entry == Some(entry) {
                cursor.entry = None;
                cursor.visit = None;
            }
            cursor.recent.retain(|recent| recent.entry != entry);
        }
    }
}

impl Cursor {
    /// Before the address changes: a settled title names the old address, a recent one
    /// is carried to the new address, which it most likely belongs to.
    fn claim_title(&mut self, at: Instant) -> (Vec<Action>, Option<String>) {
        let Some(title) = self.title.take() else {
            return (Vec::new(), None);
        };
        if at.saturating_duration_since(title.at) < TITLE_SETTLE {
            return (Vec::new(), Some(title.text));
        }
        (self.retitle(Some(title)).into_iter().collect(), None)
    }

    fn retitle(&self, title: Option<PendingTitle>) -> Option<Action> {
        let title = title?;
        let entry = self.entry?;
        if title.text.is_empty() {
            return None;
        }
        Some(Action::Retitle {
            entry,
            title: title.text,
        })
    }

    fn recent_entry(&mut self, at: Instant) -> Option<NonZeroU64> {
        self.recent
            .retain(|recent| at.saturating_duration_since(recent.at) < REVISIT_WINDOW);
        let key = without_fragment(&self.address);
        self.recent
            .iter()
            .find(|recent| recent.key == key)
            .map(|recent| recent.entry)
    }
}

/// History keeps only pages loaded from the web, never Home or local schemes.
fn is_history_address(address: &str) -> bool {
    address.starts_with("https://") || address.starts_with("http://")
}

fn without_fragment(address: &str) -> &str {
    address
        .split_once('#')
        .map_or(address, |(document, _)| document)
}

#[cfg(test)]
mod tests {
    use syrinx_core::{Activation, TabMetadata, TabModel};
    use syrinx_store::HistoryEntry;

    use super::*;

    const FIRST: &str = "https://tracker-first.example.test/";
    const SECOND: &str = "https://tracker-second.example.test/";

    fn tab() -> TabId {
        let mut model = TabModel::new();
        model
            .open(TabKind::Web, TabMetadata::default(), Activation::Foreground)
            .expect("open synthetic tab")
    }

    fn id(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).expect("synthetic identity is nonzero")
    }

    fn record(address: &str, title: &str) -> Action {
        Action::Record {
            address: address.to_owned(),
            title: title.to_owned(),
        }
    }

    fn stored(entry: u64, visit: u64, address: &str) -> RecordedVisit {
        RecordedVisit {
            visit: id(visit),
            entry: HistoryEntry {
                id: id(entry),
                address: address.to_owned(),
                title: String::new(),
                visit_count: 1,
                last_visited: 0,
            },
        }
    }

    /// Reports an address and stores any visit it produces under the given identities.
    fn visit(
        tracker: &mut VisitTracker,
        tab: TabId,
        address: &str,
        at: Instant,
        entry: u64,
    ) -> Vec<Action> {
        let actions = tracker.address(tab, address.to_owned(), None, at);
        if actions
            .iter()
            .any(|action| matches!(action, Action::Record { .. }))
        {
            tracker.recorded(tab, &stored(entry, entry, address), at);
        }
        actions
    }

    #[test]
    fn a_title_reported_just_before_its_address_names_the_new_visit() {
        let start = Instant::now();
        let mut tracker = VisitTracker::default();
        let tab = tab();
        assert_eq!(
            visit(&mut tracker, tab, FIRST, start, 1),
            [record(FIRST, "")]
        );
        // The single-page app reports its next title, then pushes its next address.
        tracker.title(
            tab,
            "Synthetic second".to_owned(),
            start + Duration::from_secs(2),
        );
        let actions = tracker.address(
            tab,
            SECOND.to_owned(),
            None,
            start + Duration::from_secs(2) + Duration::from_millis(50),
        );
        assert_eq!(actions, [record(SECOND, "Synthetic second")]);
        assert!(!tracker.has_pending_titles());
    }

    #[test]
    fn a_settled_title_names_the_current_address_before_the_next_one() {
        let start = Instant::now();
        let mut tracker = VisitTracker::default();
        let tab = tab();
        visit(&mut tracker, tab, FIRST, start, 1);
        tracker.title(tab, "Synthetic first".to_owned(), start);
        assert!(
            tracker
                .settle(start + Duration::from_millis(100), false)
                .is_empty()
        );
        assert!(tracker.has_pending_titles());
        let later = start + TITLE_SETTLE;
        let actions = tracker.address(tab, SECOND.to_owned(), None, later);
        assert_eq!(
            actions,
            [
                Action::Retitle {
                    entry: id(1),
                    title: "Synthetic first".to_owned()
                },
                record(SECOND, "")
            ]
        );
    }

    #[test]
    fn settling_applies_held_titles_once_and_force_applies_recent_ones() {
        let start = Instant::now();
        let mut tracker = VisitTracker::default();
        let tab = tab();
        visit(&mut tracker, tab, FIRST, start, 1);
        tracker.title(tab, "Synthetic first".to_owned(), start);
        let retitle = Action::Retitle {
            entry: id(1),
            title: "Synthetic first".to_owned(),
        };
        assert_eq!(
            tracker.settle(start + TITLE_SETTLE, false),
            std::slice::from_ref(&retitle)
        );
        assert!(tracker.settle(start + TITLE_SETTLE, false).is_empty());
        tracker.title(tab, "Synthetic first".to_owned(), start + TITLE_SETTLE);
        assert_eq!(tracker.settle(start + TITLE_SETTLE, true), [retitle]);
    }

    #[test]
    fn fragments_replays_and_quick_scripted_returns_are_not_new_visits() {
        let start = Instant::now();
        let mut tracker = VisitTracker::default();
        let tab = tab();
        visit(&mut tracker, tab, FIRST, start, 1);
        assert!(visit(&mut tracker, tab, FIRST, start, 9).is_empty());
        let fragment = format!("{FIRST}#section");
        assert!(visit(&mut tracker, tab, &fragment, start, 9).is_empty());
        assert_eq!(
            visit(&mut tracker, tab, SECOND, start, 2),
            [record(SECOND, "")]
        );
        // Returning within the window reuses the entry, so a later title still reaches it.
        let soon = start + Duration::from_secs(1);
        assert!(visit(&mut tracker, tab, FIRST, soon, 9).is_empty());
        tracker.title(tab, "Synthetic first".to_owned(), soon);
        assert_eq!(
            tracker.settle(soon + TITLE_SETTLE, false),
            [Action::Retitle {
                entry: id(1),
                title: "Synthetic first".to_owned()
            }]
        );
        // After the window, returning is a new visit again.
        let later = start + REVISIT_WINDOW + Duration::from_secs(1);
        assert_eq!(
            visit(&mut tracker, tab, SECOND, later, 2),
            [record(SECOND, "")]
        );
        // Home is not history, but it leaves the current visit.
        assert!(visit(&mut tracker, tab, "about:blank", later, 9).is_empty());
    }

    #[test]
    fn a_failed_navigation_retracts_only_its_unconfirmed_visit() {
        let start = Instant::now();
        let mut tracker = VisitTracker::default();
        let tab = tab();
        visit(&mut tracker, tab, FIRST, start, 1);
        assert_eq!(tracker.navigation(tab, true), None);
        assert_eq!(tracker.navigation(tab, false), None);
        visit(&mut tracker, tab, SECOND, start, 2);
        tracker.title(tab, "Synthetic error page".to_owned(), start);
        assert_eq!(
            tracker.navigation(tab, false),
            Some(Action::Retract {
                entry: id(2),
                visit: id(2)
            })
        );
        // The error page's title names nothing, and retrying the failed address is a new visit.
        assert!(tracker.settle(start + TITLE_SETTLE, true).is_empty());
        assert!(visit(&mut tracker, tab, FIRST, start, 9).is_empty());
        assert_eq!(
            visit(&mut tracker, tab, SECOND, start, 3),
            [record(SECOND, "")]
        );
    }

    #[test]
    fn restored_and_closed_tabs_record_nothing_from_old_reports() {
        let mut model = TabModel::new();
        let restored = model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: FIRST.to_owned(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .expect("open synthetic restored tab");
        let mut tracker = VisitTracker::restored(&model.snapshot());
        let start = Instant::now();
        assert!(visit(&mut tracker, restored, FIRST, start, 1).is_empty());
        model.close(restored).expect("close synthetic tab");
        tracker.retain(&model.snapshot());
        assert!(tracker.cursors.is_empty());
    }

    #[test]
    fn a_clear_forgets_retractable_visits_and_removed_entries() {
        let start = Instant::now();
        let mut tracker = VisitTracker::default();
        let tab = tab();
        assert_eq!(
            visit(&mut tracker, tab, FIRST, start, 1),
            [record(FIRST, "")]
        );
        tracker.history_cleared(&[id(1)]);

        // The cleared visit is not retracted, its title names nothing, and a quick return
        // to the address is recorded again rather than absorbed into the removed entry.
        assert_eq!(tracker.navigation(tab, false), None);
        tracker.title(tab, "Synthetic late".to_owned(), start);
        assert!(tracker.settle(start + TITLE_SETTLE, true).is_empty());
        assert_eq!(
            visit(&mut tracker, tab, SECOND, start, 2),
            [record(SECOND, "")]
        );
        assert_eq!(
            visit(&mut tracker, tab, FIRST, start, 3),
            [record(FIRST, "")]
        );
    }
}

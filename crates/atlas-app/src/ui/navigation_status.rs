//! Keeps native navigation feedback with its live tab until that tab closes.

use std::collections::{HashMap, HashSet};

use atlas_core::{TabId, TabModelSnapshot};
use atlas_platform::browser::BrowserEvent;

#[derive(Clone, Copy)]
enum Progress {
    Loading,
    Finishing,
    Completed(bool),
}

impl Progress {
    fn message(self) -> &'static str {
        match self {
            Self::Loading => "Loading…",
            Self::Finishing => "Finishing navigation…",
            Self::Completed(true) => "Page loaded.",
            Self::Completed(false) => "The page could not be loaded. Try reloading.",
        }
    }
}

#[derive(Default)]
pub(super) struct NavigationStatus {
    pages: HashMap<TabId, Progress>,
}

impl NavigationStatus {
    pub(super) fn observe(
        &mut self,
        event: &BrowserEvent,
        model: &TabModelSnapshot,
        loaded: &HashSet<TabId>,
        pending: &HashSet<TabId>,
        closing: bool,
    ) -> Option<&'static str> {
        if closing {
            return None;
        }
        let (raw_id, progress) = match event {
            BrowserEvent::Loading { id, loading: true } => (*id, Progress::Loading),
            BrowserEvent::Loading { id, loading: false } => (*id, Progress::Finishing),
            BrowserEvent::NavigationCompleted { id, success } => {
                (*id, Progress::Completed(*success))
            }
            BrowserEvent::Error { tab: Some(id), .. } => {
                let id = owned_page(*id, model, loaded, pending)?;
                self.forget(id);
                return None;
            }
            _ => return None,
        };
        let id = owned_page(raw_id, model, loaded, pending)?;
        // wry's Finished callback is not success evidence and may follow the actual result.
        if matches!(progress, Progress::Finishing)
            && !matches!(self.pages.get(&id), Some(Progress::Loading))
        {
            return None;
        }
        self.pages.insert(id, progress);
        // Events identify live tabs, not individual overlapping navigations within one tab.
        if model.active_id == Some(id) {
            return self.active_message(model);
        }
        None
    }

    pub(super) fn retain(&mut self, model: &TabModelSnapshot) {
        self.pages
            .retain(|id, _| model.tabs.iter().any(|tab| tab.id == *id));
    }

    pub(super) fn forget(&mut self, id: TabId) {
        self.pages.remove(&id);
    }

    pub(super) fn active_message(&self, model: &TabModelSnapshot) -> Option<&'static str> {
        self.pages
            .get(&model.active_id?)
            .map(|progress| progress.message())
    }
}

pub(super) fn owned_page(
    raw_id: u64,
    model: &TabModelSnapshot,
    loaded: &HashSet<TabId>,
    pending: &HashSet<TabId>,
) -> Option<TabId> {
    model.tabs.iter().find_map(|tab| {
        (tab.id.get() == raw_id && (loaded.contains(&tab.id) || pending.contains(&tab.id)))
            .then_some(tab.id)
    })
}

#[cfg(test)]
mod tests {
    use atlas_core::{Activation, TabKind, TabMetadata, TabModel};

    use super::*;

    fn page(model: &mut TabModel) -> TabId {
        model
            .open(TabKind::Web, TabMetadata::default(), Activation::Foreground)
            .expect("open synthetic page")
    }

    #[test]
    fn completion_is_truthful_in_both_native_callback_orders() {
        let mut model = TabModel::new();
        let id = page(&mut model);
        let snapshot = model.snapshot();
        let loaded = HashSet::from([id]);
        let pending = HashSet::new();
        for success in [false, true] {
            for finished_first in [false, true] {
                let mut status = NavigationStatus::default();
                assert_eq!(
                    status.observe(
                        &BrowserEvent::Loading {
                            id: id.get(),
                            loading: true
                        },
                        &snapshot,
                        &loaded,
                        &pending,
                        false
                    ),
                    Some("Loading…")
                );
                let finished = BrowserEvent::Loading {
                    id: id.get(),
                    loading: false,
                };
                let completed = BrowserEvent::NavigationCompleted {
                    id: id.get(),
                    success,
                };
                let expected = if success {
                    "Page loaded."
                } else {
                    "The page could not be loaded. Try reloading."
                };
                if finished_first {
                    assert_eq!(
                        status.observe(&finished, &snapshot, &loaded, &pending, false),
                        Some("Finishing navigation…")
                    );
                }
                assert_eq!(
                    status.observe(&completed, &snapshot, &loaded, &pending, false),
                    Some(expected)
                );
                assert_eq!(
                    status.observe(&finished, &snapshot, &loaded, &pending, false),
                    None
                );
                assert_eq!(status.active_message(&snapshot), Some(expected));
            }
        }
    }

    #[test]
    fn inactive_events_wait_for_selection_and_snapshots_retain_feedback() {
        let mut model = TabModel::new();
        let background = page(&mut model);
        let active = page(&mut model);
        let loaded = HashSet::from([background]);
        let pending = HashSet::from([active]);
        let mut status = NavigationStatus::default();
        assert_eq!(
            status.observe(
                &BrowserEvent::Loading {
                    id: active.get(),
                    loading: true
                },
                &model.snapshot(),
                &loaded,
                &pending,
                false
            ),
            Some("Loading…")
        );
        assert_eq!(
            status.observe(
                &BrowserEvent::NavigationCompleted {
                    id: background.get(),
                    success: false
                },
                &model.snapshot(),
                &loaded,
                &pending,
                false
            ),
            None
        );
        status.retain(&model.snapshot());
        assert_eq!(status.active_message(&model.snapshot()), Some("Loading…"));
        model
            .select(background)
            .expect("select synthetic background page");
        status.retain(&model.snapshot());
        assert_eq!(
            status.active_message(&model.snapshot()),
            Some("The page could not be loaded. Try reloading.")
        );
        let uncreated = page(&mut model);
        assert!(!loaded.contains(&uncreated) && !pending.contains(&uncreated));
        assert_eq!(status.active_message(&model.snapshot()), None);
    }

    #[test]
    fn closed_uncreated_and_unknown_pages_cannot_supply_feedback() {
        let mut model = TabModel::new();
        let original = page(&mut model);
        let loaded = HashSet::from([original]);
        let pending = HashSet::new();
        let mut status = NavigationStatus::default();
        assert_eq!(
            status.observe(
                &BrowserEvent::NavigationCompleted {
                    id: original.get(),
                    success: false
                },
                &model.snapshot(),
                &loaded,
                &pending,
                false
            ),
            Some("The page could not be loaded. Try reloading.")
        );
        model.close(original).expect("close synthetic page");
        let replacement = model
            .reopen_closed()
            .expect("reopen synthetic page")
            .expect("closed page exists");
        assert_ne!(original, replacement);
        status.retain(&model.snapshot());
        assert!(status.pages.is_empty());
        for id in [0, original.get(), replacement.get()] {
            assert_eq!(owned_page(id, &model.snapshot(), &loaded, &pending), None);
            assert_eq!(
                status.observe(
                    &BrowserEvent::Loading { id, loading: true },
                    &model.snapshot(),
                    &loaded,
                    &pending,
                    false
                ),
                None
            );
            assert_eq!(
                status.observe(
                    &BrowserEvent::NavigationCompleted { id, success: false },
                    &model.snapshot(),
                    &loaded,
                    &pending,
                    false
                ),
                None
            );
        }
        assert!(status.pages.is_empty());
    }

    #[test]
    fn new_navigation_and_failed_creation_discard_previous_feedback() {
        let mut model = TabModel::new();
        let id = page(&mut model);
        let snapshot = model.snapshot();
        let loaded = HashSet::new();
        let pending = HashSet::from([id]);
        let mut status = NavigationStatus::default();
        assert_eq!(
            status.observe(
                &BrowserEvent::NavigationCompleted {
                    id: id.get(),
                    success: false,
                },
                &snapshot,
                &loaded,
                &pending,
                false,
            ),
            Some("The page could not be loaded. Try reloading.")
        );
        assert_eq!(
            status.observe(
                &BrowserEvent::Loading {
                    id: id.get(),
                    loading: true
                },
                &snapshot,
                &loaded,
                &pending,
                false
            ),
            Some("Loading…")
        );
        status.forget(id);
        status.retain(&snapshot);
        assert_eq!(status.active_message(&snapshot), None);
    }

    #[test]
    fn shutdown_ignores_navigation_feedback() {
        let mut model = TabModel::new();
        let id = page(&mut model);
        let loaded = HashSet::from([id]);
        let pending = HashSet::new();
        let mut status = NavigationStatus::default();
        for event in [
            BrowserEvent::Loading {
                id: id.get(),
                loading: true,
            },
            BrowserEvent::Loading {
                id: id.get(),
                loading: false,
            },
            BrowserEvent::NavigationCompleted {
                id: id.get(),
                success: false,
            },
        ] {
            assert_eq!(
                status.observe(&event, &model.snapshot(), &loaded, &pending, true),
                None
            );
        }
        assert!(status.pages.is_empty());
    }

    #[test]
    fn page_errors_discard_only_their_own_success_and_late_finished_is_silent() {
        use atlas_platform::browser::BrowserError;

        let mut model = TabModel::new();
        let background = page(&mut model);
        let active = page(&mut model);
        let snapshot = model.snapshot();
        let loaded = HashSet::from([background, active]);
        let pending = HashSet::new();
        let mut status = NavigationStatus::default();
        for id in [background, active] {
            let expected = (id == active).then_some("Page loaded.");
            assert_eq!(
                status.observe(
                    &BrowserEvent::NavigationCompleted {
                        id: id.get(),
                        success: true
                    },
                    &snapshot,
                    &loaded,
                    &pending,
                    false
                ),
                expected
            );
        }

        // Background command errors cannot remove the active page's completed status.
        assert_eq!(
            status.observe(
                &BrowserEvent::Error {
                    tab: Some(background.get()),
                    error: BrowserError::Navigation
                },
                &snapshot,
                &loaded,
                &pending,
                false
            ),
            None
        );
        assert_eq!(status.active_message(&snapshot), Some("Page loaded."));
        model
            .select(background)
            .expect("select synthetic background page");
        assert_eq!(status.active_message(&model.snapshot()), None);
        model.select(active).expect("select synthetic active page");

        // A failed reload must not regain old success through a save or a trailing callback.
        assert_eq!(
            status.observe(
                &BrowserEvent::Error {
                    tab: Some(active.get()),
                    error: BrowserError::Navigation
                },
                &snapshot,
                &loaded,
                &pending,
                false
            ),
            None
        );
        status.retain(&snapshot);
        assert_eq!(status.active_message(&snapshot), None);
        assert_eq!(
            status.observe(
                &BrowserEvent::Loading {
                    id: active.get(),
                    loading: false
                },
                &snapshot,
                &loaded,
                &pending,
                false
            ),
            None
        );
        assert_eq!(status.active_message(&snapshot), None);
    }
}

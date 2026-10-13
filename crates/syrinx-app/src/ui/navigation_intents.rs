//! Retains explicit destinations until native page creation can consume them.

use std::collections::HashMap;

use syrinx_core::{TabId, TabModelSnapshot};

#[derive(Default)]
pub(super) struct NavigationIntents {
    destinations: HashMap<TabId, String>,
}

impl NavigationIntents {
    pub(super) fn remember(&mut self, id: TabId, destination: String) {
        self.destinations.insert(id, destination);
    }

    pub(super) fn desired_page(
        &mut self,
        snapshot: &TabModelSnapshot,
        browser_ready: bool,
        smoke: bool,
    ) -> Option<(TabId, String)> {
        // Closed logical tabs lose their intents even if they never had a native page.
        self.destinations
            .retain(|id, _| snapshot.tabs.iter().any(|tab| tab.id == *id));
        if !browser_ready {
            return None;
        }
        let active = snapshot.active_id?;
        let tab = snapshot.tabs.iter().find(|tab| tab.id == active)?;
        let destination = self
            .destinations
            .get(&active)
            .unwrap_or(&tab.metadata.address);
        if !smoke && destination == "about:blank" {
            return None;
        }
        Some((active, destination.clone()))
    }

    pub(super) fn creation_queued(&mut self, id: TabId, queued: bool) {
        if queued {
            self.destinations.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use syrinx_core::{Activation, TabKind, TabMetadata, TabModel};

    use super::*;

    const FIRST: &str = "https://first-intent.example.test/";
    const LATEST: &str = "https://latest-intent.example.test/";

    fn home() -> (TabModel, TabId) {
        let mut model = TabModel::new();
        let id = model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: "about:blank".to_owned(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .expect("open synthetic Home tab");
        (model, id)
    }

    fn observed(model: &mut TabModel, id: TabId, address: &str) {
        model
            .update_metadata(
                id,
                TabMetadata {
                    address: address.to_owned(),
                    ..TabMetadata::default()
                },
            )
            .expect("update synthetic tab metadata");
    }

    #[test]
    fn newest_submission_wins_before_blank_and_earlier_save_snapshots() {
        let (mut model, id) = home();
        let mut intents = NavigationIntents::default();
        intents.remember(id, FIRST.to_owned());
        intents.remember(id, LATEST.to_owned());

        // Neither submission has reached the UI through a saved model snapshot yet.
        assert_eq!(
            intents.desired_page(&model.snapshot(), true, false),
            Some((id, LATEST.to_owned()))
        );
        observed(&mut model, id, FIRST);
        assert_eq!(
            intents.desired_page(&model.snapshot(), true, false),
            Some((id, LATEST.to_owned()))
        );
        observed(&mut model, id, LATEST);
        assert_eq!(
            intents.desired_page(&model.snapshot(), true, false),
            Some((id, LATEST.to_owned()))
        );
    }

    #[test]
    fn delayed_browser_and_inactive_tab_keep_the_latest_destination() {
        let (mut model, id) = home();
        let mut intents = NavigationIntents::default();
        intents.remember(id, LATEST.to_owned());
        assert_eq!(intents.desired_page(&model.snapshot(), false, false), None);

        let other = model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: FIRST.to_owned(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .expect("open another synthetic tab");
        assert_eq!(
            intents.desired_page(&model.snapshot(), true, false),
            Some((other, FIRST.to_owned()))
        );
        model.select(id).expect("return to synthetic unloaded tab");
        assert_eq!(
            intents.desired_page(&model.snapshot(), true, false),
            Some((id, LATEST.to_owned()))
        );
    }

    #[test]
    fn failed_enqueue_preserves_intent_until_a_successful_creation() {
        let (mut model, id) = home();
        let mut intents = NavigationIntents::default();
        intents.remember(id, LATEST.to_owned());
        intents.creation_queued(id, false);
        observed(&mut model, id, FIRST);
        assert_eq!(
            intents.desired_page(&model.snapshot(), true, false),
            Some((id, LATEST.to_owned()))
        );

        intents.creation_queued(id, true);
        assert!(intents.destinations.is_empty());
        // Later page metadata is observed state, not a reason to recreate an old request.
        let redirected = "https://redirected-intent.example.test/";
        observed(&mut model, id, redirected);
        assert_eq!(
            intents.desired_page(&model.snapshot(), true, false),
            Some((id, redirected.to_owned()))
        );
    }

    #[test]
    fn closing_an_uncreated_tab_prunes_its_intent_before_browser_ready() {
        let (mut model, id) = home();
        let mut intents = NavigationIntents::default();
        intents.remember(id, LATEST.to_owned());
        model.close(id).expect("close synthetic unloaded tab");
        assert_eq!(intents.desired_page(&model.snapshot(), false, false), None);
        assert!(intents.destinations.is_empty());
        assert_eq!(intents.desired_page(&model.snapshot(), true, false), None);
    }

    #[test]
    fn explicit_home_supersedes_a_pending_page_without_changing_smoke_home() {
        let (mut model, id) = home();
        let mut intents = NavigationIntents::default();
        intents.remember(id, FIRST.to_owned());
        intents.remember(id, "about:blank".to_owned());
        observed(&mut model, id, FIRST);
        assert_eq!(intents.desired_page(&model.snapshot(), true, false), None);
        assert_eq!(
            intents.desired_page(&model.snapshot(), true, true),
            Some((id, "about:blank".to_owned()))
        );
    }

    fn open_web(model: &mut TabModel, address: &str) -> TabId {
        model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: address.to_owned(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .expect("open synthetic web tab")
    }

    #[test]
    fn restored_session_requests_only_the_active_page() {
        // A restored model has a background tab and an unloaded saved row; neither loads.
        let mut model = TabModel::new();
        let saved = open_web(&mut model, FIRST);
        model.save_page(saved).expect("save synthetic page");
        model.close(saved).expect("unload synthetic saved page");
        let background = open_web(&mut model, FIRST);
        let active = open_web(&mut model, LATEST);
        let restored = TabModel::from_snapshot(model.snapshot())
            .expect("restore synthetic snapshot")
            .snapshot();
        let mut intents = NavigationIntents::default();
        assert_eq!(
            intents.desired_page(&restored, true, false),
            Some((active, LATEST.to_owned()))
        );
        assert!(restored.tabs.iter().any(|tab| tab.id == background));
        assert_eq!(restored.saved_pages.len(), 1);
        assert!(restored.tabs.iter().all(|tab| tab.saved_page_id.is_none()));
    }
}

//! Resolves a tab or saved page the sidebar, palette or Favorites can open into one model action (D43).

use crate::{SavedPageId, TabId, TabModelSnapshot};

/// Something the owner can open from any surface.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ActivationTarget {
    Tab(TabId),
    Saved(SavedPageId),
}

/// The single action that opens a destination without duplicating a live page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Route {
    /// Switch to this live tab.
    Focus(TabId),
    /// Load this unloaded saved page into a new live instance.
    Load(SavedPageId),
}

/// Returns `None` when the destination no longer exists in this snapshot.
pub fn route(snapshot: &TabModelSnapshot, target: ActivationTarget) -> Option<Route> {
    match target {
        ActivationTarget::Tab(id) => snapshot
            .tabs
            .iter()
            .any(|tab| tab.id == id)
            .then_some(Route::Focus(id)),
        ActivationTarget::Saved(id) => {
            // An open saved page is focused, never loaded a second time.
            if let Some(tab) = live_instance(snapshot, id) {
                return Some(Route::Focus(tab));
            }
            snapshot
                .saved_pages
                .iter()
                .any(|page| page.id == id)
                .then_some(Route::Load(id))
        }
    }
}

/// The live tab currently showing a saved page, if any.
pub fn live_instance(snapshot: &TabModelSnapshot, saved: SavedPageId) -> Option<TabId> {
    snapshot
        .tabs
        .iter()
        .find(|tab| tab.saved_page_id == Some(saved))
        .map(|tab| tab.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Activation, TabKind, TabMetadata, TabModel};

    fn metadata(address: &str) -> TabMetadata {
        TabMetadata {
            title: "Synthetic destination".to_owned(),
            address: address.to_owned(),
            ..TabMetadata::default()
        }
    }

    #[test]
    fn saved_destinations_focus_their_live_instance_instead_of_loading_again() {
        let mut model = TabModel::new();
        let tab = model
            .open(
                TabKind::Web,
                metadata("https://destination.example.test/"),
                Activation::Foreground,
            )
            .expect("open synthetic tab");
        let saved = model.save_page(tab).expect("save synthetic tab");
        let snapshot = model.snapshot();
        assert_eq!(
            route(&snapshot, ActivationTarget::Saved(saved)),
            Some(Route::Focus(tab))
        );
        assert_eq!(
            route(&snapshot, ActivationTarget::Tab(tab)),
            Some(Route::Focus(tab))
        );
        assert_eq!(live_instance(&snapshot, saved), Some(tab));

        // Closing unloads the page; the saved row now loads a fresh instance.
        model.close(tab).expect("close synthetic tab");
        let snapshot = model.snapshot();
        assert_eq!(
            route(&snapshot, ActivationTarget::Saved(saved)),
            Some(Route::Load(saved))
        );
        assert_eq!(route(&snapshot, ActivationTarget::Tab(tab)), None);

        // Following the route through the model keeps exactly one live instance.
        let opened = model
            .open_saved(saved, Activation::Foreground)
            .expect("load saved page");
        let snapshot = model.snapshot();
        assert_eq!(
            route(&snapshot, ActivationTarget::Saved(saved)),
            Some(Route::Focus(opened))
        );
        let instances = snapshot
            .tabs
            .iter()
            .filter(|tab| tab.saved_page_id == Some(saved))
            .count();
        assert_eq!(instances, 1);
    }

    #[test]
    fn removed_destinations_have_no_route() {
        let mut model = TabModel::new();
        let tab = model
            .open(
                TabKind::Web,
                metadata("https://removed.example.test/"),
                Activation::Foreground,
            )
            .expect("open synthetic tab");
        let saved = model.save_page(tab).expect("save synthetic tab");
        model.remove_saved(saved).expect("remove saved row");
        let snapshot = model.snapshot();
        assert_eq!(route(&snapshot, ActivationTarget::Saved(saved)), None);
        // Removing the saved row leaves its page open as an ordinary tab (D43).
        assert_eq!(
            route(&snapshot, ActivationTarget::Tab(tab)),
            Some(Route::Focus(tab))
        );
    }
}

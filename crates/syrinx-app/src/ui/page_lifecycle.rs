//! Reconciles native page ownership only after the session acknowledges its durable model.

use std::collections::HashSet;

use syrinx_core::{TabId, TabModelSnapshot};

use crate::session::SessionCommand;

pub(super) fn observed_close(id: u64, closing: bool) -> Option<SessionCommand> {
    if closing {
        return None;
    }
    match TabId::new(id) {
        Ok(id) => Some(SessionCommand::ObserveClosed(id)),
        Err(_) => None,
    }
}

pub(super) fn created(
    id: TabId,
    pending: &mut HashSet<TabId>,
    loaded: &mut HashSet<TabId>,
) -> bool {
    if !pending.remove(&id) {
        return false;
    }
    loaded.insert(id);
    true
}

pub(super) fn removed(
    committed: &TabModelSnapshot,
    loaded: &HashSet<TabId>,
    pending: &HashSet<TabId>,
) -> Vec<TabId> {
    // A close request alone leaves both sets intact; only a saved model can release ownership.
    loaded
        .iter()
        .chain(pending.iter())
        .copied()
        .filter(|id| !committed.tabs.iter().any(|tab| tab.id == *id))
        .collect()
}

#[cfg(test)]
mod tests {
    use syrinx_core::{Activation, TabKind, TabMetadata, TabModel};

    use super::*;

    fn page() -> (TabModel, TabId) {
        let mut model = TabModel::new();
        let id = model
            .open(TabKind::Web, TabMetadata::default(), Activation::Foreground)
            .expect("open synthetic page");
        (model, id)
    }

    #[test]
    fn close_request_only_enqueues_and_shutdown_rejects_late_requests() {
        let (model, id) = page();
        let loaded = HashSet::from([id]);
        let pending = HashSet::new();
        assert!(matches!(
            observed_close(id.get(), false),
            Some(SessionCommand::ObserveClosed(closed)) if closed == id
        ));
        // While persistence is pending or failed, the old committed model retains the page.
        assert!(removed(&model.snapshot(), &loaded, &pending).is_empty());
        assert!(loaded.contains(&id));
        assert!(observed_close(id.get(), true).is_none());
        assert!(observed_close(0, false).is_none());
    }

    #[test]
    fn committed_close_before_created_cannot_resurrect_the_pending_page() {
        let (mut model, id) = page();
        let mut pending = HashSet::from([id]);
        let mut loaded = HashSet::new();
        assert!(removed(&model.snapshot(), &loaded, &pending).is_empty());
        model.close(id).expect("commit synthetic close");
        assert_eq!(removed(&model.snapshot(), &loaded, &pending), vec![id]);

        // Successful native Close enqueue removes pending ownership, as sync_pages does.
        pending.remove(&id);
        assert!(!created(id, &mut pending, &mut loaded));
        assert!(loaded.is_empty());
        assert!(removed(&model.snapshot(), &loaded, &pending).is_empty());
    }

    #[test]
    fn created_before_commit_retains_ownership_until_the_saved_close() {
        let (mut model, id) = page();
        let mut pending = HashSet::from([id]);
        let mut loaded = HashSet::new();
        assert!(created(id, &mut pending, &mut loaded));
        assert!(removed(&model.snapshot(), &loaded, &pending).is_empty());
        assert!(!created(id, &mut pending, &mut loaded));
        model.close(id).expect("commit synthetic close");
        assert_eq!(removed(&model.snapshot(), &loaded, &pending), vec![id]);
    }

    #[test]
    fn late_created_for_closed_saved_page_cannot_claim_its_replacement() {
        let (mut model, original) = page();
        let saved = model.save_page(original).expect("save synthetic page");
        let mut pending = HashSet::from([original]);
        let mut loaded = HashSet::new();

        // Reopen uses the surviving saved row but allocates a fresh native identity.
        model.close(original).expect("commit synthetic close");
        let replacement = model
            .reopen_closed()
            .expect("reopen synthetic saved page")
            .expect("closed synthetic page exists");
        assert_ne!(replacement, original);
        let committed = model.snapshot();
        assert_eq!(committed.saved_pages.len(), 1);
        assert_eq!(committed.tabs[0].saved_page_id, Some(saved));
        assert_eq!(removed(&committed, &loaded, &pending), vec![original]);

        // Mirror successful Close and Create enqueues while the original callback is delayed.
        assert!(pending.remove(&original));
        assert!(pending.insert(replacement));
        assert!(!created(original, &mut pending, &mut loaded));
        assert_eq!(pending, HashSet::from([replacement]));
        assert!(loaded.is_empty());
        assert!(removed(&committed, &loaded, &pending).is_empty());

        // Even after replacement creation, repeated original callbacks remain inert.
        assert!(created(replacement, &mut pending, &mut loaded));
        assert!(!created(original, &mut pending, &mut loaded));
        assert!(pending.is_empty());
        assert_eq!(loaded, HashSet::from([replacement]));
        assert!(removed(&committed, &loaded, &pending).is_empty());
    }
}

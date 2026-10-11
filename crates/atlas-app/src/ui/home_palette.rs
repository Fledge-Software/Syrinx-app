//! Keeps delayed Home acknowledgments from reopening an entry the user already finished.

use std::sync::mpsc::{SendError, Sender};

use atlas_core::{TabId, TabModelSnapshot};

use crate::session::{HomeEntryAction, SessionCommand};

#[derive(Default)]
pub(super) struct HomePaletteIntent {
    sequence: u64,
    requested: Option<u64>,
}

impl HomePaletteIntent {
    pub(super) fn send(
        &mut self,
        sender: &Sender<SessionCommand>,
        command: SessionCommand,
    ) -> Result<(), SendError<SessionCommand>> {
        let action = match &command {
            SessionCommand::NewTab { address } if address == "about:blank" => {
                Some(HomeEntryAction::NewTab)
            }
            SessionCommand::ReopenClosed => Some(HomeEntryAction::ReopenClosed),
            SessionCommand::OpenSaved(id) => Some(HomeEntryAction::OpenSaved(*id)),
            _ => None,
        };
        if let Some(action) = action {
            self.sequence = self
                .sequence
                .checked_add(1)
                .expect("Home request IDs exhausted");
            sender.send(SessionCommand::HomeEntry {
                request_id: self.sequence,
                action,
            })?;
            self.requested = Some(self.sequence);
            return Ok(());
        }

        let keeps_entry = matches!(
            command,
            SessionCommand::Query { .. }
                | SessionCommand::ObserveMetadata { .. }
                | SessionCommand::SaveActive
                | SessionCommand::Unsave(_)
        );
        sender.send(command)?;
        // Only accepted actions supersede entry; metadata and search traffic do not.
        if !keeps_entry {
            self.finish_entry();
        }
        Ok(())
    }

    pub(super) fn finish_entry(&mut self) {
        self.requested = None;
    }

    pub(super) fn acknowledge(
        &mut self,
        model: &TabModelSnapshot,
        created: &[TabId],
        request_id: Option<u64>,
    ) -> bool {
        if self.requested.is_none() || self.requested != request_id {
            return false;
        }
        // Consume matching no-op and non-Home results too; this intent has finished.
        self.finish_entry();
        model.tabs.iter().any(|tab| {
            model.active_id == Some(tab.id)
                && created.contains(&tab.id)
                && tab.metadata.address == "about:blank"
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use atlas_core::{Activation, TabKind, TabMetadata, TabModel};

    use super::*;

    fn home() -> (TabModelSnapshot, TabId) {
        let mut model = TabModel::new();
        let id = model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: "about:blank".into(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .expect("open synthetic Home");
        (model.snapshot(), id)
    }

    fn new_home() -> SessionCommand {
        SessionCommand::NewTab {
            address: "about:blank".into(),
        }
    }

    fn queue(intent: &mut HomePaletteIntent, command: SessionCommand) -> u64 {
        let (sender, receiver) = mpsc::channel();
        intent
            .send(&sender, command)
            .expect("queue synthetic Home action");
        match receiver.recv().expect("receive synthetic action") {
            SessionCommand::HomeEntry { request_id, .. } => request_id,
            _ => panic!("Home action must carry its request identity"),
        }
    }

    fn send(intent: &mut HomePaletteIntent, command: SessionCommand) {
        let (sender, _receiver) = mpsc::channel();
        intent
            .send(&sender, command)
            .expect("queue synthetic command");
    }

    #[test]
    fn only_latest_home_request_can_open_and_completion_cancels_delayed_acknowledgments() {
        let (home, id) = home();
        let mut intent = HomePaletteIntent::default();
        let first = queue(&mut intent, new_home());
        let second = queue(&mut intent, new_home());
        assert!(second > first);
        assert!(!intent.acknowledge(&home, &[id], Some(first)));
        intent.finish_entry();
        assert!(!intent.acknowledge(&home, &[id], Some(second)));
        let next = queue(&mut intent, new_home());
        assert!(next > second);
        assert!(intent.acknowledge(&home, &[id], Some(next)));
        assert!(!intent.acknowledge(&home, &[id], Some(next)));
    }

    #[test]
    fn saved_or_reopened_real_page_cannot_rearm_an_earlier_home_acknowledgment() {
        let (home, id) = home();
        let saved = atlas_core::SavedPageId::new(1).expect("synthetic saved ID");
        for action in [
            SessionCommand::OpenSaved(saved),
            SessionCommand::ReopenClosed,
        ] {
            let mut intent = HomePaletteIntent::default();
            let first = queue(&mut intent, new_home());
            intent.finish_entry();
            let later = queue(&mut intent, action);
            assert!(!intent.acknowledge(&home, &[id], Some(first)));
            let mut web = home.clone();
            web.tabs[0].metadata.address = "https://reopened.example.test/".into();
            assert!(!intent.acknowledge(&web, &[id], Some(later)));
            assert!(intent.requested.is_none());
            assert!(!intent.acknowledge(&home, &[id], Some(first)));
        }
    }

    #[test]
    fn accepted_focus_cycle_close_and_navigation_supersede_pending_home_entry() {
        let (home, id) = home();
        for command in [
            SessionCommand::FocusTab(id),
            SessionCommand::CycleActive { backwards: false },
            SessionCommand::CloseTab(id),
            SessionCommand::CloseActive,
            SessionCommand::ObserveClosed(id),
            SessionCommand::NavigateActive {
                address: "https://submitted.example.test/".into(),
            },
            SessionCommand::NewTab {
                address: "https://new.example.test/".into(),
            },
            SessionCommand::Shutdown,
        ] {
            let mut intent = HomePaletteIntent::default();
            let request = queue(&mut intent, new_home());
            send(&mut intent, command);
            assert!(!intent.acknowledge(&home, &[id], Some(request)));
        }
    }

    #[test]
    fn rejected_enqueue_preserves_only_the_previously_accepted_intent() {
        let (sender, receiver) = mpsc::channel();
        let (home, id) = home();
        let mut intent = HomePaletteIntent::default();
        let request = queue(&mut intent, new_home());
        drop(receiver);
        assert!(intent.send(&sender, SessionCommand::ReopenClosed).is_err());
        assert!(intent.send(&sender, SessionCommand::FocusTab(id)).is_err());
        assert_eq!(intent.requested, Some(request));
        assert!(intent.acknowledge(&home, &[id], Some(request)));
        assert!(intent.send(&sender, new_home()).is_err());
        assert!(intent.requested.is_none());
    }

    #[test]
    fn reopened_and_saved_home_require_their_own_acknowledged_creation() {
        let (home, id) = home();
        let saved = atlas_core::SavedPageId::new(1).expect("synthetic saved ID");
        for action in [
            SessionCommand::ReopenClosed,
            SessionCommand::OpenSaved(saved),
        ] {
            let mut intent = HomePaletteIntent::default();
            let request = queue(&mut intent, action);
            assert!(!intent.acknowledge(&home, &[id], None));
            assert!(intent.acknowledge(&home, &[id], Some(request)));
        }
        let mut intent = HomePaletteIntent::default();
        let request = queue(&mut intent, SessionCommand::ReopenClosed);
        assert!(!intent.acknowledge(&home, &[], Some(request)));
        assert!(intent.requested.is_none());
        let request = queue(&mut intent, new_home());
        let mut inactive = home.clone();
        inactive.active_id = None;
        assert!(!intent.acknowledge(&inactive, &[id], Some(request)));
        assert!(intent.requested.is_none());
    }

    #[test]
    fn metadata_and_search_traffic_do_not_cancel_or_acknowledge_pending_entry() {
        let (home, id) = home();
        let mut intent = HomePaletteIntent::default();
        let request = queue(&mut intent, new_home());
        send(
            &mut intent,
            SessionCommand::ObserveMetadata {
                id,
                title: Some("Synthetic title".into()),
                address: None,
            },
        );
        send(
            &mut intent,
            SessionCommand::Query {
                request_id: 1,
                text: "synthetic".into(),
                scope: atlas_search::Scope::Everything,
                policy: super::super::search_policy("synthetic"),
            },
        );
        assert!(!intent.acknowledge(&home, &[], None));
        assert!(intent.acknowledge(&home, &[id], Some(request)));
    }
}

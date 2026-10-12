//! Routes native popup requests through durable tab allocation before native adoption.

use super::{BrowserCommand, BrowserView, SessionCommand, TabId};

impl BrowserView {
    pub(super) fn blank_popup(&self) -> Option<(TabId, String)> {
        desired_popup(&self.model, &self.popup_pages, self.browser_ready)
    }

    pub(super) fn request_popup(&mut self, opener: u64, request: u64, address: String) {
        let opener = self
            .model
            .tabs
            .iter()
            .find(|tab| tab.id.get() == opener)
            .map(|tab| tab.id);
        if self.closing || self.session_stopped || opener.is_none() {
            self.page_command(BrowserCommand::CancelPopup { request });
            return;
        }
        if let Some(opener) = opener {
            self.command(SessionCommand::AdoptPopup {
                request,
                opener,
                address,
            });
            if self.failure.is_some() {
                self.page_command(BrowserCommand::CancelPopup { request });
            }
        }
    }

    pub(super) fn commit_popup(&mut self, request: u64, target: Option<TabId>) {
        let Some(id) = target.filter(|_| !self.closing) else {
            self.page_command(BrowserCommand::CancelPopup { request });
            return;
        };
        if self.page_command(BrowserCommand::AdoptPopup {
            request,
            id: id.get(),
        }) {
            // Keep synchronization from creating a second native view.
            self.popup_pages.insert(id);
            self.pending_pages.insert(id);
        }
    }
}

fn desired_popup(
    model: &atlas_core::TabModelSnapshot,
    popups: &std::collections::HashSet<TabId>,
    ready: bool,
) -> Option<(TabId, String)> {
    if !ready {
        return None;
    }
    let id = model.active_id?;
    if !popups.contains(&id) {
        return None;
    }
    let tab = model.tabs.iter().find(|tab| tab.id == id)?;
    Some((id, tab.metadata.address.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::{Activation, TabKind, TabMetadata, TabModel};

    #[test]
    fn only_a_live_adopted_blank_tab_overrides_home() {
        let mut model = TabModel::new();
        let popup = model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: "about:blank".into(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .expect("synthetic popup");
        let popups = std::collections::HashSet::from([popup]);
        assert_eq!(
            desired_popup(&model.snapshot(), &popups, true),
            Some((popup, "about:blank".into()))
        );
        assert_eq!(desired_popup(&model.snapshot(), &popups, false), None);
        let home = model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: "about:blank".into(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .expect("synthetic Home");
        assert_eq!(desired_popup(&model.snapshot(), &popups, true), None);
        model.close(home).expect("close synthetic Home");
        model.close(popup).expect("close synthetic popup");
        assert_eq!(desired_popup(&model.snapshot(), &popups, true), None);
    }
}

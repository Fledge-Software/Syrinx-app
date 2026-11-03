//! Retains post-commit URI changes until a loading document can publish its final history address.

#[derive(Default)]
pub(super) struct HistoryDelivery {
    deferred: bool,
}

impl HistoryDelivery {
    /// A provisional change is covered by the next commit, not a same-page report.
    pub(super) fn boundary(&mut self) {
        self.deferred = false;
    }

    /// Loading documents may run replaceState/pushState before load completion.
    pub(super) fn uri_changed(&mut self, loading: bool) -> bool {
        self.deferred = loading;
        !loading
    }

    /// Publish once after success; a failed load must not publish its provisional address.
    pub(super) fn finished(&mut self, success: bool) -> bool {
        let deferred = std::mem::take(&mut self.deferred);
        success && deferred
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syrinx_core::{HistoryChange, HistoryDirection, HistoryReport, NavigationList};

    #[test]
    fn replace_state_while_loading_is_reported_at_finish_without_adding_a_row() {
        let mut delivery = HistoryDelivery::default();
        let mut list = NavigationList::default();
        for (address, back) in [
            ("http://example.test/43", false),
            ("http://example.test/45?replace", true),
        ] {
            delivery.boundary();
            list.observe(&HistoryReport {
                change: HistoryChange::NewDocument {
                    user_initiated: true,
                },
                address: address.into(),
                can_go_back: back,
                can_go_forward: false,
            });
        }
        if delivery.uri_changed(true) || delivery.finished(true) {
            list.observe(&HistoryReport {
                change: HistoryChange::SamePage { navigating: false },
                address: "http://example.test/45r".into(),
                can_go_back: true,
                can_go_forward: false,
            });
        }
        assert_eq!(list.current_address(), Some("http://example.test/45r"));
        assert_eq!(
            list.rows(HistoryDirection::Back, 15)
                .iter()
                .map(|row| row.address)
                .collect::<Vec<_>>(),
            ["http://example.test/43"]
        );
    }
    #[test]
    fn a_deferred_address_is_delivered_once_and_immediate_changes_supersede_it() {
        let mut delivery = HistoryDelivery::default();
        assert!(!delivery.uri_changed(true));
        assert!(delivery.finished(true));
        assert!(!delivery.finished(true));
        assert!(!delivery.uri_changed(true));
        assert!(delivery.uri_changed(false));
        assert!(!delivery.finished(true));
    }

    #[test]
    fn failures_and_navigation_boundaries_discard_deferred_addresses() {
        let mut delivery = HistoryDelivery::default();
        assert!(!delivery.uri_changed(true));
        assert!(!delivery.finished(false));
        assert!(!delivery.finished(true));
        assert!(!delivery.uri_changed(true));
        delivery.boundary();
        assert!(!delivery.finished(true));
    }
}

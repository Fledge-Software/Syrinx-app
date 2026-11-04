//! Retains URI changes until the document load or same-document API request settles.

#[derive(Default)]
pub(super) struct HistoryDelivery {
    deferred: bool,
    document_loading: bool,
}

impl HistoryDelivery {
    /// A document start or commit supersedes earlier changes and waits for its own finish.
    pub(super) fn boundary(&mut self) {
        self.deferred = false;
        self.document_loading = true;
    }

    /// Loading documents may run replaceState/pushState before load completion.
    pub(super) fn uri_changed(&mut self, loading: bool) -> bool {
        self.deferred = loading || self.document_loading;
        !self.deferred
    }

    /// Same-document API loads toggle loading without a document Finished signal.
    pub(super) fn loading_changed(&mut self, loading: bool) -> bool {
        if loading || self.document_loading {
            return false;
        }
        std::mem::take(&mut self.deferred)
    }

    /// Publish once after success; a failed load must not publish its provisional address.
    pub(super) fn finished(&mut self, success: bool) -> bool {
        self.document_loading = false;
        let deferred = std::mem::take(&mut self.deferred);
        success && deferred
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syrinx_core::{HistoryChange, HistoryDirection, HistoryReport, NavigationList};

    #[test]
    fn a_fragment_load_without_document_events_updates_the_shared_list() {
        let mut delivery = HistoryDelivery::default();
        let mut list = NavigationList::default();
        // Finish the documents before replaying the fragment's URI and loading notifications.
        for (address, can_go_back) in [
            ("http://example.test/p/45r", false),
            ("http://example.test/p/pushed", true),
        ] {
            delivery.boundary();
            list.observe(&HistoryReport {
                change: HistoryChange::NewDocument {
                    user_initiated: true,
                },
                address: address.into(),
                can_go_back,
                can_go_forward: false,
            });
            assert!(!delivery.finished(true));
        }

        // No document Started, Committed or Finished follows this API fragment request.
        assert!(!delivery.loading_changed(true));
        assert!(!delivery.uri_changed(true));
        if delivery.loading_changed(false) {
            list.observe(&HistoryReport {
                change: HistoryChange::SamePage { navigating: false },
                address: "http://example.test/p/pushed#fragment".into(),
                can_go_back: true,
                can_go_forward: false,
            });
        }
        assert_eq!(
            list.current_address(),
            Some("http://example.test/p/pushed#fragment")
        );
        assert_eq!(
            list.rows(HistoryDirection::Back, 15)
                .iter()
                .map(|row| row.address)
                .collect::<Vec<_>>(),
            ["http://example.test/p/45r"]
        );
        assert!(list.rows(HistoryDirection::Forward, 15).is_empty());
    }

    #[test]
    fn a_document_waits_for_its_own_success_or_failure_after_loading_stops() {
        for (loading, success) in [(true, true), (true, false), (false, true), (false, false)] {
            let mut delivery = HistoryDelivery::default();
            delivery.boundary();
            assert!(!delivery.uri_changed(loading));
            // The loading property may settle before the document's failure is reported.
            assert!(!delivery.loading_changed(false));
            assert_eq!(delivery.finished(success), success);
            assert!(!delivery.loading_changed(false));
        }
    }

    #[test]
    fn a_fragment_is_delivered_once_and_only_after_loading_stops() {
        let mut delivery = HistoryDelivery::default();
        assert!(!delivery.uri_changed(true));
        assert!(!delivery.loading_changed(true));
        assert!(delivery.loading_changed(false));
        assert!(!delivery.loading_changed(false));
        assert!(!delivery.finished(true));
        // An already-settled URI notification needs no second report at loading=false.
        assert!(delivery.uri_changed(false));
        assert!(!delivery.loading_changed(false));
    }

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

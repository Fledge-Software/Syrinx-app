//! Retains URI changes until the document load or same-document API request settles.

#[derive(Default)]
pub(super) struct HistoryDelivery {
    deferred: bool,
    document_loading: bool,
    request_pending: bool,
}

impl HistoryDelivery {
    /// Marks the host's request before WebKit can synchronously expose its pending URI.
    pub(super) fn request(&mut self) {
        self.deferred = false;
        self.request_pending = true;
    }

    /// A document start or commit supersedes earlier changes and waits for its own finish.
    pub(super) fn boundary(&mut self) {
        self.deferred = false;
        self.document_loading = true;
        self.request_pending = false;
    }

    /// Loading documents may run replaceState/pushState before load completion.
    pub(super) fn uri_changed(&mut self, loading: bool) -> bool {
        self.deferred = loading || self.document_loading || self.request_pending;
        !self.deferred
    }

    /// Same-document API loads toggle loading without a document Finished signal.
    pub(super) fn loading_changed(&mut self, loading: bool) -> bool {
        if loading || self.document_loading {
            return false;
        }
        self.request_pending = false;
        std::mem::take(&mut self.deferred)
    }

    /// Publish once after success; a failed load must not publish its provisional address.
    pub(super) fn finished(&mut self, success: bool) -> bool {
        self.document_loading = false;
        // An older document's finish cannot publish the next request's uncommitted URI.
        if self.request_pending {
            return false;
        }
        let deferred = std::mem::take(&mut self.deferred);
        success && deferred
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syrinx_core::{HistoryChange, HistoryDirection, HistoryReport, NavigationList};

    fn addresses(list: &NavigationList, direction: HistoryDirection) -> Vec<String> {
        list.rows(direction, 15)
            .iter()
            .map(|row| row.address.to_owned())
            .collect()
    }

    #[test]
    fn an_older_finish_cannot_rename_a_forward_row_to_the_next_requested_uri() {
        let mut delivery = HistoryDelivery::default();
        let mut list = NavigationList::default();
        // Commit 40 through 43, leaving 43's document finish in flight.
        for (address, can_go_back, finished) in [
            ("http://example.test/p/40", false, true),
            ("http://example.test/p/41", true, true),
            ("http://example.test/p/42", true, true),
            ("http://example.test/p/43", true, false),
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
            if finished {
                assert!(!delivery.finished(true));
            }
        }
        // load_uri exposes 44 while 43 is still finishing; 44 has not started or committed.
        delivery.request();
        assert!(!delivery.uri_changed(true));
        if delivery.finished(true) {
            list.observe(&HistoryReport {
                change: HistoryChange::SamePage { navigating: false },
                address: "http://example.test/p/44".into(),
                can_go_back: true,
                can_go_forward: false,
            });
        }
        delivery.boundary();
        list.observe(&HistoryReport {
            change: HistoryChange::NewDocument {
                user_initiated: true,
            },
            address: "http://example.test/p/44".into(),
            can_go_back: true,
            can_go_forward: false,
        });
        assert!(!delivery.finished(true));
        let back_before_jump = addresses(&list, HistoryDirection::Back);

        // Replay the native three-row Back jump, then check both directions literally.
        assert!(list.start_jump(HistoryDirection::Back, 3));
        list.observe(&HistoryReport {
            change: HistoryChange::BackOrForward,
            address: "http://example.test/p/41".into(),
            can_go_back: true,
            can_go_forward: true,
        });
        assert_eq!(
            addresses(&list, HistoryDirection::Forward),
            [
                "http://example.test/p/42",
                "http://example.test/p/43",
                "http://example.test/p/44",
            ]
        );
        assert_eq!(
            back_before_jump,
            [
                "http://example.test/p/43",
                "http://example.test/p/42",
                "http://example.test/p/41",
                "http://example.test/p/40",
            ]
        );
        assert_eq!(
            addresses(&list, HistoryDirection::Back),
            ["http://example.test/p/40"]
        );
        assert!(list.start_jump(HistoryDirection::Forward, 2));
        list.observe(&HistoryReport {
            change: HistoryChange::BackOrForward,
            address: "http://example.test/p/43".into(),
            can_go_back: true,
            can_go_forward: true,
        });
        assert_eq!(list.current_address(), Some("http://example.test/p/43"));
        assert_eq!(
            addresses(&list, HistoryDirection::Back),
            [
                "http://example.test/p/42",
                "http://example.test/p/41",
                "http://example.test/p/40",
            ]
        );
    }

    #[test]
    fn a_requested_uri_is_not_published_before_its_document_starts() {
        let mut delivery = HistoryDelivery::default();
        delivery.request();
        assert!(!delivery.uri_changed(false));
        delivery.boundary();
        assert!(!delivery.finished(true));
        assert!(delivery.uri_changed(false));
    }

    #[test]
    fn an_older_finish_preserves_a_new_fragment_until_its_api_request_settles() {
        for success in [true, false] {
            let mut delivery = HistoryDelivery::default();
            delivery.boundary();
            delivery.request();
            assert!(!delivery.uri_changed(true));
            assert!(!delivery.loading_changed(false));
            assert!(!delivery.finished(success));
            assert!(delivery.loading_changed(false));
            assert!(!delivery.loading_changed(false));
        }
    }

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
        delivery.request();
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

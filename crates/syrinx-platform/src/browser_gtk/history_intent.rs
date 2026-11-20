//! Separates Syrinx-requested loads from page-script moves when building history rows.

use super::history_state::Started;
use webkit2gtk::NavigationType;

#[derive(Default)]
pub(super) struct HistoryIntent {
    requested: Option<String>,
}

impl HistoryIntent {
    pub(super) fn request(&mut self, address: &str) {
        self.requested = Some(address.to_owned());
    }

    /// Drop an unconsumed host request before Back, Forward or Reload.
    pub(super) fn clear(&mut self) {
        self.requested = None;
    }

    /// Consume only the matching request; an older callback must not withdraw a newer request.
    pub(super) fn consume_request(&mut self, address: &str) -> bool {
        let requested = self
            .requested
            .as_deref()
            .is_some_and(|expected| same_address(expected, address));
        if requested {
            self.clear();
        }
        requested
    }

    /// Classify a navigation; unrelated page/frame requests stay page-made.
    pub(super) fn started(
        &mut self,
        kind: NavigationType,
        gesture: bool,
        address: &str,
    ) -> Started {
        let requested = self.consume_request(address);
        match kind {
            NavigationType::BackForward => Started::BackOrForward,
            NavigationType::Reload => Started::Reload,
            _ => Started::NewDocument {
                user_initiated: gesture || requested,
            },
        }
    }
}

/// Match the same spelling WebKit uses for a host-only URL, without another URL dependency.
fn same_address(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }
    match (
        syrinx_core::web_address(expected),
        syrinx_core::web_address(actual),
    ) {
        (Ok(expected), Ok(actual)) => expected == actual,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syrinx_core::{HistoryChange, HistoryDirection, HistoryReport, NavigationList};

    #[test]
    fn a_fragment_completion_retires_only_its_matching_host_request() {
        let mut intent = HistoryIntent::default();
        let fragment = "http://example.test/p/pushed#fragment";
        let next = "http://example.test/p/next";
        // An older fragment callback must leave a newer host navigation selectable.
        intent.request(next);
        assert!(!intent.consume_request(fragment));
        assert!(matches!(
            intent.started(NavigationType::Other, false, next),
            Started::NewDocument {
                user_initiated: true
            }
        ));
        // Once the matching fragment settles, a later script cannot reuse its host intent.
        intent.request(fragment);
        assert!(intent.consume_request(fragment));
        assert!(matches!(
            intent.started(NavigationType::Other, false, fragment),
            Started::NewDocument {
                user_initiated: false
            }
        ));
    }

    #[test]
    fn five_syrinx_loads_without_webkit_gestures_leave_four_selectable_rows() {
        let mut intent = HistoryIntent::default();
        let mut list = NavigationList::default();
        for (index, address) in [
            "http://example.test/40",
            "http://example.test/41",
            "http://example.test/42",
            "http://example.test/43",
            "http://example.test/44",
        ]
        .iter()
        .enumerate()
        {
            intent.request(address);
            let Started::NewDocument { user_initiated } =
                intent.started(NavigationType::Other, false, address)
            else {
                panic!("Syrinx load must be a new document");
            };
            list.observe(&HistoryReport {
                change: HistoryChange::NewDocument { user_initiated },
                address: (*address).into(),
                can_go_back: index > 0,
                can_go_forward: false,
            });
        }
        let rows: Vec<_> = list
            .rows(HistoryDirection::Back, 15)
            .iter()
            .map(|row| row.address.to_owned())
            .collect();
        assert_eq!(
            rows,
            [
                "http://example.test/43",
                "http://example.test/42",
                "http://example.test/41",
                "http://example.test/40"
            ]
        );
        assert!(list.start_jump(HistoryDirection::Back, 3));
    }
    #[test]
    fn a_host_request_is_consumed_once_and_does_not_reclassify_a_later_script() {
        let mut intent = HistoryIntent::default();
        intent.request("https://example.test/");
        assert!(matches!(
            intent.started(NavigationType::Other, false, "https://example.test/"),
            Started::NewDocument {
                user_initiated: true
            }
        ));
        assert!(matches!(
            intent.started(NavigationType::Other, false, "https://example.test/"),
            Started::NewDocument {
                user_initiated: false
            }
        ));
    }

    #[test]
    fn an_unrelated_frame_request_does_not_consume_the_host_request() {
        let mut intent = HistoryIntent::default();
        intent.request("https://example.test/main");
        assert!(matches!(
            intent.started(NavigationType::Other, false, "https://example.test/frame"),
            Started::NewDocument {
                user_initiated: false
            }
        ));
        assert!(matches!(
            intent.started(NavigationType::Other, false, "https://example.test/main"),
            Started::NewDocument {
                user_initiated: true
            }
        ));
    }

    #[test]
    fn host_only_url_normalization_does_not_lose_intent() {
        let mut intent = HistoryIntent::default();
        intent.request("https://EXAMPLE.test");
        assert!(matches!(
            intent.started(NavigationType::Other, false, "https://example.test/"),
            Started::NewDocument {
                user_initiated: true
            }
        ));
    }

    #[test]
    fn withdrawn_requests_never_claim_a_later_script_load() {
        let mut intent = HistoryIntent::default();
        intent.request("https://example.test/main");
        intent.clear();
        assert!(matches!(
            intent.started(NavigationType::Other, false, "https://example.test/main"),
            Started::NewDocument {
                user_initiated: false
            }
        ));
        assert!(matches!(
            intent.started(
                NavigationType::LinkClicked,
                true,
                "https://example.test/linked"
            ),
            Started::NewDocument {
                user_initiated: true
            }
        ));
        assert!(matches!(
            intent.started(NavigationType::Reload, false, "https://example.test/main"),
            Started::Reload
        ));
        assert!(matches!(
            intent.started(
                NavigationType::BackForward,
                false,
                "https://example.test/main"
            ),
            Started::BackOrForward
        ));
    }
}

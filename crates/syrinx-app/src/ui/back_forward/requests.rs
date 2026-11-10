//! Keeps track of Back and Forward's menu while the worker reads its rows, and says when the
//! menu has gone stale, apart from GPUI so it can be tested (#126).

use syrinx_core::{HistoryChange, HistoryDirection, TabId};

/// A Back or Forward menu waiting for its rows from the worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RowsRequest {
    pub(super) id: u64,
    pub(super) tab: TabId,
    pub(super) direction: HistoryDirection,
    /// Opened by Alt Down, so it starts with its first row selected.
    pub(super) keyed: bool,
    /// How many of the shown page's reports had gone to the worker when this was asked.
    reports_before: u64,
}

/// What a press on Back or Forward does about the menu's rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Press {
    /// Ask the worker for the rows under this request number.
    Ask(u64),
    /// A second press before the rows arrived: the menu about to open does not.
    Cancelled,
}

/// What a page report or a change in the listed reach means for the history menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Stale {
    Keep,
    /// Close an open history menu as Escape does, giving the keys back to the page.
    Close,
}

/// The one menu being read, the last request number used, and how many of the shown
/// page's reports have gone to the worker.
#[derive(Debug, Default)]
pub(super) struct RowsRequests {
    pending: Option<RowsRequest>,
    issued: u64,
    reports: u64,
}

impl RowsRequests {
    /// A press asks for `direction`'s rows on `tab`, or cancels the same menu still being read.
    pub(super) fn press(&mut self, tab: TabId, direction: HistoryDirection, keyed: bool) -> Press {
        if self
            .pending
            .take_if(|pending| pending.direction == direction)
            .is_some()
        {
            return Press::Cancelled;
        }
        self.issued += 1;
        self.pending = Some(RowsRequest {
            id: self.issued,
            tab,
            direction,
            keyed,
            reports_before: self.reports,
        });
        Press::Ask(self.issued)
    }

    /// The request could not be sent, so no reply will come; the next press asks afresh.
    pub(super) fn unsent(&mut self, id: u64) {
        self.pending.take_if(|pending| pending.id == id);
    }

    /// The reply to request `id`: the request it answers, unless it was replaced or cancelled.
    pub(super) fn arrived(&mut self, id: u64) -> Option<RowsRequest> {
        self.pending.take_if(|pending| pending.id == id)
    }

    /// Another menu, the palette or a move of the page makes the pending menu unwanted.
    pub(super) fn cancel(&mut self) {
        self.pending = None;
    }

    /// The shown page reported `change` to the worker. A move shifts every row's step count,
    /// so a menu open or being read would jump to the wrong page.
    pub(super) fn page_reported(&mut self, change: HistoryChange) -> Stale {
        self.reports += 1;
        if !shifts_rows(change) {
            return Stale::Keep;
        }
        self.pending = None;
        Stale::Close
    }

    /// The shown tab's listed reach changed. An open menu's rows are gone. A request is stale
    /// only if a report went to the worker after it: one sent before it was already read into
    /// the reply, so its reach arriving late changes nothing (review of #213).
    pub(super) fn relisted(&mut self) -> Stale {
        let reports = self.reports;
        self.pending
            .take_if(|pending| pending.reports_before < reports);
        Stale::Close
    }
}

/// Whether a change moves the shown page within its list, which shifts every row's step count.
/// A same-page change or a reload only renames the current row, which no menu lists.
fn shifts_rows(change: HistoryChange) -> bool {
    matches!(
        change,
        HistoryChange::NewDocument { .. } | HistoryChange::BackOrForward
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACK: HistoryDirection = HistoryDirection::Back;
    const FORWARD: HistoryDirection = HistoryDirection::Forward;

    fn tab() -> TabId {
        TabId::new(3).expect("synthetic tab id")
    }

    #[test]
    fn a_reply_opens_only_the_menu_still_waiting_for_it() {
        let mut requests = RowsRequests::default();
        let Press::Ask(first) = requests.press(tab(), BACK, false) else {
            panic!("the first press asks");
        };
        let Press::Ask(second) = requests.press(tab(), FORWARD, true) else {
            panic!("a press the other way replaces it");
        };
        assert_eq!(requests.arrived(first), None, "a replaced request's reply");
        let request = requests.arrived(second).expect("the waiting request");
        assert_eq!((request.direction, request.keyed), (FORWARD, true));
        assert_eq!(
            requests.arrived(second),
            None,
            "a reply opens one menu once"
        );
    }

    #[test]
    fn a_second_press_before_the_reply_cancels_the_menu() {
        let mut requests = RowsRequests::default();
        let Press::Ask(id) = requests.press(tab(), BACK, false) else {
            panic!("the first press asks");
        };
        assert_eq!(requests.press(tab(), BACK, false), Press::Cancelled);
        assert_eq!(requests.arrived(id), None);
        assert!(matches!(requests.press(tab(), BACK, false), Press::Ask(_)));
    }

    #[test]
    fn a_request_that_could_not_be_sent_does_not_swallow_the_next_press() {
        // Review of #213: with no reply coming, the next press would only cancel.
        let mut requests = RowsRequests::default();
        let Press::Ask(id) = requests.press(tab(), BACK, false) else {
            panic!("the first press asks");
        };
        requests.unsent(id);
        assert!(matches!(requests.press(tab(), BACK, false), Press::Ask(_)));
    }

    #[test]
    fn another_menu_or_the_palette_cancels_the_waiting_menu() {
        let mut requests = RowsRequests::default();
        let Press::Ask(id) = requests.press(tab(), BACK, false) else {
            panic!("the first press asks");
        };
        requests.cancel();
        assert_eq!(requests.arrived(id), None);
    }

    #[test]
    fn a_move_of_the_shown_page_closes_the_menu_and_drops_its_rows() {
        // Review of #213, blocking: the close must give the keys back to the page.
        let mut requests = RowsRequests::default();
        let Press::Ask(id) = requests.press(tab(), BACK, false) else {
            panic!("the first press asks");
        };
        let moved = requests.page_reported(HistoryChange::NewDocument {
            user_initiated: false,
        });
        assert_eq!(moved, Stale::Close);
        assert_eq!(requests.arrived(id), None);
        assert_eq!(
            requests.page_reported(HistoryChange::BackOrForward),
            Stale::Close
        );
    }

    #[test]
    fn a_single_page_sites_own_address_change_keeps_the_menu() {
        let mut requests = RowsRequests::default();
        let Press::Ask(id) = requests.press(tab(), BACK, false) else {
            panic!("the first press asks");
        };
        for change in [
            HistoryChange::SamePage { navigating: false },
            HistoryChange::Unchanged,
            HistoryChange::JumpAbandoned,
        ] {
            assert_eq!(requests.page_reported(change), Stale::Keep, "{change:?}");
        }
        assert!(requests.arrived(id).is_some());
    }

    #[test]
    fn a_late_reach_for_an_earlier_report_keeps_a_fresh_request() {
        // Review of #213: the reach of a commit the press came after is already in its rows.
        let mut requests = RowsRequests::default();
        requests.page_reported(HistoryChange::SamePage { navigating: false });
        let Press::Ask(id) = requests.press(tab(), BACK, false) else {
            panic!("the press asks");
        };
        assert_eq!(
            requests.relisted(),
            Stale::Close,
            "an open menu still closes"
        );
        assert!(requests.arrived(id).is_some());
    }

    #[test]
    fn a_relisting_after_a_later_report_drops_the_request() {
        let mut requests = RowsRequests::default();
        let Press::Ask(id) = requests.press(tab(), BACK, false) else {
            panic!("the press asks");
        };
        requests.page_reported(HistoryChange::SamePage { navigating: false });
        requests.relisted();
        assert_eq!(requests.arrived(id), None);
    }
}

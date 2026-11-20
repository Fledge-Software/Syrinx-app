//! Checks one burst of overlapping Find operations against each request's own count (#245).

use super::super::lifecycle::require;
use syrinx_platform::browser::BrowserEvent;

/// The overlap fixture's tab, kept apart from the armed observer on tab 3000.
pub(super) const OVERLAP: u64 = 3001;

/// What one burst must report: the final request's exact reports, the native selection
/// sequence the page reports, and the count each earlier request may still report.
pub(super) struct OverlapObservation<'a> {
    request: u64,
    expected: &'a [(i32, i32)],
    selections: Option<&'a [u32]>,
    earlier: &'a [(u64, i32)],
    seen: usize,
    selected: Vec<u32>,
}

impl<'a> OverlapObservation<'a> {
    pub(super) fn new(
        request: u64,
        expected: &'a [(i32, i32)],
        selections: Option<&'a [u32]>,
        earlier: &'a [(u64, i32)],
    ) -> Self {
        Self {
            request,
            expected,
            selections,
            earlier,
            seen: 0,
            selected: Vec::new(),
        }
    }

    pub(super) fn observe(&mut self, event: &BrowserEvent) -> Result<(), String> {
        match event {
            BrowserEvent::FindResult {
                id: OVERLAP,
                request,
                count,
                active,
            } => self.observe_report(*request, *count, *active),
            BrowserEvent::Title { id: OVERLAP, title } => self.observe_selection(title),
            _ => Ok(()),
        }
    }

    /// Done once every final report has arrived and the selection ended where expected.
    pub(super) fn complete(&self) -> bool {
        self.seen == self.expected.len() && self.selection_matches()
    }

    pub(super) fn describe(&self) -> String {
        format!(
            "{} of {} reports; selections {:?}, expected {:?}",
            self.seen,
            self.expected.len(),
            self.selected,
            self.selections
        )
    }

    /// An earlier request may report only with its own query's count, and only before the final
    /// one does: the host can apply a reply between two sends, while that request is current.
    fn observe_report(&mut self, request: u64, count: i32, active: i32) -> Result<(), String> {
        if request != self.request {
            let own = self.earlier.iter().find(|(earlier, _)| *earlier == request);
            require(
                self.seen == 0,
                &format!("Request {request} reported after request {}", self.request),
            )?;
            return require(
                own.is_some_and(|&(_, own)| own == count && valid_position(count, active)),
                &format!("Request {request} reported {active} of {count}, not its own query"),
            );
        }
        let expected = self.expected.get(self.seen).copied();
        require(
            expected == Some((count, active)),
            &format!(
                "Find report {} for request {request}: expected {expected:?}, received ({count}, {active})",
                self.seen + 1
            ),
        )?;
        self.seen += 1;
        Ok(())
    }

    fn observe_selection(&mut self, title: &str) -> Result<(), String> {
        let Some(occurrence) = title.strip_prefix("overlap-selected:") else {
            return Ok(());
        };
        let occurrence = occurrence
            .parse()
            .map_err(|_| "Malformed overlap selection report")?;
        require(
            (1..=3).contains(&occurrence),
            "Overlap selection is outside the synthetic document",
        )?;
        self.selected.push(occurrence);
        Ok(())
    }

    /// The page moved only through expected occurrences, in order, and ended on the last one.
    ///
    /// The page's `selectionchange` events coalesce, so a burst may skip expected occurrences.
    fn selection_matches(&self) -> bool {
        let Some(expected) = self.selections else {
            return true;
        };
        let mut remaining = expected.iter();
        let in_order = self
            .selected
            .iter()
            .all(|seen| remaining.any(|occurrence| occurrence == seen));
        in_order && self.selected.last() == expected.last()
    }
}

/// A one-based position within `count` matches, or -1 when there are none.
fn valid_position(count: i32, active: i32) -> bool {
    if count == 0 {
        active == -1
    } else {
        (1..=count).contains(&active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(request: u64, count: i32, active: i32) -> BrowserEvent {
        BrowserEvent::FindResult {
            id: OVERLAP,
            request,
            count,
            active,
        }
    }

    fn selected(occurrence: u32) -> BrowserEvent {
        BrowserEvent::Title {
            id: OVERLAP,
            title: format!("overlap-selected:{occurrence}"),
        }
    }

    #[test]
    fn the_final_reports_and_selection_sequence_are_both_required() {
        let mut overlap = OverlapObservation::new(56, &[(3, 1), (3, 2)], Some(&[1, 2]), &[]);
        overlap.observe(&report(56, 3, 1)).unwrap();
        overlap.observe(&selected(1)).unwrap();
        overlap.observe(&report(56, 3, 2)).unwrap();
        assert!(
            !overlap.complete(),
            "the page has not shown the second match"
        );
        overlap.observe(&selected(2)).unwrap();
        assert!(overlap.complete());
    }

    #[test]
    fn a_selection_out_of_the_expected_order_is_not_complete() {
        for observed in [&[2, 1, 2][..], &[3, 1, 2], &[1, 2, 3]] {
            let mut overlap = OverlapObservation::new(56, &[(3, 1)], Some(&[1, 2, 3, 2]), &[]);
            overlap.observe(&report(56, 3, 1)).unwrap();
            for &occurrence in observed {
                overlap.observe(&selected(occurrence)).unwrap();
            }
            assert!(!overlap.complete(), "{observed:?}");
        }
        let mut coalesced = OverlapObservation::new(56, &[(3, 1)], Some(&[1, 2, 3, 2]), &[]);
        coalesced.observe(&report(56, 3, 1)).unwrap();
        for occurrence in [1, 3, 2] {
            coalesced.observe(&selected(occurrence)).unwrap();
        }
        assert!(coalesced.complete(), "coalesced events skip the first 2");
    }

    #[test]
    fn a_wrong_count_or_position_for_the_final_request_fails() {
        for wrong in [report(51, 1, 1), report(51, 3, 1), report(51, 3, 3)] {
            let mut overlap = OverlapObservation::new(51, &[(3, 1), (3, 2)], None, &[]);
            let outcome = overlap
                .observe(&report(51, 3, 1))
                .and(overlap.observe(&wrong));
            assert!(outcome.is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn an_extra_final_report_fails() {
        let mut overlap = OverlapObservation::new(55, &[(3, 1)], None, &[]);
        overlap.observe(&report(55, 3, 1)).unwrap();
        assert!(overlap.observe(&report(55, 3, 1)).is_err());
    }

    #[test]
    fn an_earlier_request_may_report_only_its_own_search_and_only_first() {
        let earlier = [(52, 6), (53, 0)];
        let mut overlap = OverlapObservation::new(55, &[(3, 1)], None, &earlier);
        overlap.observe(&report(52, 6, 1)).unwrap();
        overlap.observe(&report(52, 6, 2)).unwrap();
        overlap.observe(&report(53, 0, -1)).unwrap();
        assert!(
            overlap.observe(&report(53, 0, 1)).is_err(),
            "a match with none found"
        );
        assert!(
            overlap.observe(&report(52, 6, 7)).is_err(),
            "outside its own matches"
        );
        assert!(
            overlap.observe(&report(52, 3, 1)).is_err(),
            "credited the final count"
        );
        assert!(
            overlap.observe(&report(54, 3, 1)).is_err(),
            "not part of this burst"
        );
        overlap.observe(&report(55, 3, 1)).unwrap();
        assert!(
            overlap.observe(&report(52, 6, 1)).is_err(),
            "late after the final"
        );
    }

    #[test]
    fn malformed_or_out_of_range_selections_fail() {
        for title in [
            "overlap-selected:",
            "overlap-selected:0",
            "overlap-selected:4",
        ] {
            let mut overlap = OverlapObservation::new(50, &[(1, 1)], None, &[]);
            let event = BrowserEvent::Title {
                id: OVERLAP,
                title: title.into(),
            };
            assert!(overlap.observe(&event).is_err(), "{title}");
        }
    }
}

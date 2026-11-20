//! Joins the host's Find report with the authored document's independent selected span.

use super::super::lifecycle::require;
use syrinx_platform::browser::BrowserEvent;

pub(super) struct FindObservation {
    request: u64,
    count: i32,
    active: i32,
    operation: Option<u64>,
    result: bool,
    selection: bool,
}

impl FindObservation {
    pub(super) fn new(request: u64, count: i32, active: i32, operation: Option<u64>) -> Self {
        Self {
            request,
            count,
            active,
            operation,
            result: false,
            selection: operation.is_none(),
        }
    }

    pub(super) fn observe(&mut self, event: &BrowserEvent) -> Result<bool, String> {
        match event {
            BrowserEvent::FindResult {
                id: 3000,
                request,
                count,
                active,
            } if *request == self.request => {
                require(
                    (*count, *active) == (self.count, self.active),
                    &format!(
                        "Find expected {} of {}, received {active} of {count}",
                        self.active, self.count
                    ),
                )?;
                self.result = true;
            }
            BrowserEvent::Title { id: 3000, title } => self.observe_selection(title)?,
            _ => {}
        }
        Ok(self.result && self.selection)
    }

    fn observe_selection(&mut self, title: &str) -> Result<(), String> {
        require(
            !title.starts_with("fixture-find-error:"),
            "Authored Find observation could not arm from its current selection",
        )?;
        let Some(expected_operation) = self.operation else {
            return Ok(());
        };
        let Some((operation, occurrence)) = selected_occurrence(title)? else {
            return Ok(());
        };
        if operation < expected_operation {
            return Ok(());
        }
        require(
            operation == expected_operation,
            "Authored Find observation belongs to a later operation",
        )?;
        require(
            occurrence == self.active,
            &format!(
                "Native Find selection for operation {operation}: expected occurrence {}, actual occurrence {occurrence}",
                self.active
            ),
        )?;
        self.selection = true;
        Ok(())
    }
}

fn selected_occurrence(title: &str) -> Result<Option<(u64, i32)>, String> {
    let Some(report) = title.strip_prefix("fixture-find-selected:") else {
        return Ok(None);
    };
    let (operation, occurrence) = report
        .split_once(':')
        .ok_or("Malformed authored Find selection report")?;
    let operation = operation
        .parse()
        .map_err(|_| "Invalid authored Find operation number")?;
    let occurrence = occurrence
        .parse()
        .map_err(|_| "Invalid authored Find occurrence number")?;
    require(
        (1..=3).contains(&occurrence),
        "Authored Find occurrence is outside the synthetic document",
    )?;
    Ok(Some((operation, occurrence)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(active: i32) -> BrowserEvent {
        BrowserEvent::FindResult {
            id: 3000,
            request: 41,
            count: 3,
            active,
        }
    }
    fn selected(operation: u64, occurrence: u32) -> BrowserEvent {
        BrowserEvent::Title {
            id: 3000,
            title: format!("fixture-find-selected:{operation}:{occurrence}"),
        }
    }

    #[test]
    fn both_engine_result_and_document_selection_are_required_in_either_order() {
        let mut observer = FindObservation::new(41, 3, 2, Some(7));
        assert!(
            !observer.observe(&result(2)).unwrap(),
            "Rust's calculated index cannot establish native selection"
        );
        assert!(observer.observe(&selected(7, 2)).unwrap());
        let mut observer = FindObservation::new(41, 3, 2, Some(7));
        assert!(!observer.observe(&selected(7, 2)).unwrap());
        assert!(observer.observe(&result(2)).unwrap());
    }

    #[test]
    fn reversed_native_direction_fails_even_when_the_host_index_is_correct() {
        let mut observer = FindObservation::new(41, 3, 2, Some(7));
        let outcome = observer.observe(&selected(7, 3));
        assert!(
            outcome.is_err(),
            "native Previous selected third while host claimed Next selected second"
        );
    }

    #[test]
    fn earlier_operation_titles_cannot_satisfy_a_new_native_step() {
        let mut observer = FindObservation::new(41, 3, 2, Some(7));
        assert!(!observer.observe(&selected(6, 2)).unwrap());
        assert!(!observer.observe(&result(2)).unwrap());
        assert!(observer.observe(&selected(7, 2)).unwrap());
    }
    #[test]
    fn native_wrap_occurrence_is_checked_independently_of_the_host_index() {
        for expected in [1, 3] {
            let mut observer = FindObservation::new(41, 3, expected, Some(9));
            assert!(!observer.observe(&result(expected)).unwrap());
            assert!(observer.observe(&selected(9, 2)).is_err());
        }
    }

    #[test]
    fn malformed_authored_reports_fail_and_unrelated_titles_are_ignored() {
        for title in [
            "fixture-find-selected:",
            "fixture-find-selected:7:0",
            "fixture-find-selected:7:4",
            "fixture-find-selected:7:2:extra",
            "fixture-find-selected:bad:2",
        ] {
            assert!(selected_occurrence(title).is_err(), "{title}");
        }
        assert_eq!(selected_occurrence("controls-ready").unwrap(), None);
    }

    #[test]
    fn no_match_needs_no_invented_document_selection() {
        let mut observer = FindObservation::new(42, 0, -1, None);
        assert!(
            observer
                .observe(&BrowserEvent::FindResult {
                    id: 3000,
                    request: 42,
                    count: 0,
                    active: -1
                })
                .unwrap()
        );
    }
}

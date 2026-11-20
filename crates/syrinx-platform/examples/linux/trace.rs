//! Keeps bounded, synthetic-only navigation evidence for a failed native fixture step.

use std::collections::VecDeque;
use syrinx_core::{HistoryDirection, NavigationList};
use syrinx_platform::browser::{BrowserCommand, BrowserEvent};

#[derive(Default)]
pub struct Trace {
    events: VecDeque<String>,
}

impl Trace {
    pub fn record(&mut self, event: &BrowserEvent) {
        let line = match event {
            BrowserEvent::History { id, report } => Some(format!(
                "History tab={id} change={:?} address={} back={} forward={}",
                report.change,
                synthetic_address(&report.address),
                report.can_go_back,
                report.can_go_forward
            )),
            BrowserEvent::Url { id, url } => {
                Some(format!("Url tab={id} address={}", synthetic_address(url)))
            }
            BrowserEvent::Loading { id, loading } => {
                Some(format!("Loading tab={id} loading={loading}"))
            }
            BrowserEvent::NavigationCompleted {
                id,
                success,
                cancelled,
            } => Some(format!(
                "NavigationCompleted tab={id} success={success} cancelled={cancelled}"
            )),
            _ => None,
        };
        if let Some(line) = line {
            self.push(line);
        }
    }

    /// Keep navigation commands beside callbacks to expose a late event from an earlier step.
    pub fn command(&mut self, command: &BrowserCommand) {
        let line = match command {
            BrowserCommand::Navigate { id, url } => Some(format!(
                "Command Navigate tab={id} address={}",
                synthetic_address(url)
            )),
            BrowserCommand::Create { id, url } => Some(format!(
                "Command Create tab={id} address={}",
                synthetic_address(url)
            )),
            BrowserCommand::GoToOffset {
                id,
                direction,
                steps,
            } => Some(format!(
                "Command GoToOffset tab={id} direction={direction:?} steps={steps}"
            )),
            BrowserCommand::Reload { id } => Some(format!("Command Reload tab={id}")),
            _ => None,
        };
        if let Some(line) = line {
            self.push(line);
        }
    }

    fn push(&mut self, line: String) {
        if self.events.len() == 64 {
            self.events.pop_front();
        }
        self.events.push_back(line);
    }

    /// Compare original values; redaction is only for printing a mismatch.
    pub fn expect_rows(
        &self,
        step: &str,
        direction: HistoryDirection,
        expected: &[String],
        list: &NavigationList,
    ) -> Result<(), String> {
        let actual: Vec<_> = list
            .rows(direction, 20)
            .iter()
            .map(|row| row.address.to_owned())
            .collect();
        let actual_safe: Vec<_> = actual
            .iter()
            .map(|address| synthetic_address(address))
            .collect();
        let expected_safe: Vec<_> = expected
            .iter()
            .map(|address| synthetic_address(address))
            .collect();
        let detail = format!(
            "History rows assertion step={step}; direction={direction:?}; actual={actual_safe:?}; expected={expected_safe:?}"
        );
        println!("{detail}");
        if actual == expected {
            return Ok(());
        }
        Err(format!("{detail}\n{}", self.state(list)))
    }

    pub fn expect_jump(
        &self,
        step: &str,
        direction: HistoryDirection,
        steps: usize,
        actual: bool,
        list: &NavigationList,
    ) -> Result<(), String> {
        let detail = format!(
            "History jump assertion step={step}; direction={direction:?}; steps={steps}; actual={actual}; expected=true"
        );
        println!("{detail}");
        if actual {
            return Ok(());
        }
        Err(format!("{detail}\n{}", self.state(list)))
    }

    pub fn reached(&self, step: &str, expected: &str, list: &NavigationList) {
        println!(
            "History address assertion step={step}; actual={}; expected={}",
            list.current_address()
                .map(synthetic_address)
                .unwrap_or("<none>"),
            synthetic_address(expected)
        );
    }

    pub fn timeout(&self, step: &str, expected: Option<&str>, list: &NavigationList) -> String {
        format!(
            "Timed out at step={step}; expected={}; {}",
            expected.map(synthetic_address).unwrap_or("<none>"),
            self.state(list)
        )
    }

    fn state(&self, list: &NavigationList) -> String {
        format!(
            "current={}; back={:?}; forward={:?}\nlast navigation events:\n{}",
            list.current_address()
                .map(synthetic_address)
                .unwrap_or("<none>"),
            rows(list, HistoryDirection::Back),
            rows(list, HistoryDirection::Forward),
            self.events.iter().cloned().collect::<Vec<_>>().join("\n")
        )
    }
}

/// Refuse to echo an unexpected external address even from this synthetic-profile harness.
fn synthetic_address(address: &str) -> &str {
    if address == "about:blank" {
        return address;
    }
    for prefix in ["http://127.0.0.1:", "http://localhost:"] {
        if let Some(rest) = address.strip_prefix(prefix)
            && let Some((port, _)) = rest.split_once('/')
            && port.parse::<u16>().is_ok()
        {
            return address;
        }
    }
    "<non-synthetic address>"
}

fn rows(list: &NavigationList, direction: HistoryDirection) -> Vec<String> {
    list.rows(direction, syrinx_core::HISTORY_ENTRY_LIMIT)
        .iter()
        .map(|row| format!("{}:{}", row.steps, synthetic_address(row.address)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syrinx_core::{HistoryChange, HistoryReport};

    #[test]
    fn a_deadline_names_the_step_expected_actual_rows_and_last_navigation_events() {
        let mut trace = Trace::default();
        let mut list = NavigationList::default();
        for (address, back) in [
            ("http://127.0.0.1:43210/p/40", false),
            ("http://127.0.0.1:43210/p/41", true),
            ("http://127.0.0.1:43210/p/42", true),
        ] {
            let report = HistoryReport {
                change: HistoryChange::NewDocument {
                    user_initiated: true,
                },
                address: address.into(),
                can_go_back: back,
                can_go_forward: false,
            };
            list.observe(&report);
            trace.record(&BrowserEvent::History { id: 1, report });
        }
        let landed = HistoryReport {
            change: HistoryChange::BackOrForward,
            address: "http://127.0.0.1:43210/p/41".into(),
            can_go_back: true,
            can_go_forward: true,
        };
        list.observe(&landed);
        trace.record(&BrowserEvent::History {
            id: 1,
            report: landed,
        });
        trace.record(&BrowserEvent::Loading {
            id: 1,
            loading: false,
        });
        trace.record(&BrowserEvent::NavigationCompleted {
            id: 1,
            success: true,
            cancelled: false,
        });
        let message = trace.timeout(
            "replaceState after load",
            Some("http://127.0.0.1:43210/p/45r"),
            &list,
        );
        for required in [
            "replaceState after load",
            "expected=http://127.0.0.1:43210/p/45r",
            "current=http://127.0.0.1:43210/p/41",
            "back=[",
            "1:http://127.0.0.1:43210/p/40",
            "forward=[",
            "1:http://127.0.0.1:43210/p/42",
            "History tab=1",
            "Loading tab=1 loading=false",
            "NavigationCompleted tab=1 success=true cancelled=false",
        ] {
            assert!(
                message.contains(required),
                "Missing {required} in {message}"
            );
        }
    }

    #[test]
    fn unexpected_urls_and_titles_never_enter_a_timeout() {
        let mut trace = Trace::default();
        trace.record(&BrowserEvent::Url {
            id: 1,
            url: "https://private-sentinel.example.test/path".into(),
        });
        trace.record(&BrowserEvent::Title {
            id: 1,
            title: "synthetic-private-title".into(),
        });
        let message = trace.timeout(
            "privacy boundary",
            Some("https://unexpected.example.test/"),
            &NavigationList::default(),
        );
        assert!(!message.contains("sentinel"));
        assert!(!message.contains("synthetic-private-title"));
        assert!(!message.contains("unexpected.example"));
    }
    #[test]
    fn a_row_assertion_shows_actual_expected_and_the_last_event() {
        let mut trace = Trace::default();
        let mut list = NavigationList::default();
        for (address, back) in [
            ("http://127.0.0.1:43210/p/40", false),
            ("http://127.0.0.1:43210/p/45r", true),
        ] {
            let report = HistoryReport {
                change: HistoryChange::NewDocument {
                    user_initiated: true,
                },
                address: address.into(),
                can_go_back: back,
                can_go_forward: false,
            };
            list.observe(&report);
            trace.record(&BrowserEvent::History { id: 1, report });
        }
        let error = trace
            .expect_rows(
                "after replaceState",
                HistoryDirection::Back,
                &["http://127.0.0.1:43210/p/43".into()],
                &list,
            )
            .expect_err("mismatched rows must fail");
        for required in [
            "after replaceState",
            "actual=",
            "http://127.0.0.1:43210/p/40",
            "expected=",
            "http://127.0.0.1:43210/p/43",
            "current=http://127.0.0.1:43210/p/45r",
            "History tab=1",
        ] {
            assert!(error.contains(required), "Missing {required} in {error}");
        }
    }
    #[test]
    fn a_refused_jump_prints_both_boolean_values_and_available_rows() {
        let error = Trace::default()
            .expect_jump(
                "Back of three rows",
                HistoryDirection::Back,
                3,
                false,
                &NavigationList::default(),
            )
            .expect_err("unavailable jump must fail");
        for required in [
            "Back of three rows",
            "steps=3",
            "actual=false",
            "expected=true",
            "back=[]",
            "forward=[]",
        ] {
            assert!(error.contains(required), "Missing {required} in {error}");
        }
    }

    #[test]
    fn row_assertions_compare_original_values_and_redact_only_the_diagnostic() {
        let mut list = NavigationList::default();
        for (address, back) in [
            ("https://actual-sentinel.example.test/", false),
            ("http://127.0.0.1:43210/current", true),
        ] {
            list.observe(&HistoryReport {
                change: HistoryChange::NewDocument {
                    user_initiated: true,
                },
                address: address.into(),
                can_go_back: back,
                can_go_forward: false,
            });
        }
        let error = Trace::default()
            .expect_rows(
                "redacted mismatch",
                HistoryDirection::Back,
                &["https://expected-sentinel.example.test/".into()],
                &list,
            )
            .expect_err("two distinct redacted addresses must not compare equal");
        assert!(!error.contains("actual-sentinel"));
        assert!(!error.contains("expected-sentinel"));
        assert!(error.contains("<non-synthetic address>"));
    }
}

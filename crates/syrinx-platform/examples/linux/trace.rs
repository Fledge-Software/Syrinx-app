//! Keeps bounded, synthetic-only navigation evidence for a failed native fixture step.

use std::collections::VecDeque;
use syrinx_core::{HistoryDirection, NavigationList};
use syrinx_platform::browser::BrowserEvent;

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
            if self.events.len() == 64 {
                self.events.pop_front();
            }
            self.events.push_back(line);
        }
    }

    pub fn timeout(&self, step: &str, expected: Option<&str>, list: &NavigationList) -> String {
        format!(
            "Timed out at step={step}; expected={}; current={}; back={:?}; forward={:?}\nlast navigation events:\n{}",
            expected.map(synthetic_address).unwrap_or("<none>"),
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
}

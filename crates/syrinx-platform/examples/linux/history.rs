//! Compares WebKitGTK history reports with the shared NavigationList after real loads and jumps.

use super::{
    fixture::{Fixture, reserve_profile},
    server::Server,
};
use std::time::Duration;
use syrinx_core::HistoryDirection;
use syrinx_platform::browser::{BrowserCommand, BrowserEvent};

pub fn run() -> Result<(), String> {
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    fixture.create(1, server.url("/p/40"))?;
    fixture.at("initial host load", &server.url("/p/40"))?;
    for page in 41..=44 {
        navigate(&mut fixture, &server, &format!("/p/{page}"))?;
    }
    jump(&mut fixture, &server, HistoryDirection::Back, 3, "/p/41")?;
    rows(
        &fixture,
        "after three-row Back jump",
        &server,
        HistoryDirection::Forward,
        &[42, 43, 44],
    )?;
    jump(&mut fixture, &server, HistoryDirection::Forward, 2, "/p/43")?;
    rows(
        &fixture,
        "after two-row Forward jump",
        &server,
        HistoryDirection::Back,
        &[42, 41, 40],
    )?;
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url("/p/45?replace"),
    })?;
    fixture.at("replaceState during document load", &server.url("/p/45r"))?;
    rows(
        &fixture,
        "after replaceState",
        &server,
        HistoryDirection::Back,
        &[43, 42, 41, 40],
    )?;
    // Drain the replaceState load's completion before observing the new Reload.
    fixture.barrier()?;
    fixture.send(BrowserCommand::Reload { id: 1 })?;
    let mut reload = ReloadWatch::default();
    let completed = fixture.wait(
        "Reload completion after Loading: expected tab=1 success=true cancelled=false",
        |event, _| reload.observe(event),
    );
    println!(
        "History Reload completion assertion: actual={:?}; expected=(tab=1, success=true, cancelled=false)",
        reload.completion
    );
    completed?;
    reload.finish()?;
    rows(
        &fixture,
        "after Reload",
        &server,
        HistoryDirection::Back,
        &[43, 42, 41, 40],
    )?;
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url("/p/push"),
    })?;
    fixture.at("pushState during document load", &server.url("/p/pushed"))?;
    navigate(&mut fixture, &server, "/p/pushed#fragment")?;
    fixture.expect_rows(
        "after host fragment navigation",
        HistoryDirection::Back,
        &["/p/45r", "/p/43", "/p/42", "/p/41", "/p/40"].map(|path| server.url(path)),
    )?;
    fixture.expect_rows(
        "after host fragment navigation",
        HistoryDirection::Forward,
        &[],
    )?;
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url("/p/302"),
    })?;
    fixture.at("HTTP 302 landing", &server.url("/p/redirected"))?;
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url("/p/script-redirect"),
    })?;
    fixture.at(
        "script redirect source commit",
        &server.url("/p/script-redirect"),
    )?;
    fixture.at("script redirect landing", &server.url("/p/redirected"))?;
    fixture.stop(Duration::from_secs(5))?;
    server.finish()?;
    println!(
        "Linux navigation_history_spike: real loads, 3/2-row jumps, replaceState, reload, pushState, fragment and redirects passed."
    );
    Ok(())
}

/// Requires this tab's Reload to start before accepting its completion.
#[derive(Default)]
struct ReloadWatch {
    loading: bool,
    completion: Option<(u64, bool, bool)>,
}

impl ReloadWatch {
    fn observe(&mut self, event: &BrowserEvent) -> bool {
        if matches!(
            event,
            BrowserEvent::Loading {
                id: 1,
                loading: true
            }
        ) {
            self.loading = true;
        }
        let BrowserEvent::NavigationCompleted {
            id: 1,
            success,
            cancelled,
        } = event
        else {
            return false;
        };
        if !self.loading {
            return false;
        }
        self.completion = Some((1, *success, *cancelled));
        true
    }

    fn finish(&self) -> Result<(), String> {
        match self.completion {
            Some((1, true, false)) => Ok(()),
            actual => Err(format!("Reload failed: actual={actual:?}")),
        }
    }
}

fn navigate(fixture: &mut Fixture, server: &Server, path: &str) -> Result<(), String> {
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url(path),
    })?;
    fixture.at(&format!("host Navigate to {path}"), &server.url(path))
}

fn jump(
    fixture: &mut Fixture,
    server: &Server,
    direction: HistoryDirection,
    steps: usize,
    path: &str,
) -> Result<(), String> {
    fixture.start_jump(
        &format!("{direction:?} jump of {steps} rows"),
        direction,
        steps,
    )?;
    fixture.send(BrowserCommand::GoToOffset {
        id: 1,
        direction,
        steps: steps as u32,
    })?;
    fixture.at(
        &format!("{direction:?} jump of {steps} rows"),
        &server.url(path),
    )
}

fn rows(
    fixture: &Fixture,
    step: &str,
    server: &Server,
    direction: HistoryDirection,
    pages: &[u32],
) -> Result<(), String> {
    let expected: Vec<_> = pages
        .iter()
        .map(|page| server.url(&format!("/p/{page}")))
        .collect();
    fixture.expect_rows(step, direction, &expected)
}

#[cfg(test)]
mod tests {
    use super::super::trace::Trace;
    use super::*;
    use syrinx_core::{HistoryChange, HistoryDirection, HistoryReport, NavigationList};

    #[test]
    fn reload_ignores_prior_completion_until_its_own_loading() {
        let mut reload = ReloadWatch::default();
        let stale = BrowserEvent::NavigationCompleted {
            id: 1,
            success: true,
            cancelled: false,
        };
        assert!(
            !reload.observe(&stale),
            "the previous load has not been drained yet"
        );
        assert!(!reload.observe(&BrowserEvent::Loading {
            id: 1,
            loading: false
        }));
        assert!(!reload.observe(&stale));
        assert!(!reload.observe(&BrowserEvent::Loading {
            id: 2,
            loading: true
        }));
        assert!(
            !reload.observe(&stale),
            "another tab cannot start this Reload"
        );
        assert!(!reload.observe(&BrowserEvent::Loading {
            id: 1,
            loading: true
        }));
        assert!(!reload.observe(&BrowserEvent::NavigationCompleted {
            id: 2,
            success: true,
            cancelled: false,
        }));
        assert!(!reload.observe(&BrowserEvent::NavigationCompleted {
            id: 2,
            success: false,
            cancelled: true,
        }));
        assert!(reload.observe(&stale));
        reload.finish().expect("this Reload succeeded");
    }

    #[test]
    fn reload_failure_ends_the_wait_immediately() {
        let mut reload = ReloadWatch::default();
        assert!(!reload.observe(&BrowserEvent::Loading {
            id: 1,
            loading: true
        }));
        assert!(
            reload.observe(&BrowserEvent::NavigationCompleted {
                id: 1,
                success: false,
                cancelled: false,
            }),
            "a failed Reload must not wait for the deadline"
        );
        assert!(reload.finish().is_err());
    }

    #[test]
    fn reload_cancellation_cannot_pass_as_success() {
        for success in [false, true] {
            let mut reload = ReloadWatch::default();
            assert!(!reload.observe(&BrowserEvent::Loading {
                id: 1,
                loading: true
            }));
            assert!(reload.observe(&BrowserEvent::NavigationCompleted {
                id: 1,
                success,
                cancelled: true,
            }));
            assert!(
                reload.finish().is_err(),
                "cancelled completion is not Reload success"
            );
        }
    }

    #[test]
    fn replace_state_preserves_the_authored_jump_and_row_sequence() {
        let mut list = NavigationList::default();
        let trace = Trace::default();
        // Build the same five committed entries as the native fixture.
        for (index, address) in [
            "http://127.0.0.1:43210/p/40",
            "http://127.0.0.1:43210/p/41",
            "http://127.0.0.1:43210/p/42",
            "http://127.0.0.1:43210/p/43",
            "http://127.0.0.1:43210/p/44",
        ]
        .iter()
        .enumerate()
        {
            list.observe(&HistoryReport {
                change: HistoryChange::NewDocument {
                    user_initiated: true,
                },
                address: (*address).into(),
                can_go_back: index != 0,
                can_go_forward: false,
            });
        }
        assert!(
            list.start_jump(HistoryDirection::Back, 3),
            "Back jump: actual=false expected=true; list={list:?}"
        );
        list.observe(&HistoryReport {
            change: HistoryChange::BackOrForward,
            address: "http://127.0.0.1:43210/p/41".into(),
            can_go_back: true,
            can_go_forward: true,
        });
        trace
            .expect_rows(
                "Back landed on 41",
                HistoryDirection::Forward,
                &[
                    "http://127.0.0.1:43210/p/42".into(),
                    "http://127.0.0.1:43210/p/43".into(),
                    "http://127.0.0.1:43210/p/44".into(),
                ],
                &list,
            )
            .expect("literal Forward order");

        // Branch from 43: the old Forward entry 44 must be discarded by the new document.
        assert!(
            list.start_jump(HistoryDirection::Forward, 2),
            "Forward jump: actual=false expected=true; list={list:?}"
        );
        list.observe(&HistoryReport {
            change: HistoryChange::BackOrForward,
            address: "http://127.0.0.1:43210/p/43".into(),
            can_go_back: true,
            can_go_forward: true,
        });
        list.observe(&HistoryReport {
            change: HistoryChange::NewDocument {
                user_initiated: true,
            },
            address: "http://127.0.0.1:43210/p/45?replace".into(),
            can_go_back: true,
            can_go_forward: false,
        });
        list.observe(&HistoryReport {
            change: HistoryChange::SamePage { navigating: false },
            address: "http://127.0.0.1:43210/p/45r".into(),
            can_go_back: true,
            can_go_forward: false,
        });
        assert_eq!(list.current_address(), Some("http://127.0.0.1:43210/p/45r"));
        trace
            .expect_rows(
                "replaceState retained previous documents",
                HistoryDirection::Back,
                &[
                    "http://127.0.0.1:43210/p/43".into(),
                    "http://127.0.0.1:43210/p/42".into(),
                    "http://127.0.0.1:43210/p/41".into(),
                    "http://127.0.0.1:43210/p/40".into(),
                ],
                &list,
            )
            .expect("literal Back order remains unchanged");
        trace
            .expect_rows(
                "new document discarded old Forward branch",
                HistoryDirection::Forward,
                &[],
                &list,
            )
            .expect("empty Forward list");

        // Once Back reaches 43, its Forward row is the replacement URI, never 45?replace.
        assert!(
            list.start_jump(HistoryDirection::Back, 1),
            "Back jump: actual=false expected=true; list={list:?}"
        );
        list.observe(&HistoryReport {
            change: HistoryChange::BackOrForward,
            address: "http://127.0.0.1:43210/p/43".into(),
            can_go_back: true,
            can_go_forward: true,
        });
        trace
            .expect_rows(
                "Forward exposes the replaced entry",
                HistoryDirection::Forward,
                &["http://127.0.0.1:43210/p/45r".into()],
                &list,
            )
            .expect("replaceState renamed exactly the current entry");
    }
}

//! Proves the worker's tab model keeps each tab's back and forward list and reports its reach.

use syrinx_core::{HistoryChange, HistoryDirection, HistoryReach, HistoryReport};

use super::*;

fn typed(address: &str, back: bool) -> SessionCommand {
    report(
        HistoryChange::NewDocument {
            user_initiated: true,
        },
        address,
        back,
        false,
    )
}

fn report(change: HistoryChange, address: &str, back: bool, forward: bool) -> SessionCommand {
    SessionCommand::ObserveHistory {
        id: TabId::new(1).expect("synthetic id"),
        report: HistoryReport {
            change,
            address: address.to_owned(),
            can_go_back: back,
            can_go_forward: forward,
        },
    }
}

fn reach(events: &mut UnboundedReceiver<SessionEvent>) -> (TabId, HistoryReach) {
    match next_event(events) {
        SessionEvent::HistoryReach { id, reach } => (id, reach),
        event => panic!("expected a history reach, got {event:?}"),
    }
}

#[test]
fn the_worker_keeps_the_list_and_reports_only_changes_in_reach() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let tab = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: "https://a.test/".to_owned(),
        },
    )
    .model
    .active_id
    .expect("synthetic tab is active");
    assert_eq!(tab.get(), 1);

    // The first page has nothing behind it, so the reach is unchanged and nothing is sent.
    send(&handle, typed("https://a.test/", false));
    send(&handle, typed("https://b.test/", true));
    assert_eq!(
        reach(&mut events),
        (
            tab,
            HistoryReach {
                back: true,
                forward: false
            }
        )
    );

    // A jump planned on the model checks the landing: a wrong one resets the list.
    send(
        &handle,
        SessionCommand::StartHistoryJump {
            id: tab,
            direction: HistoryDirection::Back,
            steps: 1,
        },
    );
    send(
        &handle,
        report(
            HistoryChange::BackOrForward,
            "https://elsewhere.test/",
            true,
            true,
        ),
    );
    assert_eq!(reach(&mut events), (tab, HistoryReach::default()));

    // A report for a tab that is not open changes nothing and stops nothing.
    send(
        &handle,
        SessionCommand::ObserveHistory {
            id: TabId::new(99).expect("synthetic id"),
            report: HistoryReport {
                change: HistoryChange::Unchanged,
                address: "https://stray.test/".to_owned(),
                can_go_back: false,
                can_go_forward: false,
            },
        },
    );
    stop(handle, &mut events);
    fixture.cleanup();
}

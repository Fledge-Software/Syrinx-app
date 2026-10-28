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

/// The next reach event: the tab, where its list reaches and where its engine does.
fn reach(events: &mut UnboundedReceiver<SessionEvent>) -> (TabId, HistoryReach, HistoryReach) {
    match next_event(events) {
        SessionEvent::HistoryReach { id, reach, engine } => (id, reach, engine),
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
    let behind = HistoryReach {
        back: true,
        forward: false,
    };
    assert_eq!(reach(&mut events), (tab, behind, behind));

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
    // The engine can still go both ways, so Back works though its pages are not listed.
    let both = HistoryReach {
        back: true,
        forward: true,
    };
    assert_eq!(reach(&mut events), (tab, HistoryReach::default(), both));

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

#[test]
fn a_menu_reads_the_rows_nearest_first_and_a_closed_tab_has_none() {
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
    send(&handle, typed("https://a.test/", false));
    send(&handle, typed("https://b.test/", true));
    send(&handle, typed("https://c.test/", true));
    reach(&mut events);
    send(
        &handle,
        SessionCommand::HistoryRows {
            request_id: 7,
            id: tab,
            direction: HistoryDirection::Back,
        },
    );
    let SessionEvent::HistoryRows { request_id, rows } = next_event(&mut events) else {
        panic!("expected the menu's rows");
    };
    assert_eq!(request_id, 7);
    let read: Vec<(usize, &str)> = rows
        .iter()
        .map(|row| (row.steps, row.address.as_str()))
        .collect();
    assert_eq!(read, [(1, "https://b.test/"), (2, "https://a.test/")]);
    assert!(
        !format!("{rows:?}").contains(".test"),
        "rows never print their addresses"
    );

    send(
        &handle,
        SessionCommand::HistoryRows {
            request_id: 8,
            id: TabId::new(99).expect("synthetic id"),
            direction: HistoryDirection::Back,
        },
    );
    let SessionEvent::HistoryRows { request_id, rows } = next_event(&mut events) else {
        panic!("expected an empty answer");
    };
    assert_eq!((request_id, rows.len()), (8, 0));
    stop(handle, &mut events);
    fixture.cleanup();
}

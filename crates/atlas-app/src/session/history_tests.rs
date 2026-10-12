//! Proves history recording, de-duplication, search and reopening through the session worker.

use atlas_store::HistoryVisit;

use super::*;

const FIRST: &str = "https://history-first.example.test/";
const SECOND: &str = "https://history-second.example.test/";
const LATER: &str = "https://history-later.example.test/";
const FAILED: &str = "https://history-failed.example.test/";

fn observe(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    id: TabId,
    title: Option<&str>,
    address: Option<&str>,
) -> SessionSnapshot {
    change(
        handle,
        events,
        SessionCommand::ObserveMetadata {
            id,
            title: title.map(str::to_owned),
            address: address.map(str::to_owned),
        },
    )
}

fn recent(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
) -> Vec<HistoryVisit> {
    send(
        handle,
        SessionCommand::RecentHistory {
            request_id: 41,
            limit: 50,
        },
    );
    match next_event(events) {
        SessionEvent::History { request_id, result } => {
            assert_eq!(request_id, 41);
            result.expect("synthetic history reads")
        }
        event => panic!("expected synthetic history, got {event:?}"),
    }
}

fn timeline(visits: &[HistoryVisit]) -> Vec<(&str, &str)> {
    visits
        .iter()
        .map(|visit| (visit.address.as_str(), visit.title.as_str()))
        .collect()
}

fn history_hits(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    text: &str,
) -> SearchResults {
    send(
        handle,
        SessionCommand::Query {
            request_id: 43,
            text: text.to_owned(),
            scope: Scope::History,
            policy: query_policy(),
        },
    );
    let SessionEvent::Search { request_id, result } = next_event(events) else {
        panic!("expected synthetic history search result")
    };
    assert_eq!(request_id, 43);
    result.expect("synthetic history search succeeds")
}

#[test]
fn observed_navigation_records_each_visit_once_and_survives_restart() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let tab = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: FIRST.to_owned(),
        },
    )
    .model
    .active_id
    .expect("synthetic tab is active");
    // Opening a tab is intent; the page's own report of its address is the visit.
    assert!(recent(&handle, &mut events).is_empty());

    // Replayed address and title callbacks change nothing after their first delivery.
    observe(&handle, &mut events, tab, None, Some(FIRST));
    observe(&handle, &mut events, tab, None, Some(FIRST));
    observe(&handle, &mut events, tab, Some("Synthetic first"), None);
    observe(&handle, &mut events, tab, Some("Synthetic first"), None);
    // A title is written once it settles; the next command finds it settled.
    thread::sleep(history::TITLE_SETTLE + Duration::from_millis(100));
    observe(
        &handle,
        &mut events,
        tab,
        Some("Synthetic second"),
        Some(SECOND),
    );
    // A fragment, or a quick scripted return through Home, is not another visit.
    observe(&handle, &mut events, tab, None, Some(FIRST));
    observe(&handle, &mut events, tab, None, Some("about:blank"));
    observe(&handle, &mut events, tab, None, Some(FIRST));
    observe(
        &handle,
        &mut events,
        tab,
        None,
        Some(&format!("{FIRST}#part")),
    );
    // A navigation that fails retracts the visit its address change recorded.
    observe(&handle, &mut events, tab, None, Some(FAILED));
    send(
        &handle,
        SessionCommand::ObserveNavigation {
            id: tab,
            success: false,
        },
    );
    let visits = recent(&handle, &mut events);
    assert_eq!(
        timeline(&visits),
        [(SECOND, "Synthetic second"), (FIRST, "Synthetic first")]
    );
    assert_eq!(
        history_hits(&handle, &mut events, "history-failed")
            .counts
            .history,
        0
    );

    // A late callback for a closed tab records nothing.
    change(&handle, &mut events, SessionCommand::CloseTab(tab));
    observe(
        &handle,
        &mut events,
        tab,
        Some("Synthetic late"),
        Some(LATER),
    );
    assert_eq!(recent(&handle, &mut events), visits);

    // The catalogue holds one row per address, and opening it starts a new tab there.
    let results = history_hits(&handle, &mut events, "history-first");
    assert_eq!(results.counts.history, 1);
    let Target::History(entry) = results.hits[0].target else {
        panic!("history search returns a history row");
    };
    assert_eq!(results.hits[0].title, "Synthetic first");
    let opened = change(&handle, &mut events, SessionCommand::OpenHistory(entry));
    let reopened = opened.model.active_id.expect("history opens a tab");
    assert_eq!(opened.created, vec![reopened]);
    assert_eq!(opened.model.tabs[0].metadata.address, FIRST);
    stop(handle, &mut events);

    // After restart, history and its search rows return; the restored page's reload is not a visit.
    let (handle, mut events) = fixture.start();
    state(&mut events);
    assert_eq!(recent(&handle, &mut events), visits);
    assert_eq!(
        history_hits(&handle, &mut events, "history").counts.history,
        2
    );
    observe(&handle, &mut events, reopened, None, Some(FIRST));
    assert_eq!(recent(&handle, &mut events), visits);
    observe(&handle, &mut events, reopened, None, Some(LATER));
    assert_eq!(recent(&handle, &mut events)[0].address, LATER);
    assert_eq!(
        history_hits(&handle, &mut events, "history").counts.history,
        3
    );
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn opening_a_missing_history_entry_is_a_recoverable_failure() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    let initial = state(&mut events);
    let missing = NonZeroU64::new(77).expect("synthetic history identity");
    send(&handle, SessionCommand::OpenHistory(missing));
    assert!(matches!(
        next_event(&mut events),
        SessionEvent::Failure {
            error: SessionError::HistoryUnavailable,
            fatal: false,
        }
    ));
    let unchanged = change(&handle, &mut events, SessionCommand::ReopenClosed);
    assert_eq!(unchanged.model, initial.model);
    stop(handle, &mut events);
    fixture.cleanup();
}

fn clear(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    start: i64,
    end: i64,
) -> usize {
    send(
        handle,
        SessionCommand::ClearHistory {
            request_id: 47,
            start,
            end,
        },
    );
    match next_event(events) {
        SessionEvent::HistoryCleared { request_id, result } => {
            assert_eq!(request_id, 47);
            result.expect("synthetic clear succeeds")
        }
        event => panic!("expected a synthetic clear result, got {event:?}"),
    }
}

#[test]
fn clearing_history_updates_search_at_once_and_stays_cleared_after_restart() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let tab = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: FIRST.to_owned(),
        },
    )
    .model
    .active_id
    .expect("synthetic tab is active");
    observe(&handle, &mut events, tab, None, Some(FIRST));
    observe(&handle, &mut events, tab, None, Some(SECOND));
    assert_eq!(recent(&handle, &mut events).len(), 2);
    // The open tab's own row also matches, so compare against the count before clearing.
    let before = history_hits(&handle, &mut events, "history").counts.total();

    // A range before any visit removes nothing; the current one removes both addresses.
    assert_eq!(clear(&handle, &mut events, 0, 1), 0);
    let now = utc_seconds().expect("synthetic clock");
    assert_eq!(clear(&handle, &mut events, now - 3_600, now + 1), 2);
    assert!(recent(&handle, &mut events).is_empty());
    assert_eq!(
        history_hits(&handle, &mut events, "history").counts.total(),
        before - 2
    );

    // A failed navigation after the clear retracts nothing that was already cleared,
    // and the next visit records normally.
    send(
        &handle,
        SessionCommand::ObserveNavigation {
            id: tab,
            success: false,
        },
    );
    observe(&handle, &mut events, tab, None, Some(LATER));
    assert_eq!(timeline(&recent(&handle, &mut events)), [(LATER, "")]);
    stop(handle, &mut events);

    // The journal and the database agree after a restart; nothing comes back.
    let (handle, mut events) = fixture.start();
    state(&mut events);
    assert_eq!(timeline(&recent(&handle, &mut events)), [(LATER, "")]);
    let results = history_hits(&handle, &mut events, "history");
    assert!(
        results
            .hits
            .iter()
            .all(|hit| hit.address != FIRST && hit.address != SECOND)
    );
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn an_invalid_clear_range_is_refused_without_stopping_the_session() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(
        &handle,
        SessionCommand::ClearHistory {
            request_id: 48,
            start: 10,
            end: 10,
        },
    );
    match next_event(&mut events) {
        SessionEvent::HistoryCleared { request_id, result } => {
            assert_eq!(request_id, 48);
            assert!(matches!(result, Err(SessionError::Deletion(_))));
        }
        event => panic!("expected a refused synthetic clear, got {event:?}"),
    }
    assert!(recent(&handle, &mut events).is_empty());
    stop(handle, &mut events);
    fixture.cleanup();
}

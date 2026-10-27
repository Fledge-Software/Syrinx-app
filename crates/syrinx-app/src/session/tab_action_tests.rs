//! Proves #95's tab actions through a real worker: one save per action, a catalogue that
//! follows the closes, and stale identities reported as recoverable errors.

use syrinx_core::TabError;

use super::*;

/// Opens a synthetic page and returns the tab it opened.
fn open_page(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    name: &str,
) -> TabId {
    let opened = change(
        handle,
        events,
        SessionCommand::NewTab {
            address: format!("https://{name}.tab-actions.example.test/"),
        },
    );
    assert!(
        opened.reopened.is_empty(),
        "a new tab is the owner's own choice"
    );
    opened.created[0]
}

/// Every tab and saved row the palette's blank query lists.
fn catalogued(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    request_id: u64,
) -> Vec<Target> {
    send(
        handle,
        SessionCommand::Query {
            request_id,
            text: String::new(),
            scope: Scope::Everything,
            policy: query_policy(),
        },
    );
    let SessionEvent::Search {
        request_id: answered,
        result: Ok(results),
    } = next_event(events)
    else {
        panic!("expected a synthetic catalogue listing")
    };
    assert_eq!(answered, request_id);
    results.hits.into_iter().map(|hit| hit.target).collect()
}

fn tab_ids(snapshot: &SessionSnapshot) -> Vec<TabId> {
    snapshot.model.tabs.iter().map(|tab| tab.id).collect()
}

fn assert_recoverable_unknown(events: &mut UnboundedReceiver<SessionEvent>, stale: TabId) {
    let event = next_event(events);
    assert!(
        matches!(
            event,
            SessionEvent::Failure {
                error: SessionError::Model(TabError::UnknownTab(id)),
                fatal: false,
            } if id == stale
        ),
        "expected a recoverable stale-tab failure, got {event:?}"
    );
}

#[test]
fn bulk_closes_save_once_drop_closed_rows_and_keep_unloaded_saved_rows() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);

    // One saved page is unloaded, one is open, and four ordinary tabs follow.
    let unloaded = open_page(&handle, &mut events, "unloaded");
    change(&handle, &mut events, SessionCommand::SaveActive);
    let closed = change(&handle, &mut events, SessionCommand::CloseTab(unloaded));
    let unloaded_saved = closed.model.saved_pages[0].id;
    let saved_live = open_page(&handle, &mut events, "saved-live");
    let saved = change(&handle, &mut events, SessionCommand::SaveTab(saved_live));
    let live_saved = saved.model.saved_pages[1].id;
    let first = open_page(&handle, &mut events, "first");
    let keep = open_page(&handle, &mut events, "keep");
    let below = open_page(&handle, &mut events, "below");
    let lowest = open_page(&handle, &mut events, "lowest");

    // Closing below `keep` writes one revision and drops exactly the closed rows.
    let before = change(&handle, &mut events, SessionCommand::FocusTab(first));
    let after = change(&handle, &mut events, SessionCommand::CloseBelow(keep));
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(tab_ids(&after), vec![saved_live, first, keep]);
    assert_eq!(after.model.active_id, Some(keep));
    let rows = catalogued(&handle, &mut events, 1);
    for gone in [below, lowest] {
        assert!(!rows.contains(&Target::Tab(gone)));
    }
    assert!(rows.contains(&Target::Tab(first)));

    // Closing the others spares the open saved page and keeps the unloaded saved row.
    let after_others = change(&handle, &mut events, SessionCommand::CloseOthers(keep));
    assert_eq!(after_others.revision, after.revision + 1);
    assert_eq!(tab_ids(&after_others), vec![saved_live, keep]);
    assert_eq!(after_others.model.active_id, Some(keep));
    assert_eq!(after_others.model.saved_pages, saved.model.saved_pages);
    let rows = catalogued(&handle, &mut events, 2);
    assert!(!rows.contains(&Target::Tab(first)));
    assert!(rows.contains(&Target::Saved(unloaded_saved)));
    assert!(rows.contains(&Target::Saved(live_saved)));

    // Reopen restores the closed tabs top-down, starting with `first`.
    let reopened = change(&handle, &mut events, SessionCommand::ReopenClosed);
    let restored = reopened.created[0];
    assert_eq!(tab_ids(&reopened), vec![saved_live, restored, keep]);
    // Its first page must not be read: it may have been reached from an excluded site (#54).
    assert_eq!(reopened.reopened, reopened.created);
    assert!(after_others.reopened.is_empty());
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn duplicate_creates_a_selected_copy_after_its_source() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let source = open_page(&handle, &mut events, "source");
    let after_source = open_page(&handle, &mut events, "after-source");

    let duplicated = change(&handle, &mut events, SessionCommand::Duplicate(source));
    assert_eq!(duplicated.created.len(), 1);
    let copy = duplicated.created[0];
    assert_eq!(
        duplicated.reopened,
        [copy],
        "a copy is not read from its first page (#54)"
    );
    assert_eq!(tab_ids(&duplicated), vec![source, copy, after_source]);
    assert_eq!(duplicated.model.active_id, Some(copy));
    assert_eq!(
        duplicated.model.tabs[1].metadata,
        duplicated.model.tabs[0].metadata
    );
    assert!(catalogued(&handle, &mut events, 1).contains(&Target::Tab(copy)));
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn stale_tab_actions_are_recoverable_and_change_nothing() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let stale = open_page(&handle, &mut events, "stale");
    let remaining = open_page(&handle, &mut events, "remaining");
    let before = change(&handle, &mut events, SessionCommand::CloseTab(stale));

    for command in [
        SessionCommand::Duplicate(stale),
        SessionCommand::CloseOthers(stale),
        SessionCommand::CloseBelow(stale),
        SessionCommand::SaveTab(stale),
    ] {
        send(&handle, command);
        assert_recoverable_unknown(&mut events, stale);
    }

    // The worker keeps serving, and nothing was written.
    let after = change(&handle, &mut events, SessionCommand::FocusTab(remaining));
    assert_eq!(after.model, before.model);
    assert_eq!(after.revision, before.revision);
    stop(handle, &mut events);
    fixture.cleanup();
}

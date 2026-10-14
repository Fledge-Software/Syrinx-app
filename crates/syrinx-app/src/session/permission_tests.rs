//! Proves the settings list and forget commands for remembered site permissions (DW-7).

use syrinx_store::{PermissionKind, RememberedPermission};

use super::*;

const ORIGIN: &str = "https://settings.example.test";

fn remember(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    kind: PermissionKind,
    allowed: bool,
) {
    send(
        handle,
        SessionCommand::RememberPermission {
            id: 1,
            request: 1,
            origin: ORIGIN.to_owned(),
            kind,
            allowed,
        },
    );
    assert!(matches!(
        next_event(events),
        SessionEvent::Permission { result: Ok(Some(saved)), .. } if saved == allowed
    ));
}

fn listed(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    request_id: u64,
) -> Vec<RememberedPermission> {
    send(handle, SessionCommand::RememberedPermissions { request_id });
    match next_event(events) {
        SessionEvent::RememberedPermissions {
            request_id: answered,
            result: Ok(rows),
        } if answered == request_id => rows,
        event => panic!("expected the synthetic permission list, got {event:?}"),
    }
}

fn forgotten(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    request_id: u64,
    listed: RememberedPermission,
) -> bool {
    send(
        handle,
        SessionCommand::ForgetPermission { request_id, listed },
    );
    match next_event(events) {
        SessionEvent::PermissionForgotten {
            request_id: answered,
            result: Ok(removed),
        } if answered == request_id => removed,
        event => panic!("expected the synthetic forget answer, got {event:?}"),
    }
}

#[test]
fn a_listed_choice_is_forgotten_once_and_stays_forgotten_after_restart() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    remember(&handle, &mut events, PermissionKind::Camera, true);
    remember(&handle, &mut events, PermissionKind::Clipboard, false);
    let rows = listed(&handle, &mut events, 7);
    assert_eq!(rows.len(), 2);
    let camera = rows[0].clone();
    assert_eq!(
        (camera.kind, camera.allowed),
        (PermissionKind::Camera, true)
    );

    // A double click sends the same forget twice; the second finds nothing.
    assert!(forgotten(&handle, &mut events, 8, camera.clone()));
    assert!(!forgotten(&handle, &mut events, 9, camera));
    stop(handle, &mut events);

    let (handle, mut events) = fixture.start();
    state(&mut events);
    let rows = listed(&handle, &mut events, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].kind, rows[0].allowed),
        (PermissionKind::Clipboard, false)
    );
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn a_stale_forget_keeps_the_choice_the_owner_made_since() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    remember(&handle, &mut events, PermissionKind::Location, false);
    let shown = listed(&handle, &mut events, 1).remove(0);
    remember(&handle, &mut events, PermissionKind::Location, true);

    assert!(!forgotten(&handle, &mut events, 2, shown));
    let rows = listed(&handle, &mut events, 3);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].allowed, "the newer allow is still remembered");
    stop(handle, &mut events);
    fixture.cleanup();
}

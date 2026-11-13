//! Proves the session starts the plugin runtime from its own store at start-up (#136), and
//! that a grant it cannot read never stops browsing.

use syrinx_plugin_host::owner::OwnerError;

use super::*;

/// Starts and stops a fresh worker, so the database exists at the current schema.
fn create_database(fixture: &Fixture) {
    let (handle, mut events) = fixture.start();
    state(&mut events);
    stop(handle, &mut events);
}

#[test]
fn an_unreadable_grant_is_reported_and_browsing_goes_on() {
    let fixture = Fixture::new();
    create_database(&fixture);
    let connection =
        rusqlite::Connection::open(fixture.0.join("syrinx.db")).expect("open synthetic database");
    connection
        .execute_batch(
            "INSERT INTO plugin_install VALUES
                 ('synthetic', '0.1.0', zeroblob(32), zeroblob(32), 'key', 1, 0, 0);
             INSERT INTO plugin_grant(plugin_id, capability, scope_json, granted_at,
                 granted_for_manifest)
                 VALUES ('synthetic', 'Damaged', '{}', 0, zeroblob(32));",
        )
        .expect("seed a damaged synthetic grant");
    assert!(connection.close().is_ok(), "close synthetic connection");

    let (handle, mut events) = fixture.start();
    let reported = next_any_event(&mut events);
    let expected = SessionError::Plugins(OwnerError::Store(StoreError::InvalidEncoding(
        EncodingField::Plugin,
    )));
    assert!(
        matches!(&reported, SessionEvent::Failure { error, fatal: false } if *error == expected),
        "{reported:?}"
    );
    state(&mut events);
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: "about:blank".to_owned(),
        },
    );
    assert_eq!(opened.model.tabs.len(), 1);
    stop(handle, &mut events);
    fixture.cleanup();
}

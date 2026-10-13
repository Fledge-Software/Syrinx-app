//! Detects a linked SQLite build that lacks the reviewed recovery fixes.

use rusqlite::Connection;

#[test]
fn linked_engine_matches_the_reviewed_maintenance_source() {
    // Check the running C library, not just the Rust package or generated constants.
    let connection = Connection::open_in_memory().expect("open synthetic in-memory database");
    let (version, source): (String, String) = connection
        .query_row("SELECT sqlite_version(), sqlite_source_id()", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("read linked SQLite identity");
    assert_eq!(version, "3.53.4");
    assert_eq!(
        source,
        "2026-07-24 19:02:57 bf7c7f30031888f4e796e429ab3978879485813aaca6f641c7b33e4e09459bcc"
    );
    assert!(connection.close().is_ok(), "close synthetic database");
}

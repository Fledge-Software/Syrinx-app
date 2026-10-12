//! Proves history persistence, its version 3 migration and D47 range clears on disposable databases.

use std::fs;
use std::num::NonZeroU64;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use super::{TestDirectory, close, fixture};
use crate::deletion::{DeletionCategory, DeletionLog, DeletionRange};
use crate::{EncodingField, HistoryVisit, Store, StoreError, StoreOperation, records, schema};

const FIRST: &str = "https://history-first.example.test/";
const SECOND: &str = "https://history-second.example.test/";
const THIRD: &str = "https://history-third.example.test/";

fn timeline(visits: &[HistoryVisit]) -> Vec<(&str, i64)> {
    visits
        .iter()
        .map(|visit| (visit.address.as_str(), visit.visited_at))
        .collect()
}

fn user_version(connection: &Connection) -> i64 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read synthetic schema version")
}

fn object_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .expect("inspect synthetic schema")
        == 1
}

/// Creates the version 2 (folders) schema with the fixture session at revision 1.
fn folders_database(directory: &TestDirectory) -> Connection {
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let initial = schema::initial_schema();
    let folders = schema::folders_migration();
    let migrations = Migrations::new(vec![
        M::up(&initial).foreign_key_check(),
        M::up(&folders).foreign_key_check(),
    ]);
    schema::apply_migrations(&mut connection, &migrations).expect("create the folders schema");
    assert_eq!(
        records::save(&mut connection, 0, &fixture()),
        Ok(1),
        "seed the synthetic version 2 session"
    );
    connection
}

#[test]
fn visits_merge_by_address_list_newest_first_and_survive_restart() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("new store opens");
    let first = store
        .record_visit(FIRST.to_owned(), String::new(), 100)
        .expect("first visit records");
    assert_eq!((first.visit_count, first.last_visited), (1, 100));
    assert!(first.title.is_empty());
    store
        .record_visit(SECOND.to_owned(), "Synthetic second".to_owned(), 200)
        .expect("second visit records");
    let again = store
        .record_visit(FIRST.to_owned(), "Synthetic first".to_owned(), 300)
        .expect("repeat visit records");
    assert_eq!(again.id, first.id);
    assert_eq!((again.visit_count, again.last_visited), (2, 300));
    assert_eq!(again.title, "Synthetic first");

    // A visit reported before its title keeps the title already known for that address.
    let untitled = store
        .record_visit(SECOND.to_owned(), String::new(), 400)
        .expect("untitled repeat records");
    assert_eq!(untitled.title, "Synthetic second");
    let recent = store.recent_visits(10).expect("recent visits read");
    assert_eq!(
        timeline(&recent),
        [(SECOND, 400), (FIRST, 300), (SECOND, 200), (FIRST, 100)]
    );
    assert_eq!(store.recent_visits(2).expect("limited read").len(), 2);

    // Retitling changes one entry; a missing entry is not recreated.
    let retitled = store
        .retitle_history(first.id, "Synthetic retitled".to_owned())
        .expect("retitle succeeds")
        .expect("entry exists");
    assert_eq!(retitled.title, "Synthetic retitled");
    let missing = NonZeroU64::new(999).expect("synthetic identity is nonzero");
    assert_eq!(
        store.retitle_history(missing, "Synthetic late title".to_owned()),
        Ok(None)
    );
    // History writes leave the session's compare-and-save revision alone.
    assert_eq!(store.load().expect("session loads").revision, 0);
    store.shutdown().expect("store closes");

    let store = Store::open(directory.lease()).expect("store reopens");
    let entries = store.history_entries().expect("entries read");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], retitled);
    assert_eq!((entries[1].visit_count, entries[1].last_visited), (2, 400));
    assert_eq!(
        timeline(&store.recent_visits(10).expect("recent visits reread")),
        timeline(&recent)
    );
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn invalid_visits_are_refused_without_changing_history() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("new store opens");
    let invalid = Err(StoreError::InvalidEncoding(EncodingField::History));
    assert_eq!(
        store.record_visit(String::new(), "Synthetic".to_owned(), 1),
        invalid
    );
    assert_eq!(
        store.record_visit(FIRST.to_owned(), "Synthetic".to_owned(), -1),
        invalid
    );
    assert_eq!(store.history_entries(), Ok(Vec::new()));
    assert_eq!(store.recent_visits(10), Ok(Vec::new()));
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn upgrade_from_folders_schema_keeps_the_session_and_starts_empty_history() {
    let directory = TestDirectory::new();
    close(folders_database(&directory));

    let store = Store::open(directory.lease()).expect("version 2 database upgrades");
    let stored = store.load().expect("upgraded session loads");
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.snapshot, fixture());
    assert_eq!(store.history_entries(), Ok(Vec::new()));
    store
        .record_visit(FIRST.to_owned(), "Synthetic upgraded".to_owned(), 10)
        .expect("upgraded database records history");
    store.shutdown().expect("store closes");

    let connection = directory.connection();
    assert_eq!(user_version(&connection), schema::SCHEMA_VERSION);
    assert_eq!(schema::SCHEMA_VERSION, 3);
    close(connection);
    let store = Store::open(directory.lease()).expect("version 3 database reopens");
    assert_eq!(store.load().expect("session reloads").snapshot, fixture());
    assert_eq!(store.history_entries().expect("history reloads").len(), 1);
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn failed_history_migration_rolls_back_to_the_intact_folders_schema() {
    // A conflicting table fails the migration's own SQL; a layout gap fails its validation hook.
    for damage in [
        "CREATE TABLE history_urls(value TEXT)",
        "UPDATE saved_layout SET entry_order = 9 WHERE entry_order = 1",
    ] {
        let directory = TestDirectory::new();
        let mut connection = folders_database(&directory);
        connection
            .execute_batch(damage)
            .expect("damage synthetic version 2 database");
        let pages = |connection: &Connection| -> i64 {
            connection
                .query_row("SELECT count(*) FROM saved_pages", [], |row| row.get(0))
                .expect("count synthetic pages")
        };
        let pages_before = pages(&connection);

        let scripts = schema::MigrationScripts::new();
        assert!(matches!(
            schema::apply_migrations(&mut connection, &scripts.migrations()),
            Err(StoreError::Database {
                operation: StoreOperation::Migrate,
                ..
            })
        ));
        assert!(connection.is_autocommit());
        assert_eq!(user_version(&connection), 2);
        assert!(!object_exists(&connection, "history_visits"));
        assert!(!object_exists(&connection, "history_visits_by_url"));
        assert_eq!(pages(&connection), pages_before);
        close(connection);
        directory.cleanup();
    }
}

#[test]
fn damaged_version_2_rows_are_refused_before_migrating_or_changing_the_file() {
    let directory = TestDirectory::new();
    let connection = folders_database(&directory);
    connection
        .execute(
            "UPDATE saved_layout SET entry_order = 9 WHERE entry_order = 1",
            [],
        )
        .expect("damage synthetic layout");
    close(connection);
    let before = fs::read(directory.database_path()).expect("read synthetic bytes");
    assert_eq!(
        Store::open(directory.lease()).expect_err("damaged rows are refused"),
        StoreError::InvalidEncoding(EncodingField::SavedLayout)
    );
    assert_eq!(
        fs::read(directory.database_path()).expect("reread synthetic bytes"),
        before
    );
    directory.cleanup();
}

#[test]
fn cleared_history_stays_cleared_when_an_older_copy_is_restored_and_replayed() {
    let live = TestDirectory::new();
    let older = TestDirectory::new();
    let store = Store::open(live.lease()).expect("live store opens");
    for (address, at) in [
        (FIRST, 100),
        (SECOND, 100),
        (FIRST, 200),
        (SECOND, 300),
        (THIRD, 300),
        (FIRST, 400),
    ] {
        store
            .record_visit(address.to_owned(), "Synthetic".to_owned(), at)
            .expect("synthetic visit records");
    }
    store.shutdown().expect("live store closes");
    // A closed database file stands in for the older daily snapshot.
    fs::copy(live.database_path(), older.database_path()).expect("copy synthetic recovery point");

    // The clear is requested first, applied, then completed (D47's journal order).
    let mut log = DeletionLog::default();
    let range = DeletionRange::new(150, 350).expect("synthetic range");
    let request = log
        .request(DeletionCategory::History, range)
        .expect("request accepted");
    log.accept(request).expect("request recorded");
    let pending = log.pending().expect("clear is pending");
    let store = Store::open(live.lease()).expect("live store reopens");
    store
        .apply_history_deletions(&[pending])
        .expect("live clear applies");
    log.accept(log.completion().expect("completion prepared"))
        .expect("completion recorded");
    let cleared = store.history_entries().expect("cleared entries read");
    let cleared_visits = store.recent_visits(10).expect("cleared visits read");
    assert_eq!(
        timeline(&cleared_visits),
        [(FIRST, 400), (SECOND, 100), (FIRST, 100)]
    );
    assert_eq!(cleared.len(), 2);
    assert_eq!((cleared[0].visit_count, cleared[0].last_visited), (2, 400));
    assert_eq!((cleared[1].visit_count, cleared[1].last_visited), (1, 100));
    store.shutdown().expect("live store closes");

    // The older copy still holds the cleared visits until the completed clears replay.
    let restored = Store::open(older.lease()).expect("older copy opens");
    assert_eq!(restored.history_entries().expect("older entries").len(), 3);
    let replay = log.replay_after(0).expect("completed clears replay");
    restored
        .apply_history_deletions(replay)
        .expect("replay applies");
    assert_eq!(restored.history_entries(), Ok(cleared.clone()));
    assert_eq!(restored.recent_visits(10), Ok(cleared_visits.clone()));
    // Replay is idempotent, so an interrupted restore may safely repeat it.
    restored
        .apply_history_deletions(replay)
        .expect("repeated replay applies");
    assert_eq!(restored.history_entries(), Ok(cleared));
    restored.shutdown().expect("restored store closes");
    live.cleanup();
    older.cleanup();
}

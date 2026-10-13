//! Proves history persistence, its version 4 migration and D47 range clears on disposable databases.

use std::fs;
use std::num::NonZeroU64;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use super::{TestDirectory, close, fixture};
use crate::deletion::{DeletionCategory, DeletionError, DeletionLog, DeletionRange};
use crate::{
    EncodingField, HistoryVisit, Store, StoreError, StoreOperation, records,
    replay_history_deletions, schema,
};

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
    seeded_database(directory, false)
}

/// Creates the version 3 (preferences) schema, the one history migrates from.
fn preferences_database(directory: &TestDirectory) -> Connection {
    seeded_database(directory, true)
}

/// Builds a pre-history schema and seeds the fixture session at revision 1.
fn seeded_database(directory: &TestDirectory, with_preferences: bool) -> Connection {
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let initial = schema::initial_schema();
    let folders = schema::folders_migration();
    let preferences = schema::preferences_migration();
    let mut steps = vec![
        M::up(&initial).foreign_key_check(),
        M::up(&folders).foreign_key_check(),
    ];
    if with_preferences {
        steps.push(M::up(&preferences));
    }
    let migrations = Migrations::new(steps);
    schema::apply_migrations(&mut connection, &migrations).expect("create the older schema");
    assert_eq!(
        records::save(&mut connection, 0, &fixture(), records::SavedRows::Replace),
        Ok(1),
        "seed the synthetic older session"
    );
    connection
}

#[test]
fn visits_merge_by_address_list_newest_first_and_survive_restart() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("new store opens");
    let first = store
        .record_visit(FIRST.to_owned(), String::new(), 100)
        .expect("first visit records")
        .entry;
    assert_eq!((first.visit_count, first.last_visited), (1, 100));
    assert!(first.title.is_empty());
    store
        .record_visit(SECOND.to_owned(), "Synthetic second".to_owned(), 200)
        .expect("second visit records");
    let again = store
        .record_visit(FIRST.to_owned(), "Synthetic first".to_owned(), 300)
        .expect("repeat visit records")
        .entry;
    assert_eq!(again.id, first.id);
    assert_eq!((again.visit_count, again.last_visited), (2, 300));
    assert_eq!(again.title, "Synthetic first");

    // A visit reported before its title keeps the title already known for that address.
    let untitled = store
        .record_visit(SECOND.to_owned(), String::new(), 400)
        .expect("untitled repeat records")
        .entry;
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
fn removing_a_visit_recomputes_its_address_or_drops_it() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("new store opens");
    let older = store
        .record_visit(FIRST.to_owned(), "Synthetic first".to_owned(), 100)
        .expect("older visit records");
    let newer = store
        .record_visit(FIRST.to_owned(), String::new(), 200)
        .expect("newer visit records");
    let only = store
        .record_visit(SECOND.to_owned(), String::new(), 300)
        .expect("single visit records");

    // A retracted visit leaves its address's earlier visit and title.
    let remaining = store
        .remove_visit(newer.visit)
        .expect("visit removes")
        .expect("address remains");
    assert_eq!(remaining.id, older.entry.id);
    assert_eq!((remaining.visit_count, remaining.last_visited), (1, 100));
    assert_eq!(remaining.title, "Synthetic first");

    // Retracting an address's only visit removes the address; repeating it changes nothing.
    assert_eq!(store.remove_visit(only.visit), Ok(None));
    assert_eq!(store.remove_visit(only.visit), Ok(None));
    assert_eq!(store.history_entries(), Ok(vec![remaining]));
    assert_eq!(
        timeline(&store.recent_visits(10).expect("recent visits read")),
        [(FIRST, 100)]
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
    assert_eq!(schema::SCHEMA_VERSION, 6);
    close(connection);
    let store = Store::open(directory.lease()).expect("current database reopens");
    assert_eq!(store.load().expect("session reloads").snapshot, fixture());
    assert_eq!(store.history_entries().expect("history reloads").len(), 1);
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn failed_history_migration_rolls_back_to_the_intact_preferences_schema() {
    // A conflicting table fails the migration's own SQL; a layout gap fails its validation hook.
    for damage in [
        "CREATE TABLE history_urls(value TEXT)",
        "UPDATE saved_layout SET entry_order = 9 WHERE entry_order = 1",
    ] {
        let directory = TestDirectory::new();
        let mut connection = preferences_database(&directory);
        connection
            .execute_batch(damage)
            .expect("damage synthetic version 3 database");
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
        assert_eq!(user_version(&connection), 3);
        assert!(object_exists(&connection, "settings"));
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
    assert_eq!(store.history_deletion_generation(), Ok(1));
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
    assert_eq!(restored.history_deletion_generation(), Ok(0));
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

#[test]
fn a_clear_reports_removed_and_changed_addresses_and_records_its_generation() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("new store opens");
    let first = store
        .record_visit(FIRST.to_owned(), "Synthetic first".to_owned(), 100)
        .expect("first visit records")
        .entry;
    store
        .record_visit(FIRST.to_owned(), String::new(), 300)
        .expect("repeat visit records");
    let second = store
        .record_visit(SECOND.to_owned(), String::new(), 250)
        .expect("second visit records")
        .entry;
    let third = store
        .record_visit(THIRD.to_owned(), String::new(), 50)
        .expect("third visit records")
        .entry;
    assert_eq!(store.history_deletion_generation(), Ok(0));

    let mut log = DeletionLog::default();
    let range = DeletionRange::new(200, 400).expect("synthetic range");
    let request = log
        .request(DeletionCategory::History, range)
        .expect("request accepted");
    log.accept(request).expect("request recorded");
    let pending = log.pending().expect("clear is pending");
    let deletion = store
        .apply_history_deletions(&[pending])
        .expect("clear applies");

    // The second address lost its only visit; the first keeps its older one.
    assert_eq!(deletion.visits, 2);
    assert_eq!(deletion.removed, [second.id]);
    assert_eq!(deletion.changed.len(), 1);
    let changed = &deletion.changed[0];
    assert_eq!(changed.id, first.id);
    assert_eq!((changed.visit_count, changed.last_visited), (1, 100));
    assert_eq!(changed.title, "Synthetic first");
    assert!(!format!("{deletion:?}").contains("history-first"));
    assert_eq!(
        store.history_entries().expect("entries read"),
        [changed.clone(), third]
    );
    assert_eq!(store.history_deletion_generation(), Ok(1));

    // Replaying an older request again deletes nothing and never lowers the generation.
    let empty = DeletionLog::default();
    assert!(empty.unapplied(0).expect("empty log").is_empty());
    let repeated = store
        .apply_history_deletions(&[pending])
        .expect("repeat applies");
    assert!(repeated.removed.is_empty() && repeated.changed.is_empty());
    store.shutdown().expect("store closes");
    let store = Store::open(directory.lease()).expect("store reopens");
    assert_eq!(store.history_deletion_generation(), Ok(1));
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn version_4_history_upgrades_to_an_unapplied_deletion_generation() {
    let directory = TestDirectory::new();
    let connection = preferences_database(&directory);
    let history = schema::history_migration();
    connection
        .execute_batch(&format!("{history} PRAGMA user_version = 4;"))
        .expect("create synthetic version 4 history");
    connection
        .execute(
            "INSERT INTO history_urls(address, title, visit_count, last_visited)
             VALUES (?1, 'Synthetic', 1, 10)",
            [FIRST],
        )
        .expect("seed synthetic address");
    connection
        .execute(
            "INSERT INTO history_visits(url_id, visited_at) VALUES (1, 10)",
            [],
        )
        .expect("seed synthetic visit");
    close(connection);

    let store = Store::open(directory.lease()).expect("version 4 database upgrades");
    assert_eq!(store.history_deletion_generation(), Ok(0));
    assert_eq!(store.history_entries().expect("history kept").len(), 1);
    store.shutdown().expect("store closes");
    let connection = directory.connection();
    assert_eq!(user_version(&connection), schema::SCHEMA_VERSION);
    assert!(object_exists(&connection, "history_deletion_state"));
    assert!(object_exists(&connection, "history_visits_by_time"));
    close(connection);
    directory.cleanup();
}

/// Records one completed clear of `[start, end)` in the log, as the session's journal would.
fn completed_clear(log: &mut DeletionLog, start: i64, end: i64) {
    let range = DeletionRange::new(start, end).expect("synthetic range");
    let request = log
        .request(DeletionCategory::History, range)
        .expect("request accepted");
    log.accept(request).expect("request recorded");
    log.accept(log.completion().expect("completion prepared"))
        .expect("completion recorded");
}

fn no_sidecars(directory: &TestDirectory) -> bool {
    ["syrinx.db-wal", "syrinx.db-shm", "syrinx.db-journal"]
        .iter()
        .all(|name| !directory.path.join(name).exists())
}

#[test]
fn staged_replay_clears_an_older_copy_records_its_generation_and_leaves_no_sidecars() {
    let live = TestDirectory::new();
    let staged = TestDirectory::new();
    let store = Store::open(live.lease()).expect("live store opens");
    for (address, at) in [(FIRST, 100), (SECOND, 200), (THIRD, 300)] {
        store
            .record_visit(address.to_owned(), String::new(), at)
            .expect("synthetic visit records");
    }
    store.shutdown().expect("live store closes");
    fs::copy(live.database_path(), staged.database_path()).expect("copy synthetic snapshot");

    let mut log = DeletionLog::default();
    completed_clear(&mut log, 150, 250);
    completed_clear(&mut log, 290, 310);
    replay_history_deletions(staged.lease().as_ref(), &log, 0).expect("staged replay applies");
    assert!(no_sidecars(&staged));

    let restored = Store::open(staged.lease()).expect("replayed copy opens");
    assert_eq!(restored.history_deletion_generation(), Ok(2));
    assert_eq!(
        timeline(&restored.recent_visits(10).expect("visits read")),
        [(FIRST, 100)]
    );
    restored.shutdown().expect("replayed copy closes");

    // A completion record that disagrees with the copy, or a journal missing its generation,
    // refuses the copy rather than guessing.
    assert_eq!(
        replay_history_deletions(staged.lease().as_ref(), &log, 0),
        Err(StoreError::DeletionGenerationMismatch)
    );
    assert_eq!(
        replay_history_deletions(staged.lease().as_ref(), &DeletionLog::default(), 2),
        Err(StoreError::Deletion(DeletionError::MissingGeneration))
    );
    // A copy that already has every clear is left as it was.
    replay_history_deletions(staged.lease().as_ref(), &log, 2).expect("nothing to replay");
    live.cleanup();
    staged.cleanup();
}

#[test]
fn staged_replay_clears_version_4_history_without_changing_its_schema() {
    let directory = TestDirectory::new();
    let connection = preferences_database(&directory);
    let history = schema::history_migration();
    connection
        .execute_batch(&format!(
            "{history} PRAGMA user_version = 4;
             INSERT INTO history_urls(address, title, visit_count, last_visited)
                 VALUES ('{FIRST}', '', 1, 10), ('{SECOND}', '', 1, 20);
             INSERT INTO history_visits(url_id, visited_at) VALUES (1, 10), (2, 20);"
        ))
        .expect("create synthetic version 4 history");
    close(connection);

    let mut log = DeletionLog::default();
    completed_clear(&mut log, 15, 25);
    // A version 4 copy predates generation records, so its completion record says zero.
    assert_eq!(
        replay_history_deletions(directory.lease().as_ref(), &log, 1),
        Err(StoreError::DeletionGenerationMismatch)
    );
    replay_history_deletions(directory.lease().as_ref(), &log, 0).expect("version 4 replay");
    assert!(no_sidecars(&directory));
    let connection = directory.connection();
    assert_eq!(user_version(&connection), 4);
    let remaining: i64 = connection
        .query_row("SELECT count(*) FROM history_urls", [], |row| row.get(0))
        .expect("count synthetic addresses");
    assert_eq!(remaining, 1);
    close(connection);

    // Opening migrates it to the latest version at generation zero; startup then replays again harmlessly.
    let store = Store::open(directory.lease()).expect("version 4 copy upgrades");
    assert_eq!(store.history_deletion_generation(), Ok(0));
    let replay = log.unapplied(0).expect("journal covers the copy");
    let repeated = store
        .apply_history_deletions(replay)
        .expect("replay repeats");
    assert!(repeated.removed.is_empty());
    assert_eq!(store.history_deletion_generation(), Ok(1));
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn an_expiry_ends_at_the_cutoff_or_after_at_most_its_limit_of_visits() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("new store opens");
    assert_eq!(store.history_expiry_end(1_000, 10), Ok(None));
    for (address, visited_at) in [(FIRST, 100), (FIRST, 100), (SECOND, 200), (SECOND, 900)] {
        store
            .record_visit(address.to_owned(), String::new(), visited_at)
            .expect("synthetic visit records");
    }

    // Nothing older than the cutoff; everything fits; a limit stops at the next visit.
    assert_eq!(store.history_expiry_end(100, 10), Ok(None));
    assert_eq!(store.history_expiry_end(1_000, 10), Ok(Some(1_000)));
    assert_eq!(store.history_expiry_end(1_000, 2), Ok(Some(200)));
    // Visits sharing the oldest second are removed together, so a batch is never empty.
    assert_eq!(store.history_expiry_end(1_000, 1), Ok(Some(101)));
    assert_eq!(store.history_expiry_end(1_000, 0), Ok(Some(101)));
    store.shutdown().expect("store closes");
    directory.cleanup();
}

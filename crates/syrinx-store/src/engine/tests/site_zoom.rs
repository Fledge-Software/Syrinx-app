//! Proves per-site zoom levels (#90), their version 7 migration, their survival in backups and
//! their journaled clears (#121).

use std::collections::BTreeMap;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use super::{TestDirectory, close, fixture};
use crate::deletion::{DeletionCategory, DeletionLog, DeletionRange, DeletionRequest};
use crate::{
    EncodingField, HistoryDeletion, PermissionKind, Store, StoreError, StoreOperation, records,
    schema,
};

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

/// The real scripts for versions 1 to 6, so a test can stop before version 7.
struct VersionSixScripts([String; 6]);

impl VersionSixScripts {
    fn new() -> Self {
        Self([
            schema::initial_schema(),
            schema::folders_migration(),
            schema::preferences_migration(),
            schema::history_migration(),
            schema::deletions_migration(),
            schema::permissions_migration(),
        ])
    }

    fn migrations(&self) -> Vec<M<'_>> {
        let [
            initial,
            folders,
            preferences,
            history,
            deletions,
            permissions,
        ] = &self.0;
        vec![
            M::up(initial).foreign_key_check(),
            M::up(folders).foreign_key_check(),
            M::up(preferences),
            M::up(history).foreign_key_check(),
            M::up(deletions),
            M::up(permissions),
        ]
    }
}

/// Builds the version 6 (permissions) schema with the fixture session and one remembered choice.
fn permissions_database(directory: &TestDirectory) -> Connection {
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let scripts = VersionSixScripts::new();
    schema::apply_migrations(&mut connection, &Migrations::new(scripts.migrations()))
        .expect("create synthetic version 6");
    assert_eq!(user_version(&connection), 6);
    assert_eq!(
        records::save(&mut connection, 0, &fixture(), records::SavedRows::Replace),
        Ok(1),
        "seed the synthetic version 6 session"
    );
    connection
        .execute_batch(
            "INSERT INTO site_permissions(origin, kind, allowed)
                 VALUES ('https://zoom-upgrade.example.test', 2, 0);",
        )
        .expect("seed a synthetic remembered choice");
    connection
}

fn zoomed(entries: &[(&str, f64)]) -> BTreeMap<String, f64> {
    entries
        .iter()
        .map(|(host, factor)| ((*host).to_owned(), *factor))
        .collect()
}

#[test]
fn version_6_upgrades_to_7_keeping_session_and_permissions_with_no_zoom() {
    let directory = TestDirectory::new();
    close(permissions_database(&directory));

    let store = Store::open(directory.lease()).expect("version 6 database upgrades");
    assert_eq!(store.load().expect("session kept").snapshot, fixture());
    assert_eq!(
        store.permission("https://zoom-upgrade.example.test", PermissionKind::Camera),
        Ok(Some(false)),
        "remembered permissions survive the upgrade"
    );
    assert_eq!(
        store.site_zooms(),
        Ok(BTreeMap::new()),
        "every site starts at 100%"
    );
    store.shutdown().expect("store closes");

    let connection = directory.connection();
    assert_eq!(user_version(&connection), schema::SCHEMA_VERSION);
    assert_eq!(schema::SCHEMA_VERSION, 7);
    assert!(object_exists(&connection, "site_zoom"));
    close(connection);
    directory.cleanup();
}

#[test]
fn site_zoom_hook_refuses_rows_and_rolls_back_to_the_intact_version_6_schema() {
    // The migration's own SQL succeeds here; only its hook sees the seeded row and refuses.
    let directory = TestDirectory::new();
    let mut connection = permissions_database(&directory);
    let seeded = format!(
        "{} INSERT INTO site_zoom(host, factor, last_modified) VALUES ('seeded.example.test', 1.25, 10);",
        schema::site_zoom_migration()
    );
    let scripts = VersionSixScripts::new();
    let mut migrations = scripts.migrations();
    migrations.push(M::up_with_hook(&seeded, schema::check_migrated_site_zoom));
    assert!(matches!(
        schema::apply_migrations(&mut connection, &Migrations::new(migrations)),
        Err(StoreError::Database {
            operation: StoreOperation::Migrate,
            ..
        })
    ));
    assert!(connection.is_autocommit());
    assert_eq!(user_version(&connection), 6);
    assert!(!object_exists(&connection, "site_zoom"));
    close(connection);

    // The untouched version 6 file still opens and upgrades through the real migration.
    let store = Store::open(directory.lease()).expect("intact version 6 database upgrades");
    assert_eq!(store.site_zooms(), Ok(BTreeMap::new()));
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn saved_levels_survive_restart_and_forgetting_one_keeps_the_others() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    let save = |host: &str, factor| {
        store
            .save_site_zoom(host.to_owned(), factor, 100)
            .expect("save synthetic zoom");
    };
    save("zoomed.example.test", Some(1.25));
    save("small.example.test", Some(0.75));
    save("zoomed.example.test", Some(1.5));
    store.shutdown().expect("close first store");

    let store = Store::open(directory.lease()).expect("reopen synthetic store");
    assert_eq!(
        store.site_zooms(),
        Ok(zoomed(&[
            ("small.example.test", 0.75),
            ("zoomed.example.test", 1.5)
        ]))
    );
    store
        .save_site_zoom("zoomed.example.test".to_owned(), None, 100)
        .expect("forget synthetic zoom");
    store
        .save_site_zoom("never-zoomed.example.test".to_owned(), None, 100)
        .expect("forgetting an unsaved host is not an error");
    assert_eq!(
        store.site_zooms(),
        Ok(zoomed(&[("small.example.test", 0.75)]))
    );
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn addresses_defaults_and_impossible_levels_are_refused_without_writing() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    let refused = Err(StoreError::InvalidEncoding(EncodingField::SiteZoom));
    for host in [
        "",
        "https://scheme.example.test",
        "path.example.test/private",
        "user@login.example.test",
        "Upper.Example.test",
        "space .example.test",
    ] {
        assert_eq!(
            store.save_site_zoom(host.to_owned(), Some(1.25), 100),
            refused,
            "{host:?} is not a canonical host"
        );
    }
    for factor in [1., 1.000_4, 0.2, 5.5, f64::NAN, f64::INFINITY] {
        assert_eq!(
            store.save_site_zoom("level.example.test".to_owned(), Some(factor), 100),
            refused,
            "{factor} is not a storable level"
        );
    }
    assert_eq!(store.site_zooms(), Ok(BTreeMap::new()));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn a_damaged_level_refuses_the_map_instead_of_applying_a_guess() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    store
        .save_site_zoom("damaged.example.test".to_owned(), Some(1.25), 100)
        .expect("save synthetic zoom");
    let connection = directory.connection();
    connection
        .execute_batch("PRAGMA ignore_check_constraints = ON; UPDATE site_zoom SET factor = 1.0;")
        .expect("damage synthetic level");
    close(connection);
    assert_eq!(
        store.site_zooms(),
        Err(StoreError::InvalidEncoding(EncodingField::SiteZoom))
    );
    store.shutdown().expect("close damaged synthetic store");
    directory.cleanup();
}

#[test]
fn saved_levels_survive_a_vacuum_snapshot() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    store
        .save_site_zoom("backup.example.test".to_owned(), Some(2.), 100)
        .expect("save synthetic zoom");
    store.shutdown().expect("close synthetic store");
    let connection = directory.connection();
    let snapshot = TestDirectory::new();
    connection
        .execute(
            "VACUUM INTO ?1",
            [snapshot.database_path().to_str().expect("synthetic path")],
        )
        .expect("snapshot zoom state");
    close(connection);
    let mut saved = snapshot.connection();
    schema::validate_backup(&mut saved).expect("validate zoom snapshot");
    assert_eq!(
        crate::site_zoom::load(&saved),
        Ok(zoomed(&[("backup.example.test", 2.)]))
    );
    close(saved);
    directory.cleanup();
    snapshot.cleanup();
}

#[test]
fn only_hundred_percent_counts_as_the_default() {
    assert!(crate::is_default_zoom(1.));
    assert!(crate::is_default_zoom(0.999_8));
    assert!(!crate::is_default_zoom(1.1));
    assert!(!crate::is_default_zoom(0.9));
}

/// Saves synthetic levels changed at 100, 5 000 and 9 000 Unix seconds.
fn store_with_dated_levels(directory: &TestDirectory) -> Store {
    let store = Store::open(directory.lease()).expect("synthetic store");
    for (host, factor, modified_at) in [
        ("old.example.test", 1.25, 100),
        ("middle.example.test", 1.5, 5_000),
        ("recent.example.test", 0.75, 9_000),
    ] {
        store
            .save_site_zoom(host.to_owned(), Some(factor), modified_at)
            .expect("save synthetic dated zoom");
    }
    store
}

/// Journals one `category` clear of `[start, end)` and returns the pending request, as the
/// session does before it asks the store to apply it (D47's request-then-delete order).
fn requested(
    log: &mut DeletionLog,
    category: DeletionCategory,
    start: i64,
    end: i64,
) -> DeletionRequest {
    let range = DeletionRange::new(start, end).expect("synthetic range");
    let request = log.request(category, range).expect("request accepted");
    log.accept(request).expect("request recorded");
    log.pending().expect("clear is pending")
}

fn completed(log: &mut DeletionLog) {
    log.accept(log.completion().expect("completion prepared"))
        .expect("completion recorded");
}

#[test]
fn a_ranged_zoom_clear_removes_only_levels_changed_in_that_range_and_records_its_generation() {
    let directory = TestDirectory::new();
    let store = store_with_dated_levels(&directory);
    let mut log = DeletionLog::default();
    // The range ends before 9 000 and starts at 5 000, which it includes.
    let pending = requested(&mut log, DeletionCategory::SiteZoom, 5_000, 9_000);
    let applied = store
        .apply_deletions(&[pending])
        .expect("zoom clear applies");
    assert_eq!(applied.site_zooms, 1);
    assert_eq!(applied.history, HistoryDeletion::default());
    assert_eq!(store.deletion_generation(), Ok(1));
    assert_eq!(
        store.site_zooms(),
        Ok(zoomed(&[
            ("old.example.test", 1.25),
            ("recent.example.test", 0.75)
        ]))
    );
    completed(&mut log);

    // Changing a level again moves it into a later range.
    store
        .save_site_zoom("old.example.test".to_owned(), Some(2.), 9_500)
        .expect("change synthetic zoom");
    let pending = requested(&mut log, DeletionCategory::SiteZoom, 9_000, 10_000);
    let applied = store
        .apply_deletions(&[pending])
        .expect("second zoom clear applies");
    assert_eq!(applied.site_zooms, 2);
    assert_eq!(store.site_zooms(), Ok(BTreeMap::new()));
    assert_eq!(store.deletion_generation(), Ok(2));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn an_all_time_zoom_clear_removes_every_level_and_survives_restart() {
    let directory = TestDirectory::new();
    let store = store_with_dated_levels(&directory);
    let mut log = DeletionLog::default();
    let pending = requested(&mut log, DeletionCategory::SiteZoom, 0, i64::MAX);
    let applied = store
        .apply_deletions(&[pending])
        .expect("zoom clear applies");
    assert_eq!(applied.site_zooms, 3);
    // Replaying the same request removes nothing more and never lowers the generation.
    let repeated = store
        .apply_deletions(&[pending])
        .expect("repeated zoom clear applies");
    assert_eq!(repeated.site_zooms, 0);
    assert_eq!(store.deletion_generation(), Ok(1));
    store.shutdown().expect("close first store");

    let store = Store::open(directory.lease()).expect("reopen synthetic store");
    assert_eq!(store.site_zooms(), Ok(BTreeMap::new()));
    assert_eq!(store.deletion_generation(), Ok(1));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn a_history_clear_leaves_zoom_levels_alone_and_a_zoom_clear_leaves_history_alone() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    store
        .record_visit(
            "https://visited.example.test/".to_owned(),
            String::new(),
            100,
        )
        .expect("record synthetic visit");
    store
        .save_site_zoom("zoomed.example.test".to_owned(), Some(1.25), 100)
        .expect("save synthetic zoom");
    let mut log = DeletionLog::default();

    // A history clear over the same seconds does not touch the zoom table.
    let history = requested(&mut log, DeletionCategory::History, 0, i64::MAX);
    let applied = store
        .apply_deletions(&[history])
        .expect("history clear applies");
    assert_eq!(applied.history.visits, 1);
    assert_eq!(applied.site_zooms, 0);
    assert_eq!(
        store.site_zooms(),
        Ok(zoomed(&[("zoomed.example.test", 1.25)]))
    );
    completed(&mut log);

    // A zoom clear over the same seconds does not touch history.
    store
        .record_visit(
            "https://visited-again.example.test/".to_owned(),
            String::new(),
            200,
        )
        .expect("record a later synthetic visit");
    let zoom = requested(&mut log, DeletionCategory::SiteZoom, 0, i64::MAX);
    let applied = store.apply_deletions(&[zoom]).expect("zoom clear applies");
    assert_eq!(applied.site_zooms, 1);
    assert_eq!(applied.history, HistoryDeletion::default());
    assert_eq!(store.history_entries().expect("history kept").len(), 1);
    assert_eq!(store.site_zooms(), Ok(BTreeMap::new()));
    assert_eq!(store.deletion_generation(), Ok(2));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn a_mixed_batch_applies_both_categories_in_one_call_and_advances_the_generation_once() {
    // A restored copy at generation 0 meets a journal holding a history and a zoom clear,
    // which the start-up reconcile applies in one call.
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    store
        .record_visit("https://mixed.example.test/".to_owned(), String::new(), 100)
        .expect("record synthetic visit");
    store
        .save_site_zoom("mixed.example.test".to_owned(), Some(1.25), 100)
        .expect("save synthetic zoom");
    let mut log = DeletionLog::default();
    requested(&mut log, DeletionCategory::History, 0, 150);
    completed(&mut log);
    requested(&mut log, DeletionCategory::SiteZoom, 0, 150);
    completed(&mut log);

    let replay = log.unapplied(0).expect("every clear");
    assert_eq!(replay.len(), 2);
    let applied = store.apply_deletions(replay).expect("mixed batch applies");
    assert_eq!(applied.history.visits, 1);
    assert_eq!(applied.site_zooms, 1);
    assert!(!format!("{applied:?}").contains("example.test"));
    assert_eq!(store.deletion_generation(), Ok(2));
    assert_eq!(store.history_entries(), Ok(Vec::new()));
    assert_eq!(store.site_zooms(), Ok(BTreeMap::new()));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn a_negative_change_time_is_refused() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    assert_eq!(
        store.save_site_zoom("time.example.test".to_owned(), Some(1.25), -1),
        Err(StoreError::InvalidEncoding(EncodingField::SiteZoom))
    );
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

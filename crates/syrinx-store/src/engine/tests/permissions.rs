//! Proves remembered site permissions, their version 6 migration and their survival in backups.

use std::fs;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use super::{TestDirectory, close, fixture};
use crate::{EncodingField, PermissionKind, Store, StoreError, StoreOperation, records, schema};

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

/// Builds the version 5 (deletions) schema with the fixture session and one history visit.
fn deletions_database(directory: &TestDirectory) -> Connection {
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let initial = schema::initial_schema();
    let folders = schema::folders_migration();
    let preferences = schema::preferences_migration();
    let history = schema::history_migration();
    let deletions = schema::deletions_migration();
    let migrations = Migrations::new(vec![
        M::up(&initial).foreign_key_check(),
        M::up(&folders).foreign_key_check(),
        M::up(&preferences),
        M::up(&history).foreign_key_check(),
        M::up(&deletions),
    ]);
    schema::apply_migrations(&mut connection, &migrations).expect("create synthetic version 5");
    assert_eq!(user_version(&connection), 5);
    assert_eq!(
        records::save(&mut connection, 0, &fixture(), records::SavedRows::Replace),
        Ok(1),
        "seed the synthetic version 5 session"
    );
    connection
        .execute_batch(
            "INSERT INTO history_urls(address, title, visit_count, last_visited)
                 VALUES ('https://permission-history.example.test/', 'Synthetic', 1, 10);
             INSERT INTO history_visits(url_id, visited_at) VALUES (1, 10);
             UPDATE history_deletion_state SET applied_generation = 3;",
        )
        .expect("seed synthetic history and clear generation");
    connection
}

#[test]
fn version_5_upgrades_to_6_keeping_session_history_and_clear_generation() {
    let directory = TestDirectory::new();
    close(deletions_database(&directory));

    let store = Store::open(directory.lease()).expect("version 5 database upgrades");
    let stored = store.load().expect("upgraded session loads");
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.snapshot, fixture());
    assert_eq!(store.history_entries().expect("history kept").len(), 1);
    assert_eq!(store.history_deletion_generation(), Ok(3));
    assert_eq!(
        store.permission("https://upgrade.example.test", PermissionKind::Camera),
        Ok(None),
        "an upgraded database still asks"
    );
    store.shutdown().expect("store closes");

    let connection = directory.connection();
    assert_eq!(user_version(&connection), schema::SCHEMA_VERSION);
    assert_eq!(schema::SCHEMA_VERSION, 6);
    assert!(object_exists(&connection, "site_permissions"));
    close(connection);
    directory.cleanup();
}

#[test]
fn permission_hook_refuses_rows_and_rolls_back_to_the_intact_version_5_schema() {
    // The migration's own SQL succeeds here; only its hook sees the seeded row and refuses.
    let directory = TestDirectory::new();
    let mut connection = deletions_database(&directory);
    let seeded = format!(
        "{} INSERT INTO site_permissions(origin, kind, allowed)
             VALUES ('https://seeded.example.test', 1, 1);",
        schema::permissions_migration()
    );
    let initial = schema::initial_schema();
    let folders = schema::folders_migration();
    let preferences = schema::preferences_migration();
    let history = schema::history_migration();
    let deletions = schema::deletions_migration();
    let migrations = Migrations::new(vec![
        M::up(&initial).foreign_key_check(),
        M::up(&folders).foreign_key_check(),
        M::up(&preferences),
        M::up(&history).foreign_key_check(),
        M::up(&deletions),
        M::up_with_hook(&seeded, schema::check_migrated_permissions),
    ]);
    assert!(matches!(
        schema::apply_migrations(&mut connection, &migrations),
        Err(StoreError::Database {
            operation: StoreOperation::Migrate,
            ..
        })
    ));
    assert!(connection.is_autocommit());
    assert_eq!(user_version(&connection), 5);
    assert!(!object_exists(&connection, "site_permissions"));
    assert!(object_exists(&connection, "history_deletion_state"));
    close(connection);

    // The untouched version 5 file still opens and upgrades through the real migration.
    let store = Store::open(directory.lease()).expect("intact version 5 database upgrades");
    assert_eq!(store.load().expect("session kept").snapshot, fixture());
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn failed_permission_migration_rolls_back_to_the_intact_version_5_schema() {
    // A conflicting table fails the migration's own SQL before anything commits.
    let directory = TestDirectory::new();
    let mut connection = deletions_database(&directory);
    connection
        .execute_batch("CREATE TABLE site_permissions(value TEXT)")
        .expect("damage synthetic version 5 database");

    let scripts = schema::MigrationScripts::new();
    assert!(matches!(
        schema::apply_migrations(&mut connection, &scripts.migrations()),
        Err(StoreError::Database {
            operation: StoreOperation::Migrate,
            ..
        })
    ));
    assert!(connection.is_autocommit());
    assert_eq!(user_version(&connection), 5);
    assert!(object_exists(&connection, "history_deletion_state"));
    close(connection);
    directory.cleanup();
}

#[test]
fn permission_migration_preserves_session_and_remembered_choices_survive_restart() {
    let directory = TestDirectory::new();
    close(deletions_database(&directory));
    let snapshot = fixture();
    let store = Store::open(directory.lease()).expect("migrate old schema");
    assert_eq!(
        store.load().expect("read preserved session").snapshot,
        snapshot
    );
    assert_eq!(
        store.permission(
            "https://consent.example.test",
            PermissionKind::Notifications
        ),
        Ok(None)
    );
    store
        .remember_permission(
            "https://consent.example.test".into(),
            PermissionKind::Notifications,
            true,
        )
        .expect("remember allow");
    store
        .remember_permission(
            "https://consent.example.test".into(),
            PermissionKind::Clipboard,
            false,
        )
        .expect("remember deny");
    assert_eq!(store.load().expect("session revision").revision, 1);
    store.shutdown().expect("close first store");
    let store = Store::open(directory.lease()).expect("reopen migrated store");
    assert_eq!(
        store.permission(
            "https://consent.example.test",
            PermissionKind::Notifications
        ),
        Ok(Some(true))
    );
    assert_eq!(
        store.permission("https://consent.example.test", PermissionKind::Clipboard),
        Ok(Some(false))
    );
    assert_eq!(
        store.permission("http://consent.example.test", PermissionKind::Notifications),
        Ok(None)
    );
    assert_eq!(
        store.permission(
            "https://consent.example.test:8443",
            PermissionKind::Notifications
        ),
        Ok(None)
    );
    store
        .remember_permission(
            "https://consent.example.test".into(),
            PermissionKind::Notifications,
            false,
        )
        .expect("replace choice");
    assert_eq!(
        store.permission(
            "https://consent.example.test",
            PermissionKind::Notifications
        ),
        Ok(Some(false))
    );
    assert!(
        store
            .remember_permission(
                "https://consent.example.test/private".into(),
                PermissionKind::Notifications,
                true
            )
            .is_err()
    );
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn old_backup_validation_is_read_only_and_permissions_survive_vacuum_snapshot() {
    let directory = TestDirectory::new();
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure old backup");
    let initial = schema::initial_schema();
    schema::apply_migrations(&mut connection, &Migrations::new(vec![M::up(&initial)]))
        .expect("create old backup");
    close(connection);
    let before = fs::read(directory.database_path()).expect("old backup bytes");
    let mut connection = directory.connection();
    schema::validate_backup(&mut connection).expect("validate schema 1 backup");
    close(connection);
    assert_eq!(
        fs::read(directory.database_path()).expect("validated bytes"),
        before
    );
    let store = Store::open(directory.lease()).expect("migrate restored old backup");
    store
        .remember_permission(
            "https://backup.example.test".into(),
            crate::PermissionKind::Camera,
            false,
        )
        .expect("save consent");
    store.shutdown().expect("close restored store");
    let connection = directory.connection();
    let snapshot = TestDirectory::new();
    connection
        .execute(
            "VACUUM INTO ?1",
            [snapshot.database_path().to_str().expect("synthetic path")],
        )
        .expect("snapshot permission state");
    close(connection);
    let mut saved = snapshot.connection();
    schema::validate_backup(&mut saved).expect("validate permission snapshot");
    assert_eq!(
        crate::permissions::load(
            &saved,
            "https://backup.example.test",
            crate::PermissionKind::Camera
        ),
        Ok(Some(false))
    );
    close(saved);
    directory.cleanup();
    snapshot.cleanup();
}

#[test]
fn corrupted_permission_is_never_interpreted_as_a_grant() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    store
        .remember_permission(
            "https://corrupt.example.test".into(),
            crate::PermissionKind::Camera,
            false,
        )
        .expect("save deny");
    let connection = directory.connection();
    connection
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON; UPDATE site_permissions SET allowed = 2;",
        )
        .expect("damage synthetic choice");
    close(connection);
    assert_eq!(
        store.permission(
            "https://corrupt.example.test",
            crate::PermissionKind::Camera
        ),
        Err(StoreError::InvalidEncoding(EncodingField::Permission))
    );
    store.shutdown().expect("close corrupt synthetic store");
    directory.cleanup();
}

#[test]
fn forget_deletes_only_the_matching_row_on_a_plain_connection() {
    let directory = TestDirectory::new();
    let connection = directory.connection();
    connection
        .execute_batch(crate::permissions::TABLE)
        .expect("create synthetic permission table");
    let save = |origin: &str, kind: PermissionKind| {
        crate::permissions::save(&connection, origin, kind, true).expect("save synthetic choice");
    };
    save("https://row.example.test", PermissionKind::Camera);
    save("https://row.example.test", PermissionKind::Fonts);
    save("https://other.example.test", PermissionKind::Camera);
    let forget =
        |origin: &str, kind: PermissionKind| crate::permissions::forget(&connection, origin, kind);
    assert_eq!(
        forget("https://row.example.test", PermissionKind::Camera),
        Ok(true)
    );
    assert_eq!(
        forget("https://row.example.test", PermissionKind::Camera),
        Ok(false)
    );
    assert_eq!(
        forget("https://row.example.test?x", PermissionKind::Camera),
        Err(StoreError::InvalidEncoding(EncodingField::Permission))
    );
    assert_eq!(crate::permissions::count(&connection), Ok(2));
    assert_eq!(
        crate::permissions::load(
            &connection,
            "https://row.example.test",
            PermissionKind::Camera
        ),
        Ok(None)
    );
    close(connection);
    directory.cleanup();
}

/// Saves one synthetic choice through the serialized writer.
fn remember(store: &Store, origin: &str, kind: PermissionKind, allowed: bool) {
    store
        .remember_permission(origin.into(), kind, allowed)
        .expect("remember synthetic choice");
}

#[test]
fn forgetting_one_choice_asks_again_and_keeps_other_kinds_and_origins() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    remember(
        &store,
        "https://forget.example.test",
        PermissionKind::Camera,
        true,
    );
    remember(
        &store,
        "https://forget.example.test",
        PermissionKind::Microphone,
        false,
    );
    remember(
        &store,
        "https://keep.example.test",
        PermissionKind::Camera,
        false,
    );
    assert_eq!(
        store.forget_permission("https://forget.example.test".into(), PermissionKind::Camera),
        Ok(true)
    );
    assert_eq!(
        store.permission("https://forget.example.test", PermissionKind::Camera),
        Ok(None),
        "a forgotten choice asks again"
    );
    assert_eq!(
        store.permission("https://forget.example.test", PermissionKind::Microphone),
        Ok(Some(false))
    );
    assert_eq!(
        store.permission("https://keep.example.test", PermissionKind::Camera),
        Ok(Some(false))
    );
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn forgetting_an_unsaved_choice_reports_false_and_invalid_origins_are_refused() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    remember(
        &store,
        "https://saved.example.test",
        PermissionKind::Location,
        true,
    );
    assert_eq!(
        store.forget_permission(
            "https://absent.example.test".into(),
            PermissionKind::Location
        ),
        Ok(false)
    );
    assert_eq!(
        store.forget_permission("http://saved.example.test".into(), PermissionKind::Location),
        Ok(false),
        "the scheme stays part of the origin"
    );
    assert_eq!(
        store.forget_permission(
            "https://saved.example.test/private".into(),
            PermissionKind::Location
        ),
        Err(StoreError::InvalidEncoding(EncodingField::Permission))
    );
    assert_eq!(
        store.permission("https://saved.example.test", PermissionKind::Location),
        Ok(Some(true))
    );
    assert_eq!(
        store.forget_permission(
            "https://saved.example.test".into(),
            PermissionKind::Location
        ),
        Ok(true)
    );
    assert_eq!(
        store.forget_permission(
            "https://saved.example.test".into(),
            PermissionKind::Location
        ),
        Ok(false),
        "a second forget finds nothing"
    );
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn forgotten_choice_stays_forgotten_after_restart() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    remember(
        &store,
        "https://restart.example.test",
        PermissionKind::Notifications,
        true,
    );
    remember(
        &store,
        "https://restart.example.test",
        PermissionKind::Clipboard,
        true,
    );
    assert_eq!(
        store.forget_permission(
            "https://restart.example.test".into(),
            PermissionKind::Notifications
        ),
        Ok(true)
    );
    store.shutdown().expect("close first store");
    let store = Store::open(directory.lease()).expect("reopen synthetic store");
    assert_eq!(
        store.permission(
            "https://restart.example.test",
            PermissionKind::Notifications
        ),
        Ok(None)
    );
    assert_eq!(
        store.permission("https://restart.example.test", PermissionKind::Clipboard),
        Ok(Some(true))
    );
    store.shutdown().expect("close reopened store");
    directory.cleanup();
}

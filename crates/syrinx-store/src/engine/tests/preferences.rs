//! Proves the settings/tile migration and round trips on disposable synthetic databases.

use std::fs;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};
use syrinx_core::{
    BuiltInPlugin, Destination, DestinationSurface, DestinationTarget, QuickDestinations,
};

use super::{TestDirectory, close, fixture};
use crate::{EncodingField, Store, StoreError, StoreOperation, schema};

fn web(label: &str, number: u32) -> Destination {
    Destination::new(
        label,
        DestinationTarget::Web(format!("https://tile-{number}.example.test/")),
    )
    .expect("synthetic tile is valid")
}

fn plugin(label: &str, plugin: BuiltInPlugin) -> Destination {
    Destination::new(label, DestinationTarget::Plugin(plugin)).expect("synthetic plugin tile")
}

fn settings(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn user_version(connection: &Connection) -> i64 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read synthetic schema version")
}

fn table_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .expect("inspect synthetic schema")
        == 1
}

/// Creates the version 2 (folders) schema, as P1-21 left it, with one saved revision.
fn folders_database(directory: &TestDirectory) -> Connection {
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let initial = schema::initial_schema();
    let folders = schema::folders_migration();
    let migrations = Migrations::new(vec![M::up(&initial), M::up(&folders)]);
    schema::apply_migrations(&mut connection, &migrations).expect("create the folders schema");
    connection
        .execute_batch(
            "INSERT INTO saved_pages VALUES
                 (x'0000000000000001', 0, 'Synthetic preference page', 'https://page-1.example.test/', NULL, NULL);
             INSERT INTO saved_layout VALUES (0, x'0000000000000001', NULL, NULL);
             UPDATE session_state SET revision = x'0000000000000003', last_saved_page_id = x'0000000000000001';",
        )
        .expect("seed synthetic folder rows");
    connection
}

#[test]
fn new_store_starts_with_default_preferences_and_roundtrips_changes() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("new synthetic store opens");
    let empty = store.load_preferences().expect("empty preferences load");
    assert!(empty.settings.is_empty());
    assert_eq!(empty.destinations, QuickDestinations::default());

    let quick = vec![
        web("Alpha", 1),
        plugin("Synthetic forge", BuiltInPlugin::Forge),
    ];
    let favorites = vec![web("Bravo", 2)];
    store
        .save_settings(settings(&[
            ("appearance.palette", "tint"),
            ("appearance.accent", "#6EA8FF"),
        ]))
        .expect("settings save");
    store
        .save_settings(settings(&[("appearance.palette", "paper")]))
        .expect("a later value replaces the earlier one");
    store
        .save_destinations(DestinationSurface::QuickAccess, quick.clone())
        .expect("quick access saves");
    store
        .save_destinations(DestinationSurface::Favorites, favorites.clone())
        .expect("favorites save");
    store.shutdown().expect("store closes");

    let reopened = Store::open(directory.lease()).expect("store reopens");
    let loaded = reopened.load_preferences().expect("preferences reload");
    assert_eq!(
        loaded
            .settings
            .get("appearance.palette")
            .map(String::as_str),
        Some("paper")
    );
    assert_eq!(
        loaded.settings.get("appearance.accent").map(String::as_str),
        Some("#6EA8FF")
    );
    assert_eq!(
        loaded.destinations,
        QuickDestinations::from_lists(quick, favorites).expect("expected lists")
    );

    // Replacing a surface with a shorter list leaves no stale positions behind.
    reopened
        .save_destinations(DestinationSurface::QuickAccess, vec![web("Only", 3)])
        .expect("shorter list saves");
    let shorter = reopened.load_preferences().expect("shorter list loads");
    assert_eq!(
        shorter.destinations.list(DestinationSurface::QuickAccess),
        [web("Only", 3)]
    );
    reopened.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn preference_writes_leave_the_tab_snapshot_and_revision_unchanged() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store opens");
    let snapshot = fixture();
    assert_eq!(store.save(0, snapshot.clone()), Ok(1));
    store
        .save_destinations(DestinationSurface::Favorites, vec![web("Alpha", 1)])
        .expect("favorites save");
    let stored = store.load().expect("tab state loads");
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.snapshot, snapshot);
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn upgrade_from_folders_schema_keeps_records_and_adds_empty_preferences() {
    let directory = TestDirectory::new();
    let connection = folders_database(&directory);
    assert_eq!(user_version(&connection), 2);
    close(connection);

    let store = Store::open(directory.lease()).expect("folders database upgrades");
    let stored = store.load().expect("upgraded state loads");
    assert_eq!(stored.revision, 3);
    assert_eq!(stored.snapshot.saved_pages.len(), 1);
    let preferences = store.load_preferences().expect("preferences load");
    assert!(preferences.settings.is_empty());
    assert_eq!(preferences.destinations, QuickDestinations::default());
    store.shutdown().expect("store closes");

    let connection = directory.connection();
    assert_eq!(user_version(&connection), schema::SCHEMA_VERSION);
    assert!(table_exists(&connection, "settings"));
    assert!(table_exists(&connection, "quick_destinations"));
    close(connection);
    directory.cleanup();
}

#[test]
fn failed_preferences_migration_rolls_back_to_the_intact_folders_schema() {
    // A stray table with the new table's name makes the version 3 script fail midway.
    let directory = TestDirectory::new();
    let mut connection = folders_database(&directory);
    connection
        .execute_batch("CREATE TABLE quick_destinations(value TEXT);")
        .expect("create a conflicting synthetic table");

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
    assert!(
        !table_exists(&connection, "settings"),
        "settings rolled back"
    );
    let pages: i64 = connection
        .query_row("SELECT count(*) FROM saved_pages", [], |row| row.get(0))
        .expect("count synthetic pages");
    assert_eq!(pages, 1);
    close(connection);
    directory.cleanup();
}

#[test]
fn damaged_tile_rows_are_refused_without_repair() {
    for damage in [
        // A position gap.
        "INSERT INTO quick_destinations VALUES (0, 1, 'Gap', 0, 'https://gap.example.test/')",
        // An unknown plugin.
        "INSERT INTO quick_destinations VALUES (1, 0, 'Unknown', 1, 'synthetic-plugin')",
        // A label the model would have trimmed.
        "INSERT INTO quick_destinations VALUES (0, 0, ' Padded', 0, 'https://padded.example.test/')",
        // A target that is not a web address.
        "INSERT INTO quick_destinations VALUES (0, 0, 'Script', 0, 'javascript:void(0)')",
    ] {
        let directory = TestDirectory::new();
        let store = Store::open(directory.lease()).expect("new store opens");
        store.shutdown().expect("store closes");
        let connection = directory.connection();
        connection
            .execute_batch(damage)
            .expect("damage synthetic tiles");
        close(connection);
        let before = fs::read(directory.database_path()).expect("read synthetic bytes");

        assert_eq!(
            Store::open(directory.lease()).expect_err("damaged tiles are refused"),
            StoreError::InvalidEncoding(EncodingField::Destination)
        );
        assert_eq!(
            fs::read(directory.database_path()).expect("reread synthetic bytes"),
            before
        );
        directory.cleanup();
    }
}

#[test]
fn invalid_preference_writes_change_nothing() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("new store opens");
    store
        .save_settings(settings(&[("appearance.palette", "glass")]))
        .expect("valid setting saves");
    let long_value = "x".repeat(crate::MAX_SETTING_VALUE_BYTES + 1);
    for entries in [
        settings(&[("appearance.palette", "haze"), ("", "empty key")]),
        settings(&[("appearance.accent", &long_value)]),
    ] {
        assert_eq!(
            store.save_settings(entries),
            Err(StoreError::InvalidEncoding(EncodingField::Setting))
        );
    }
    let too_many = (0..=syrinx_core::MAX_DESTINATIONS as u32)
        .map(|number| web("Tile", number))
        .collect();
    assert_eq!(
        store.save_destinations(DestinationSurface::QuickAccess, too_many),
        Err(StoreError::InvalidEncoding(EncodingField::Destination))
    );
    let loaded = store.load_preferences().expect("preferences load");
    assert_eq!(
        loaded
            .settings
            .get("appearance.palette")
            .map(String::as_str),
        Some("glass")
    );
    assert!(
        loaded
            .destinations
            .list(DestinationSurface::QuickAccess)
            .is_empty()
    );
    store.shutdown().expect("store closes");
    directory.cleanup();
}

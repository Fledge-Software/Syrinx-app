//! Proves the saved-folder migration and tree persistence on disposable synthetic databases.

use std::fs;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};
use syrinx_core::{
    Activation, FolderRemoval, SavedEntryId, SavedFolderId, SavedPageId, SavedPageSnapshot,
    SavedPlacement, SnapshotError, TabId, TabKind, TabMetadata, TabModel, TabModelSnapshot,
    TabSnapshot,
};

use super::{TestDirectory, close};
use crate::{EncodingField, Store, StoreError, StoreOperation, schema, validate_database};

fn synthetic(number: u64) -> TabMetadata {
    TabMetadata {
        title: format!("Synthetic folder test {number}"),
        address: format!("https://folder-{number}.example.test/"),
        favicon: None,
        badge: None,
    }
}

fn tab_id(value: u64) -> TabId {
    TabId::new(value).expect("synthetic tab ID is nonzero")
}

fn page_id(value: u64) -> SavedPageId {
    SavedPageId::new(value).expect("synthetic saved ID is nonzero")
}

/// Creates the version 1 (flat) schema without enabling WAL, as P1-03 left it.
fn flat_database(directory: &TestDirectory) -> Connection {
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let initial = schema::initial_schema();
    let migrations = Migrations::new(vec![M::up(&initial).foreign_key_check()]);
    schema::apply_migrations(&mut connection, &migrations).expect("create the flat schema");
    connection
}

/// Three flat saved rows, the middle one open, plus one ordinary tab, at revision 4.
fn seed_flat_rows(connection: &Connection) {
    connection
        .execute_batch(
            "INSERT INTO saved_pages VALUES
                 (x'0000000000000001', 0, 'Synthetic folder test 1', 'https://folder-1.example.test/', NULL, NULL),
                 (x'0000000000000002', 1, 'Synthetic folder test 2', 'https://folder-2.example.test/', NULL, NULL),
                 (x'0000000000000003', 2, 'Synthetic folder test 3', 'https://folder-3.example.test/', NULL, NULL);
             INSERT INTO live_tabs VALUES
                 (x'0000000000000001', 0, 0, 'Synthetic folder test 12', 'https://folder-12.example.test/', NULL, NULL, x'0000000000000002'),
                 (x'0000000000000002', 1, 0, 'Synthetic folder test 20', 'https://folder-20.example.test/', NULL, NULL, NULL);
             UPDATE session_state SET revision = x'0000000000000004', active_tab_id = x'0000000000000002',
                 last_tab_id = x'0000000000000002', last_saved_page_id = x'0000000000000003';",
        )
        .expect("seed synthetic flat rows");
}

fn expected_flat_snapshot() -> TabModelSnapshot {
    let saved_pages: Vec<SavedPageSnapshot> = (1..=3)
        .map(|number| SavedPageSnapshot {
            id: page_id(number),
            metadata: synthetic(number),
        })
        .collect();
    TabModelSnapshot {
        tabs: vec![
            TabSnapshot {
                id: tab_id(1),
                kind: TabKind::Web,
                metadata: synthetic(12),
                saved_page_id: Some(page_id(2)),
            },
            TabSnapshot {
                id: tab_id(2),
                kind: TabKind::Web,
                metadata: synthetic(20),
                saved_page_id: None,
            },
        ],
        saved_layout: saved_pages
            .iter()
            .map(|page| SavedPlacement {
                entry: SavedEntryId::Page(page.id),
                parent: None,
            })
            .collect(),
        saved_pages,
        saved_folders: Vec::new(),
        active_id: Some(tab_id(2)),
        last_tab_id: 2,
        last_saved_page_id: 3,
        last_saved_folder_id: 0,
    }
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

/// Nested folders, an open page inside a folder, unloaded pages and an ordinary tab.
fn tree_model() -> (TabModel, SavedFolderId, SavedFolderId) {
    let mut model = TabModel::new();
    let work = model
        .create_folder("Synthetic work".to_owned(), None)
        .expect("top folder");
    let nested = model
        .create_folder("Synthetic nested".to_owned(), Some(work))
        .expect("nested folder");
    let open = model
        .open(TabKind::Web, synthetic(1), Activation::Foreground)
        .expect("tab opens");
    let saved = model.save_page(open).expect("page saves");
    model
        .move_saved_entry(SavedEntryId::Page(saved), Some(nested), 0)
        .expect("page moves into the nested folder");
    model
        .add_saved_page(synthetic(2), Some(work))
        .expect("unloaded page in folder");
    model
        .add_saved_page(synthetic(3), None)
        .expect("unloaded top-level page");
    model
        .open(TabKind::Web, synthetic(4), Activation::Background)
        .expect("ordinary tab");
    (model, work, nested)
}

#[test]
fn upgrade_from_flat_schema_keeps_rows_order_revision_and_associations() {
    let directory = TestDirectory::new();
    let connection = flat_database(&directory);
    seed_flat_rows(&connection);
    close(connection);

    // The writer migrates the version 1 file in place, preserving every record.
    let store = Store::open(directory.lease()).expect("flat database upgrades");
    let stored = store.load().expect("upgraded state loads");
    assert_eq!(stored.revision, 4);
    assert_eq!(stored.snapshot, expected_flat_snapshot());
    store.shutdown().expect("store closes");
    let connection = directory.connection();
    assert_eq!(user_version(&connection), schema::SCHEMA_VERSION);
    close(connection);

    // The upgraded tree accepts folders and survives a restart.
    let mut model = TabModel::from_snapshot(stored.snapshot).expect("core accepts upgrade");
    let folder = model
        .create_folder("Synthetic upgraded".to_owned(), None)
        .expect("folder created");
    model
        .move_saved_entry(SavedEntryId::Page(page_id(2)), Some(folder), 0)
        .expect("open saved page moves into the folder");
    let store = Store::open(directory.lease()).expect("current database reopens");
    assert_eq!(store.save(4, model.snapshot()), Ok(5));
    store.shutdown().expect("store closes");
    let store = Store::open(directory.lease()).expect("current database reopens again");
    assert_eq!(store.load().expect("tree loads").snapshot, model.snapshot());
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn damaged_flat_rows_are_refused_before_migrating_or_changing_the_file() {
    let directory = TestDirectory::new();
    let connection = flat_database(&directory);
    seed_flat_rows(&connection);
    connection
        .execute(
            "UPDATE saved_pages SET saved_order = 9 WHERE saved_order = 2",
            [],
        )
        .expect("damage synthetic saved order");
    close(connection);
    let before = fs::read(directory.database_path()).expect("read synthetic bytes");

    assert_eq!(
        Store::open(directory.lease()).expect_err("damaged flat rows are refused"),
        StoreError::InvalidEncoding(EncodingField::SavedOrder)
    );
    assert_eq!(
        fs::read(directory.database_path()).expect("reread synthetic bytes"),
        before
    );
    directory.cleanup();
}

#[test]
fn failed_folder_migration_rolls_back_to_the_intact_flat_schema() {
    // A saved-order gap fails the migration hook; a dangling association fails its foreign key check.
    for damage in [
        "UPDATE saved_pages SET saved_order = 9 WHERE saved_order = 2",
        "PRAGMA foreign_keys = OFF; DELETE FROM saved_pages WHERE saved_order = 1; PRAGMA foreign_keys = ON;",
    ] {
        let directory = TestDirectory::new();
        let mut connection = flat_database(&directory);
        seed_flat_rows(&connection);
        connection
            .execute_batch(damage)
            .expect("damage synthetic flat rows");
        let page_count = |connection: &Connection| -> i64 {
            connection
                .query_row("SELECT count(*) FROM saved_pages", [], |row| row.get(0))
                .expect("count synthetic pages")
        };
        let pages_before = page_count(&connection);

        let scripts = schema::MigrationScripts::new();
        assert!(matches!(
            schema::apply_migrations(&mut connection, &scripts.migrations()),
            Err(StoreError::Database {
                operation: StoreOperation::Migrate,
                ..
            })
        ));
        assert!(connection.is_autocommit());
        assert_eq!(user_version(&connection), 1);
        for table in ["saved_folders", "saved_layout", "saved_folder_state"] {
            assert!(!table_exists(&connection, table), "{table} was rolled back");
        }
        assert_eq!(page_count(&connection), pages_before);
        close(connection);
        directory.cleanup();
    }
}

#[test]
fn folder_tree_roundtrips_across_restarts_and_removals() {
    let directory = TestDirectory::new();
    let (mut model, work, nested) = tree_model();
    let store = Store::open(directory.lease()).expect("new store opens");
    assert_eq!(store.save(0, model.snapshot()), Ok(1));
    store.shutdown().expect("store closes");

    // Restart, restore the core model and keep editing the tree.
    let store = Store::open(directory.lease()).expect("store reopens");
    let stored = store.load().expect("tree loads");
    assert_eq!(stored.snapshot, model.snapshot());
    let restored = TabModel::from_snapshot(stored.snapshot).expect("core accepts tree");
    assert_eq!(restored.saved_layout(), model.saved_layout());
    model
        .remove_folder(nested, FolderRemoval::MoveContentsUp)
        .expect("nested folder removed");
    model
        .rename_folder(work, "Synthetic renamed".to_owned())
        .expect("folder renamed");
    assert_eq!(store.save(1, model.snapshot()), Ok(2));
    store.shutdown().expect("store closes");

    let store = Store::open(directory.lease()).expect("store reopens");
    assert_eq!(
        store.load().expect("edited tree loads").snapshot,
        model.snapshot()
    );
    model
        .remove_folder(work, FolderRemoval::RemoveContents)
        .expect("folder and contents removed");
    assert_eq!(store.save(2, model.snapshot()), Ok(3));
    store.shutdown().expect("store closes");

    // Removed folders leave no rows behind: only the top-level page remains saved.
    let connection = directory.connection();
    for (query, expected) in [
        ("SELECT count(*) FROM saved_folders", 0),
        ("SELECT count(*) FROM saved_layout", 1),
        ("SELECT count(*) FROM saved_pages", 1),
    ] {
        let count: i64 = connection
            .query_row(query, [], |row| row.get(0))
            .expect("count synthetic rows");
        assert_eq!(count, expected, "{query}");
    }
    close(connection);
    let store = Store::open(directory.lease()).expect("store reopens");
    assert_eq!(
        store.load().expect("final tree loads").snapshot,
        model.snapshot()
    );
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn folder_watermark_regression_is_refused() {
    let directory = TestDirectory::new();
    let (model, _, _) = tree_model();
    let store = Store::open(directory.lease()).expect("store opens");
    let snapshot = model.snapshot();
    assert_eq!(store.save(0, snapshot.clone()), Ok(1));
    let mut regressed = TabModelSnapshot {
        saved_folders: Vec::new(),
        last_saved_folder_id: 0,
        ..snapshot.clone()
    };
    regressed
        .saved_layout
        .retain(|placement| matches!(placement.entry, SavedEntryId::Page(_)));
    for placement in &mut regressed.saved_layout {
        placement.parent = None;
    }
    regressed
        .validate()
        .expect("regressed snapshot is otherwise valid");
    assert_eq!(
        store.save(1, regressed),
        Err(StoreError::WatermarkRegression)
    );
    assert_eq!(store.load().expect("state is unchanged").snapshot, snapshot);
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn damaged_tree_rows_are_refused_without_repair() {
    let (model, _, _) = tree_model();
    let snapshot = model.snapshot();
    let nested_page = snapshot.saved_layout[2].entry;
    for (sql, expected) in [
        (
            "UPDATE saved_layout SET page_id = NULL WHERE page_id IS NOT NULL AND entry_order = 2",
            StoreError::InvalidEncoding(EncodingField::SavedLayout),
        ),
        (
            "UPDATE saved_layout SET entry_order = 40 WHERE entry_order = 4",
            StoreError::InvalidEncoding(EncodingField::SavedLayout),
        ),
        (
            "UPDATE saved_layout SET parent_folder_id = zeroblob(8) WHERE entry_order = 2",
            StoreError::InvalidEncoding(EncodingField::SavedFolderId),
        ),
        (
            "DELETE FROM saved_folder_state",
            StoreError::InvalidEncoding(EncodingField::SavedFolderWatermark),
        ),
        (
            "UPDATE saved_layout SET parent_folder_id = NULL WHERE entry_order = 1; UPDATE saved_layout SET parent_folder_id = x'0000000000000002' WHERE entry_order = 0",
            StoreError::InvalidSnapshot(SnapshotError::InvalidSavedParent(SavedEntryId::Folder(
                SavedFolderId::new(1).expect("synthetic folder ID"),
            ))),
        ),
        (
            "DELETE FROM saved_layout WHERE entry_order = 4",
            StoreError::InvalidSnapshot(SnapshotError::SavedLayoutIncomplete),
        ),
        (
            "UPDATE saved_layout SET entry_order = entry_order + 10 WHERE entry_order IN (1, 2); UPDATE saved_layout SET entry_order = 1 WHERE entry_order = 12; UPDATE saved_layout SET entry_order = 2 WHERE entry_order = 11",
            StoreError::InvalidSnapshot(SnapshotError::InvalidSavedParent(nested_page)),
        ),
    ] {
        let directory = TestDirectory::new();
        let store = Store::open(directory.lease()).expect("store opens");
        store.save(0, snapshot.clone()).expect("tree saves");
        store.shutdown().expect("store closes");
        let connection = directory.connection();
        connection
            .execute_batch("PRAGMA foreign_keys = OFF; PRAGMA ignore_check_constraints = ON;")
            .expect("allow deliberate synthetic damage");
        connection
            .execute_batch(sql)
            .expect("damage synthetic tree");
        close(connection);
        assert_eq!(
            Store::open(directory.lease()).expect_err("damaged tree is refused"),
            expected,
            "{sql}"
        );
        directory.cleanup();
    }
}

#[test]
fn flat_backups_still_validate_as_a_top_level_tree() {
    let directory = TestDirectory::new();
    let connection = flat_database(&directory);
    seed_flat_rows(&connection);
    close(connection);
    let before = fs::read(directory.database_path()).expect("read synthetic bytes");

    // Read-only validation accepts a version 1 copy without migrating it.
    let lease = directory.lease();
    let validated = validate_database(lease.as_ref()).expect("flat database validates");
    assert_eq!(validated.revision, 4);
    assert_eq!(validated.snapshot, expected_flat_snapshot());
    assert_eq!(
        fs::read(directory.database_path()).expect("reread synthetic bytes"),
        before
    );
    directory.cleanup();
}

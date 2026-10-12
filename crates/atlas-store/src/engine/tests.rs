//! Exercises persistence failures and concurrency using only disposable synthetic databases.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use atlas_core::{
    Activation, SavedEntryId, SavedPageId, SavedPageSnapshot, SavedPlacement, TabId, TabKind,
    TabMetadata, TabModel, TabModelSnapshot, TabSnapshot,
};
use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use super::{SaveRequest, Store, WriterHandle, WriterRequest};
use crate::{DirectoryLease, EncodingField, LocationError, StoreError, StoreOperation, schema};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        // Exclusive creation establishes ownership of every file this helper later removes.
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock follows the epoch")
            .as_nanos();
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "atlas-engine-test-{}-{stamp}-{sequence}",
            std::process::id()
        ));
        assert!(
            fs::create_dir(&path).is_ok(),
            "create disposable test directory"
        );
        Self { path }
    }

    fn database_path(&self) -> PathBuf {
        self.path.join("atlas.db")
    }

    fn lease(&self) -> Arc<SyntheticLease> {
        Arc::new(SyntheticLease {
            path: self.database_path(),
            valid: AtomicBool::new(true),
            checks: AtomicU64::new(0),
        })
    }

    fn connection(&self) -> Connection {
        Connection::open(self.database_path()).unwrap_or_else(|_| panic!("open synthetic database"))
    }

    fn cleanup(self) {
        // Only fixed files inside this newly created directory are eligible for cleanup.
        for name in [
            "atlas.db",
            "atlas.db-wal",
            "atlas.db-shm",
            "atlas.db-journal",
        ] {
            match fs::remove_file(self.path.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => panic!("remove a known synthetic database file"),
            }
        }
        assert!(
            fs::remove_dir(self.path).is_ok(),
            "remove empty synthetic directory"
        );
    }
}

struct SyntheticLease {
    path: PathBuf,
    valid: AtomicBool,
    checks: AtomicU64,
}

impl DirectoryLease for SyntheticLease {
    fn database_path(&self) -> &Path {
        &self.path
    }

    fn revalidate(&self) -> Result<(), LocationError> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        if self.valid.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(LocationError::IdentityChanged)
        }
    }
}

fn close(connection: Connection) {
    assert!(connection.close().is_ok(), "close synthetic connection");
}

fn metadata(number: u64) -> TabMetadata {
    TabMetadata {
        title: format!("Synthetic snapshot {number} — café\0end"),
        address: format!("https://snapshot-{number}.test/"),
        favicon: Some(format!("https://snapshot-{number}.test/icon.svg")),
        badge: Some(format!("synthetic-badge-{number}")),
    }
}

fn one_tab(number: u64) -> TabModelSnapshot {
    let mut model = TabModel::new();
    model
        .open(TabKind::Web, metadata(number), Activation::Foreground)
        .expect("synthetic tab opens");
    model.snapshot()
}

fn fixture() -> TabModelSnapshot {
    let mut model = TabModel::new();
    let first = model
        .open(TabKind::Web, metadata(1), Activation::Foreground)
        .expect("first tab opens");
    let unloaded = model
        .open(TabKind::Web, metadata(2), Activation::Foreground)
        .expect("second tab opens");
    model.save_page(first).expect("first page saves");
    model.save_page(unloaded).expect("second page saves");
    model.close(unloaded).expect("second page unloads");
    model
        .update_metadata(first, metadata(10))
        .expect("live metadata changes");
    for kind in [
        TabKind::PluginView,
        TabKind::Note,
        TabKind::File,
        TabKind::Diff,
        TabKind::Terminal,
    ] {
        model
            .open(kind, metadata(3), Activation::Background)
            .expect("generic tab opens");
    }
    model.snapshot()
}

#[test]
fn roundtrip_preserves_unloaded_rows_kinds_order_selection_and_metadata() {
    let directory = TestDirectory::new();
    let snapshot = fixture();
    let store = Store::open(directory.lease()).expect("new synthetic store opens");
    assert_eq!(store.load().expect("empty state loads").revision, 0);
    assert_eq!(store.save(0, snapshot.clone()), Ok(1));
    store.shutdown().expect("store closes");

    let reopened = Store::open(directory.lease()).expect("synthetic store reopens");
    let stored = reopened.load().expect("persisted state loads");
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.snapshot, snapshot);
    let restored = TabModel::from_snapshot(stored.snapshot).expect("core accepts saved state");
    assert_eq!(restored.closed_count(), 0);
    assert_eq!(restored.saved_pages().len(), 2);
    reopened.shutdown().expect("reopened store closes");
    directory.cleanup();
}

#[test]
fn stale_revisions_and_counter_regression_leave_the_database_unchanged() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store opens");
    let snapshot = fixture();
    assert_eq!(store.save(0, snapshot.clone()), Ok(1));
    assert_eq!(
        store.save(0, snapshot.clone()),
        Err(StoreError::StaleRevision {
            expected: 0,
            actual: 1
        })
    );
    assert_eq!(
        store.save(1, TabModelSnapshot::default()),
        Err(StoreError::WatermarkRegression)
    );
    assert_eq!(
        store.load().expect("previous state loads").snapshot,
        snapshot
    );
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn failed_insert_rolls_back_rows_selection_and_revision_without_leaking_sqlite_text() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store opens");
    let previous = fixture();
    store
        .save(0, previous.clone())
        .expect("initial state saves");
    let connection = directory.connection();
    connection.execute_batch("CREATE TRIGGER synthetic_failure BEFORE INSERT ON live_tabs BEGIN SELECT RAISE(ABORT, 'synthetic-sensitive-diagnostic'); END;").expect("install synthetic failure trigger");
    let failure = store
        .save(1, previous.clone())
        .expect_err("injected insert fails");
    assert!(matches!(
        failure,
        StoreError::Database {
            operation: StoreOperation::Write,
            ..
        }
    ));
    assert!(!format!("{failure} {failure:?}").contains("synthetic-sensitive-diagnostic"));
    let stored = store.load().expect("rolled back state loads");
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.snapshot, previous);
    connection
        .execute_batch("DROP TRIGGER synthetic_failure")
        .expect("remove synthetic failure trigger");
    assert_eq!(store.save(1, previous), Ok(2));
    close(connection);
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn failed_migration_rolls_back_application_marker_schema_and_version() {
    let directory = TestDirectory::new();
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let initial = schema::initial_schema();
    let migrations = Migrations::new(vec![
        M::up(&initial).foreign_key_check(),
        M::up(
            "CREATE TABLE synthetic_partial(value TEXT) STRICT; INSERT INTO missing_table VALUES (1);",
        ),
    ]);
    assert!(matches!(
        schema::apply_migrations(&mut connection, &migrations),
        Err(StoreError::Database {
            operation: StoreOperation::Migrate,
            ..
        })
    ));
    assert!(connection.is_autocommit());
    for query in [
        "PRAGMA application_id",
        "PRAGMA user_version",
        "SELECT count(*) FROM sqlite_schema",
    ] {
        let value: i64 = connection
            .query_row(query, [], |row| row.get(0))
            .expect("inspect rolled back schema");
        assert_eq!(value, 0);
    }
    close(connection);
    let store = Store::open(directory.lease()).expect("rolled back database initializes cleanly");
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn foreign_future_and_altered_schemas_are_refused_without_rewriting_the_file() {
    for (sql, expected) in [
        (
            "CREATE TABLE foreign_data(value TEXT);".to_owned(),
            StoreError::ForeignDatabase,
        ),
        (
            "CREATE TABLE sqliteZforeign(value TEXT);".to_owned(),
            StoreError::ForeignDatabase,
        ),
        (
            format!(
                "PRAGMA application_id = {}; PRAGMA user_version = {}; CREATE TABLE future_data(value TEXT);",
                schema::APPLICATION_ID,
                schema::SCHEMA_VERSION + 1
            ),
            StoreError::FutureSchema {
                version: schema::SCHEMA_VERSION + 1,
            },
        ),
    ] {
        let directory = TestDirectory::new();
        let connection = directory.connection();
        connection
            .execute_batch(&sql)
            .expect("create synthetic noncurrent database");
        close(connection);
        let bytes = fs::read(directory.database_path()).expect("read synthetic bytes");
        assert_eq!(
            Store::open(directory.lease()).expect_err("noncurrent schema is rejected"),
            expected
        );
        assert_eq!(
            fs::read(directory.database_path()).expect("reread synthetic bytes"),
            bytes
        );
        directory.cleanup();
    }

    let directory = TestDirectory::new();
    Store::open(directory.lease())
        .expect("synthetic store opens")
        .shutdown()
        .expect("store closes");
    let connection = directory.connection();
    connection
        .execute_batch("ALTER TABLE live_tabs ADD COLUMN unexpected TEXT")
        .expect("alter synthetic schema");
    close(connection);
    let bytes = fs::read(directory.database_path()).expect("read altered bytes");
    assert_eq!(
        Store::open(directory.lease()).expect_err("altered schema is rejected"),
        StoreError::SchemaMismatch
    );
    assert_eq!(
        fs::read(directory.database_path()).expect("reread altered bytes"),
        bytes
    );
    directory.cleanup();
}

#[test]
fn full_u64_identities_and_exhausted_revision_roundtrip_exactly() {
    let directory = TestDirectory::new();
    let tab_id = TabId::new(u64::MAX).expect("nonzero synthetic tab ID");
    let saved_id = SavedPageId::new(u64::MAX).expect("nonzero synthetic saved ID");
    let snapshot = TabModelSnapshot {
        tabs: vec![TabSnapshot {
            id: tab_id,
            kind: TabKind::Web,
            metadata: metadata(1),
            saved_page_id: Some(saved_id),
        }],
        saved_pages: vec![SavedPageSnapshot {
            id: saved_id,
            metadata: metadata(2),
        }],
        saved_folders: Vec::new(),
        saved_layout: vec![SavedPlacement {
            entry: SavedEntryId::Page(saved_id),
            parent: None,
        }],
        active_id: Some(tab_id),
        last_tab_id: u64::MAX,
        last_saved_page_id: u64::MAX,
        last_saved_folder_id: u64::MAX,
    };
    let store = Store::open(directory.lease()).expect("synthetic store opens");
    store
        .save(0, snapshot.clone())
        .expect("full-range IDs save");
    store.shutdown().expect("store closes");
    let connection = directory.connection();
    connection
        .execute(
            "UPDATE session_state SET revision = ?1",
            [u64::MAX.to_be_bytes()],
        )
        .expect("set synthetic exhausted revision");
    close(connection);
    let reopened = Store::open(directory.lease()).expect("exhausted state opens");
    let loaded = reopened.load().expect("full-range values load");
    assert_eq!(loaded.revision, u64::MAX);
    assert_eq!(loaded.snapshot, snapshot);
    assert_eq!(
        reopened.save(u64::MAX, snapshot),
        Err(StoreError::RevisionExhausted)
    );
    assert_eq!(reopened.load().expect("failed save preserves data"), loaded);
    reopened.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn malformed_encodings_and_missing_state_are_rejected_without_repair() {
    for (sql, field) in [
        (
            "UPDATE session_state SET revision = x'01'",
            EncodingField::Revision,
        ),
        (
            "UPDATE session_state SET active_tab_id = zeroblob(8)",
            EncodingField::ActiveTabId,
        ),
        (
            "UPDATE live_tabs SET id = zeroblob(8)",
            EncodingField::TabId,
        ),
        ("UPDATE live_tabs SET kind = 99", EncodingField::TabKind),
        (
            "UPDATE live_tabs SET live_order = 7",
            EncodingField::LiveOrder,
        ),
        (
            "UPDATE live_tabs SET title = CAST(x'80' AS TEXT)",
            EncodingField::Metadata,
        ),
        ("DELETE FROM session_state", EncodingField::SessionState),
    ] {
        let directory = TestDirectory::new();
        let store = Store::open(directory.lease()).expect("synthetic store opens");
        store.save(0, one_tab(1)).expect("synthetic state saves");
        store.shutdown().expect("store closes");
        let connection = directory.connection();
        connection
            .execute_batch("PRAGMA foreign_keys = OFF; PRAGMA ignore_check_constraints = ON;")
            .expect("allow deliberate synthetic damage");
        connection
            .execute_batch(sql)
            .expect("damage synthetic record encoding");
        close(connection);
        assert_eq!(
            Store::open(directory.lease()).expect_err("damaged state is rejected"),
            StoreError::InvalidEncoding(field)
        );
        directory.cleanup();
    }
}

#[test]
fn malformed_current_snapshot_is_refused_before_changing_journal_mode() {
    let directory = TestDirectory::new();
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let scripts = schema::MigrationScripts::new();
    schema::apply_migrations(&mut connection, &scripts.migrations())
        .expect("initialize current schema without enabling WAL");
    connection
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON; UPDATE session_state SET revision = x'01';",
        )
        .expect("damage synthetic revision encoding");
    close(connection);
    let before = fs::read(directory.database_path()).expect("read synthetic bytes");

    assert_eq!(
        Store::open(directory.lease()).expect_err("malformed current state is refused"),
        StoreError::InvalidEncoding(EncodingField::Revision),
    );
    assert_eq!(
        fs::read(directory.database_path()).expect("reread synthetic bytes"),
        before
    );
    let connection = directory.connection();
    let journal: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("inspect unchanged journal mode");
    assert_eq!(journal, "delete");
    close(connection);
    directory.cleanup();
}

#[test]
fn competing_writers_cannot_both_commit_the_same_expected_revision() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store opens");
    let barrier = Barrier::new(2);
    let results = thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            store.save(0, one_tab(1))
        });
        let second = scope.spawn(|| {
            barrier.wait();
            store.save(0, one_tab(2))
        });
        [
            first.join().expect("first caller returns"),
            second.join().expect("second caller returns"),
        ]
    });
    assert_eq!(results.iter().filter(|result| **result == Ok(1)).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| **result
                == Err(StoreError::StaleRevision {
                    expected: 0,
                    actual: 1
                }))
            .count(),
        1
    );
    assert_eq!(store.load().expect("winning revision loads").revision, 1);
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn concurrent_readers_observe_complete_revisions_during_writes() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store opens");
    let barrier = Barrier::new(5);
    thread::scope(|scope| {
        let mut readers = Vec::new();
        for _ in 0..4 {
            readers.push(scope.spawn(|| {
                barrier.wait();
                for _ in 0..100 {
                    let stored = store.load().expect("concurrent read succeeds");
                    let expected = if stored.revision == 0 {
                        TabModelSnapshot::default()
                    } else {
                        one_tab(stored.revision)
                    };
                    assert_eq!(stored.snapshot, expected);
                }
            }));
        }
        barrier.wait();
        for revision in 1..=24 {
            assert_eq!(store.save(revision - 1, one_tab(revision)), Ok(revision));
        }
        for reader in readers {
            reader.join().expect("reader completes");
        }
    });
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn lease_revalidation_checks_connection_lifecycle_without_repeating_for_snapshots() {
    let directory = TestDirectory::new();
    let lease = directory.lease();
    lease.valid.store(false, Ordering::SeqCst);
    assert_eq!(
        Store::open(lease.clone()).expect_err("invalid lease refuses opening"),
        StoreError::Location(LocationError::IdentityChanged),
    );
    lease.valid.store(true, Ordering::SeqCst);
    let before_open = lease.checks.load(Ordering::SeqCst);
    let store = Store::open(lease.clone()).expect("valid synthetic lease opens");
    let opened = lease.checks.load(Ordering::SeqCst);
    assert!(opened > before_open);
    assert_eq!(store.load().expect("empty store loads").revision, 0);
    assert_eq!(store.save(0, one_tab(1)), Ok(1));
    assert_eq!(store.load().expect("committed store loads").revision, 1);
    assert_eq!(lease.checks.load(Ordering::SeqCst), opened);
    store.shutdown().expect("store closes");
    assert!(lease.checks.load(Ordering::SeqCst) > opened);
    drop(lease);
    directory.cleanup();
}

#[test]
fn abandoned_write_reply_is_reported_at_shutdown_and_committed_revision_is_recoverable() {
    let directory = TestDirectory::new();
    let writer = WriterHandle::start(directory.lease(), crate::ShutdownState::Unclean)
        .expect("writer starts");
    let (reply, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    assert!(
        writer
            .sender
            .send(WriterRequest::Save(SaveRequest {
                expected_revision: 0,
                snapshot: one_tab(1),
                reply
            }))
            .is_ok()
    );
    assert_eq!(writer.stop(), Err(StoreError::ReplyAbandoned));
    let store = Store::open(directory.lease()).expect("acknowledgement loss permits reopening");
    assert_eq!(store.load().expect("committed revision loads").revision, 1);
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn store_and_snapshot_debug_redact_paths_and_all_metadata_fields() {
    let directory = TestDirectory::new();
    let snapshot = fixture();
    let store = Store::open(directory.lease()).expect("synthetic store opens");
    store
        .save(0, snapshot.clone())
        .expect("synthetic snapshot saves");
    let stored = store.load().expect("synthetic snapshot loads");
    let diagnostic = format!("{store:?} {stored:?}");
    assert!(
        !diagnostic.contains(
            directory
                .path
                .to_str()
                .expect("synthetic temp path is Unicode")
        )
    );
    for values in snapshot
        .tabs
        .iter()
        .map(|tab| &tab.metadata)
        .chain(snapshot.saved_pages.iter().map(|page| &page.metadata))
    {
        for text in [
            &values.title,
            &values.address,
            values.favicon.as_ref().expect("synthetic icon"),
            values.badge.as_ref().expect("synthetic badge"),
        ] {
            assert!(!diagnostic.contains(text));
        }
    }
    store.shutdown().expect("store closes");
    directory.cleanup();
}

mod folders;
mod import;
mod lifecycle;
mod preferences;

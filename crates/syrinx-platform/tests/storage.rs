//! Exercises the native directory lease and SQLite together on disposable synthetic data.

use std::{
    fs, io,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Component, Path, PathBuf, Prefix},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use syrinx_core::{Activation, TabId, TabKind, TabMetadata, TabModel};
use syrinx_platform::WindowsDirectoryLease;
use syrinx_store::{DirectoryLease, LocationError, ShutdownState, Store, StoreError};

const DATABASE_FILES: [&str; 4] = [
    "syrinx.db",
    "syrinx.db-wal",
    "syrinx.db-shm",
    "syrinx.db-journal",
];
const CONTROL_FILES: [&str; 2] = ["syrinx.lifecycle", "syrinx.recovery-intent"];
const SENTINEL: &[u8] = b"synthetic target bytes: retain until refusal is verified";
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct DisposableDirectory {
    path: PathBuf,
}

impl DisposableDirectory {
    fn new() -> Self {
        // Exclusive creation establishes ownership before any test writes or cleans up.
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock supports a synthetic directory name")
            .as_nanos();
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "syrinx-storage-test-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        checked_io(fs::create_dir(&path), "create disposable directory");
        Self { path }
    }

    fn cleanup(self) {
        // Remove only known test-owned files after store and lease handles have closed.
        for name in DATABASE_FILES.into_iter().chain(CONTROL_FILES) {
            remove_file_if_present(&self.path.join(name));
        }
        checked_io(fs::remove_dir(self.path), "remove disposable directory");
    }
}

fn checked_io<T>(result: io::Result<T>, operation: &str) -> T {
    // OS error messages can contain private paths; print categories and codes only.
    match result {
        Ok(value) => value,
        Err(error) => panic!(
            "{operation} failed: {:?}, native code {:?}",
            error.kind(),
            error.raw_os_error()
        ),
    }
}

fn remove_file_if_present(path: &Path) {
    // An absent SQLite sidecar is normal after a clean close.
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => checked_io(Err(error), "remove synthetic database file"),
    }
}

fn exists(path: &Path) -> bool {
    checked_io(path.try_exists(), "check synthetic entry")
}

fn metadata(number: u64) -> TabMetadata {
    TabMetadata {
        title: format!("Synthetic stored page {number} — café"),
        address: format!("https://storage-{number}.test/"),
        favicon: Some(format!("https://storage-{number}.test/icon.svg")),
        badge: Some(format!("synthetic-{number}")),
    }
}

fn open_tab(model: &mut TabModel, number: u64) -> TabId {
    model
        .open(TabKind::Web, metadata(number), Activation::Foreground)
        .expect("synthetic tab allocation succeeds")
}

fn open_store(directory: &Path) -> Store {
    // Explicit disposable paths use the same native checks as the production opener.
    let lease = WindowsDirectoryLease::prepare(directory)
        .expect("native guard accepts a disposable local directory");
    Store::open(Arc::new(lease)).expect("guarded synthetic store opens")
}

#[test]
fn saved_destinations_and_navigated_live_metadata_survive_native_store_reopen() {
    let directory = DisposableDirectory::new();
    let mut model = TabModel::new();
    let unloaded_tab = open_tab(&mut model, 1);
    let unloaded_saved = model.save_page(unloaded_tab).expect("synthetic page saves");
    model.close(unloaded_tab).expect("synthetic tab closes");
    let live_tab = open_tab(&mut model, 2);
    let live_saved = model.save_page(live_tab).expect("synthetic page saves");
    model
        .update_metadata(live_tab, metadata(20))
        .expect("synthetic live navigation succeeds");
    open_tab(&mut model, 3);
    model.select(live_tab).expect("synthetic live tab exists");
    let expected = model.snapshot();

    // Commit both metadata copies and the unloaded row through the real SQLite writer.
    let store = open_store(&directory.path);
    let initial = store.load().expect("new synthetic store loads");
    assert_eq!(initial.revision, 0);
    assert_eq!(initial.snapshot, TabModel::new().snapshot());
    let revision = store
        .save(initial.revision, expected.clone())
        .expect("synthetic snapshot commits");
    assert_eq!(revision, 1);
    store.shutdown().expect("synthetic store closes");

    // Reconstruct a model only after opening a fresh guarded set of connections.
    let reopened = open_store(&directory.path);
    let stored = reopened.load().expect("committed synthetic snapshot loads");
    assert_eq!(stored.revision, revision);
    assert_eq!(stored.snapshot, expected);
    let restored = TabModel::from_snapshot(stored.snapshot).expect("stored snapshot is valid");
    assert_eq!(restored.active_id(), Some(live_tab));
    assert_eq!(restored.closed_count(), 0);
    assert_eq!(
        restored
            .get_saved(unloaded_saved)
            .expect("unloaded row persists")
            .metadata(),
        &metadata(1)
    );
    assert!(
        restored
            .tabs()
            .iter()
            .all(|tab| tab.saved_page_id() != Some(unloaded_saved))
    );
    assert_eq!(
        restored
            .get_saved(live_saved)
            .expect("saved target persists")
            .metadata(),
        &metadata(2)
    );
    assert_eq!(
        restored
            .get(live_tab)
            .expect("live tab persists")
            .metadata(),
        &metadata(20)
    );
    reopened
        .shutdown()
        .expect("reopened synthetic store closes");
    directory.cleanup();
}

#[test]
fn stale_revision_rejection_preserves_committed_records_and_watermarks() {
    let directory = DisposableDirectory::new();
    let store = open_store(&directory.path);
    let mut accepted = TabModel::new();
    let tab = open_tab(&mut accepted, 10);
    accepted.save_page(tab).expect("synthetic page saves");
    let expected = accepted.snapshot();
    let revision = store
        .save(0, expected.clone())
        .expect("first synthetic snapshot commits");

    // The rejected write changes metadata, ordering and both allocation watermarks.
    let mut stale = TabModel::from_snapshot(expected.clone()).expect("valid synthetic snapshot");
    stale
        .update_metadata(tab, metadata(11))
        .expect("synthetic metadata changes");
    let extra = open_tab(&mut stale, 12);
    stale
        .save_page(extra)
        .expect("additional synthetic row saves");
    assert_eq!(
        store.save(0, stale.snapshot()),
        Err(StoreError::StaleRevision {
            expected: 0,
            actual: revision,
        })
    );
    let unchanged = store.load().expect("snapshot loads after rejected write");
    assert_eq!(unchanged.revision, revision);
    assert_eq!(unchanged.snapshot, expected);
    store.shutdown().expect("synthetic store closes");

    // A new connection must observe the same atomic rejection.
    let reopened = open_store(&directory.path);
    let unchanged = reopened.load().expect("committed snapshot reopens");
    assert_eq!(unchanged.revision, revision);
    assert_eq!(unchanged.snapshot, expected);
    reopened
        .shutdown()
        .expect("reopened synthetic store closes");
    directory.cleanup();
}

#[test]
fn removed_identities_remain_retired_after_an_empty_session_is_persisted() {
    let directory = DisposableDirectory::new();
    let mut original = TabModel::new();
    let retired_tab = open_tab(&mut original, 30);
    let retired_saved = original
        .save_page(retired_tab)
        .expect("synthetic page saves");
    original.close(retired_tab).expect("synthetic tab closes");
    original
        .remove_saved(retired_saved)
        .expect("synthetic saved row is removed");
    let store = open_store(&directory.path);
    let revision = store
        .save(0, original.snapshot())
        .expect("empty synthetic state commits");
    store.shutdown().expect("synthetic store closes");

    // Empty row sets must not reset the persisted allocator watermarks.
    let reopened = open_store(&directory.path);
    let stored = reopened.load().expect("empty synthetic state reopens");
    assert_eq!(stored.revision, revision);
    assert_eq!(stored.snapshot, original.snapshot());
    let mut restored = TabModel::from_snapshot(stored.snapshot).expect("stored snapshot is valid");
    assert_eq!(restored.active_id(), None);
    assert!(restored.tabs().is_empty());
    assert!(restored.saved_pages().is_empty());
    assert_eq!(restored.reopen_closed(), Ok(None));
    let fresh_tab = open_tab(&mut restored, 31);
    let fresh_saved = restored
        .save_page(fresh_tab)
        .expect("fresh synthetic row saves");
    assert!(fresh_tab.get() > retired_tab.get());
    assert!(fresh_saved.get() > retired_saved.get());
    reopened
        .save(revision, restored.snapshot())
        .expect("fresh identities commit without reusing retired IDs");
    reopened
        .shutdown()
        .expect("reopened synthetic store closes");
    directory.cleanup();
}

#[test]
fn wal_cleanup_and_directory_release_follow_store_shutdown() {
    let directory = DisposableDirectory::new();
    let lease = Arc::new(
        WindowsDirectoryLease::prepare(&directory.path)
            .expect("native guard accepts disposable directory"),
    );
    assert!(matches!(
        lease.database_path().components().next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))
    ));
    let retained = Arc::downgrade(&lease);
    let store = Store::open(lease).expect("guarded synthetic store opens");
    let mut model = TabModel::new();
    open_tab(&mut model, 40);
    let expected = model.snapshot();
    let revision = store.save(0, expected.clone()).expect("WAL write commits");

    // An active store owns the directory lease and SQLite's live WAL sidecars.
    assert!(retained.upgrade().is_some());
    let wal = directory.path.join("syrinx.db-wal");
    assert!(checked_io(fs::metadata(&wal), "inspect synthetic WAL").len() > 0);
    assert!(exists(&directory.path.join("syrinx.db-shm")));
    let moved = directory.path.with_extension("moved");
    assert!(!exists(&moved));
    let refusal = fs::rename(&directory.path, &moved)
        .expect_err("a retained native directory lease must prevent rename");
    assert!(matches!(refusal.raw_os_error(), Some(5 | 32)));

    // The final connection close cleans sidecars before the guard releases its handles.
    store.shutdown().expect("WAL store shuts down cleanly");
    assert!(retained.upgrade().is_none());
    assert!(!exists(&wal));
    assert!(!exists(&directory.path.join("syrinx.db-shm")));
    assert!(exists(&directory.path.join("syrinx.db")));
    checked_io(
        fs::rename(&directory.path, &moved),
        "rename released test directory",
    );
    checked_io(
        fs::rename(&moved, &directory.path),
        "restore test directory name",
    );
    let reopened = open_store(&directory.path);
    let stored = reopened
        .load()
        .expect("checkpointed synthetic state reopens");
    assert_eq!(stored.revision, revision);
    assert_eq!(stored.snapshot, expected);
    reopened.shutdown().expect("reopened WAL store closes");
    directory.cleanup();
}

#[test]
fn sqlite_roundtrip_supports_paths_beyond_the_native_legacy_length() {
    roundtrip_at_path_length(350, |database| {
        assert!(database.as_os_str().encode_wide().count() > 260);
    });
}

#[test]
fn sqlite_roundtrip_supports_paths_beyond_the_default_vfs_capacity() {
    roundtrip_at_path_length(1100, |database| {
        let name = database.to_str().expect("synthetic path has Unicode text");
        assert!(name.len() > 1040);
    });
}

#[test]
fn sqlite_roundtrip_extends_a_path_when_only_its_sidecar_exceeds_the_limit() {
    roundtrip_at_path_length(252, |database| {
        let units = database.as_os_str().encode_wide().count();
        assert!(units < 260);
        assert!(units + "-journal".len() + 1 > 260);
    });
}

fn create_nested_path(root: &Path, database_units: usize) -> Vec<PathBuf> {
    // Use short synthetic components while choosing the complete path length exactly.
    let mut destination = root.to_owned();
    let mut current_units = destination
        .join("syrinx.db")
        .as_os_str()
        .encode_wide()
        .count();
    assert!(database_units.saturating_sub(current_units) >= 2);
    let mut children = Vec::new();
    while current_units < database_units {
        let remaining = database_units - current_units;
        let mut component_units = (remaining - 1).min(48);
        if remaining - component_units - 1 == 1 {
            component_units -= 1;
        }
        destination.push("x".repeat(component_units));
        checked_io(
            fs::create_dir(&destination),
            "create synthetic long-path child",
        );
        children.push(destination.clone());
        current_units += component_units + 1;
    }
    children
}

fn roundtrip_at_path_length(database_units: usize, verify_path: impl FnOnce(&Path)) {
    let directory = DisposableDirectory::new();
    let children = create_nested_path(&directory.path, database_units);
    let destination = children.last().expect("synthetic child directory exists");
    let database = destination.join("syrinx.db");
    assert_eq!(database.as_os_str().encode_wide().count(), database_units);
    verify_path(&database);

    // Extended DOS spelling must support SQLite and every sidecar without OS settings.
    let lease = WindowsDirectoryLease::prepare(destination)
        .expect("native guard accepts a disposable long path");
    assert!(matches!(
        lease.database_path().components().next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::VerbatimDisk(_))
    ));
    let store = Store::open(Arc::new(lease)).expect("long-path SQLite store opens");
    let mut model = TabModel::new();
    let tab = open_tab(&mut model, 50);
    model.save_page(tab).expect("synthetic page saves");
    let expected = model.snapshot();
    let revision = store
        .save(0, expected.clone())
        .expect("long-path snapshot commits");
    store.shutdown().expect("long-path store closes");

    // A fresh guarded connection must recover the persisted snapshot and release WAL files.
    let reopened = open_store(destination);
    let stored = reopened.load().expect("long-path snapshot reopens");
    assert_eq!(stored.revision, revision);
    assert_eq!(stored.snapshot, expected);
    reopened
        .shutdown()
        .expect("reopened long-path store closes");
    assert!(!exists(&destination.join("syrinx.db-wal")));
    assert!(!exists(&destination.join("syrinx.db-shm")));

    // Delete only the known database files, then the exact children this test created.
    for name in DATABASE_FILES.into_iter().chain(CONTROL_FILES) {
        remove_file_if_present(&destination.join(name));
    }
    for child in children.into_iter().rev() {
        checked_io(fs::remove_dir(child), "remove synthetic long-path child");
    }
    directory.cleanup();
}

#[test]
fn native_guard_refuses_hardlinked_database_and_each_sidecar_without_changes() {
    for name in DATABASE_FILES {
        let directory = DisposableDirectory::new();
        let original = directory.path.join("synthetic-original.bin");
        let linked = directory.path.join(name);
        checked_io(
            fs::write(&original, SENTINEL),
            "write synthetic hardlink target",
        );
        checked_io(
            fs::hard_link(&original, &linked),
            "create synthetic hardlink",
        );

        // Refusal must preserve the shared bytes and avoid creating a missing database.
        let error = WindowsDirectoryLease::prepare(&directory.path)
            .expect_err("hardlinked database entries must be refused");
        assert_eq!(error, LocationError::HardLinkedFile);
        assert_eq!(
            checked_io(fs::read(&original), "read synthetic target"),
            SENTINEL
        );
        if name != "syrinx.db" {
            assert!(!exists(&directory.path.join("syrinx.db")));
        }
        checked_io(fs::remove_file(&linked), "remove synthetic hardlink");
        assert_eq!(
            checked_io(fs::read(&original), "recheck synthetic target"),
            SENTINEL
        );
        checked_io(
            fs::remove_file(&original),
            "remove verified synthetic target",
        );
        directory.cleanup();
    }
}

fn create_junction(link: &Path, target: &Path) {
    // Static PowerShell receives paths as data; captured output never reaches diagnostics.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = checked_io(
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:SYRINX_TEST_JUNCTION -Target $env:SYRINX_TEST_TARGET | Out-Null",
            ])
            .env("SYRINX_TEST_JUNCTION", link)
            .env("SYRINX_TEST_TARGET", target)
            .creation_flags(CREATE_NO_WINDOW)
            .output(),
        "create synthetic junction with Windows PowerShell",
    );
    assert!(
        output.status.success(),
        "synthetic junction creation unavailable; exit code {:?}",
        output.status.code()
    );
}

#[test]
fn native_guard_refuses_a_junction_at_the_destination_and_in_its_ancestors() {
    let directory = DisposableDirectory::new();
    let target = directory.path.join("synthetic-target");
    let link = directory.path.join("synthetic-junction");
    checked_io(fs::create_dir(&target), "create synthetic junction target");
    let marker = target.join("sentinel.bin");
    checked_io(
        fs::write(&marker, SENTINEL),
        "write synthetic junction sentinel",
    );
    create_junction(&link, &target);

    // Neither the alias itself nor a missing child through it may be accepted.
    for candidate in [&link, &link.join("uncreated-child")] {
        let error = WindowsDirectoryLease::prepare(candidate)
            .expect_err("junction destination and ancestor must be refused");
        assert_eq!(error, LocationError::ReparsePoint);
        assert_eq!(
            checked_io(fs::read(&marker), "read junction sentinel"),
            SENTINEL
        );
        assert!(!exists(&target.join("syrinx.db")));
        assert!(!exists(&target.join("uncreated-child")));
    }

    // Remove the junction itself without traversing or recursively deleting its target.
    checked_io(fs::remove_dir(&link), "remove synthetic junction");
    assert_eq!(
        checked_io(fs::read(&marker), "recheck junction sentinel"),
        SENTINEL
    );
    checked_io(
        fs::remove_file(&marker),
        "remove verified junction sentinel",
    );
    checked_io(fs::remove_dir(&target), "remove synthetic junction target");
    directory.cleanup();
}

#[test]
fn native_guard_refuses_a_junction_in_place_of_a_cache_folder() {
    let directory = DisposableDirectory::new();
    let data = directory.path.join("data");
    let target = directory.path.join("synthetic-target");
    checked_io(fs::create_dir(&data), "create synthetic data directory");
    checked_io(fs::create_dir(&target), "create synthetic junction target");
    let link = data.join("synthetic-cache");
    create_junction(&link, &target);

    // A cache folder redirected elsewhere, perhaps into a synced folder, is never written.
    let lease = WindowsDirectoryLease::prepare(&data).expect("guard synthetic data");
    assert_eq!(
        lease.cache_folder("synthetic-cache").err(),
        Some(LocationError::ReparsePoint)
    );
    drop(lease);
    assert!(!exists(&target.join("meta.json")));

    // Remove the junction itself without traversing its target.
    checked_io(fs::remove_dir(&link), "remove synthetic junction");
    checked_io(fs::remove_dir(&target), "remove synthetic junction target");
    for name in DATABASE_FILES.into_iter().chain(CONTROL_FILES) {
        remove_file_if_present(&data.join(name));
    }
    checked_io(fs::remove_dir(&data), "remove synthetic data directory");
    directory.cleanup();
}

#[test]
fn native_session_excludes_an_alias_and_retains_ownership_after_lease_drop() {
    let directory = DisposableDirectory::new();
    let alias = checked_io(fs::canonicalize(&directory.path), "resolve synthetic alias");
    let lease = WindowsDirectoryLease::prepare(&directory.path)
        .expect("native guard accepts disposable directory");

    // A distinct path spelling must resolve to the same exclusive control file.
    assert_eq!(
        WindowsDirectoryLease::prepare(&alias).expect_err("an alias cannot acquire a second lease"),
        LocationError::StorageBusy
    );
    let session = lease
        .session_control()
        .expect("native lease supplies session tracking")
        .begin()
        .expect("first synthetic session begins");
    assert_eq!(session.previous_shutdown(), ShutdownState::Unclean);
    drop(lease);
    assert_eq!(
        WindowsDirectoryLease::prepare(&alias)
            .expect_err("an outstanding session token retains exclusive ownership"),
        LocationError::StorageBusy
    );

    // Consuming the final token releases the file while preserving its clean evidence.
    session.complete().expect("synthetic session completes");
    let reopened = open_store(&alias);
    assert_eq!(reopened.previous_shutdown(), ShutdownState::Clean);
    reopened.shutdown().expect("synthetic alias store closes");
    directory.cleanup();
}

#[test]
fn one_lease_refuses_overlapping_sessions_but_allows_a_later_session() {
    let directory = DisposableDirectory::new();
    let lease = WindowsDirectoryLease::prepare(&directory.path)
        .expect("native guard accepts disposable directory");
    let control = lease
        .session_control()
        .expect("native lease supplies session tracking");
    let first = control.begin().expect("first synthetic session begins");

    // Sharing one Arc cannot bypass the separate in-process session claim.
    assert!(matches!(
        control.begin(),
        Err(LocationError::SessionAlreadyActive)
    ));
    first.complete().expect("first synthetic session completes");
    let second = control.begin().expect("later synthetic session begins");
    assert_eq!(second.previous_shutdown(), ShutdownState::Clean);
    drop(second);
    let third = control.begin().expect("dropped session releases its claim");
    assert_eq!(third.previous_shutdown(), ShutdownState::Unclean);
    third.complete().expect("final synthetic session completes");
    drop(lease);
    directory.cleanup();
}

#[test]
fn a_second_store_on_the_same_lease_cannot_change_the_first_stores_records() {
    let directory = DisposableDirectory::new();
    let lease = Arc::new(
        WindowsDirectoryLease::prepare(&directory.path)
            .expect("native guard accepts disposable directory"),
    );
    let store = Store::open(lease.clone()).expect("first synthetic store opens");
    let mut model = TabModel::new();
    open_tab(&mut model, 60);
    let expected = model.snapshot();
    let revision = store
        .save(0, expected.clone())
        .expect("synthetic state commits");

    // Refuse before the second writer can open SQLite or change lifecycle state.
    let error = Store::open(lease.clone()).expect_err("one lease cannot open overlapping stores");
    assert_eq!(
        error,
        StoreError::Location(LocationError::SessionAlreadyActive)
    );
    let unchanged = store
        .load()
        .expect("original synthetic store remains readable");
    assert_eq!(unchanged.revision, revision);
    assert_eq!(unchanged.snapshot, expected);
    store.shutdown().expect("first synthetic store closes");
    let reopened = Store::open(lease.clone()).expect("same lease permits a later store");
    assert_eq!(reopened.previous_shutdown(), ShutdownState::Clean);
    assert_eq!(reopened.load().expect("synthetic state reopens"), unchanged);
    reopened.shutdown().expect("later synthetic store closes");
    drop(lease);
    directory.cleanup();
}

#[test]
fn restart_distinguishes_explicit_shutdown_drop_and_malformed_lifecycle_evidence() {
    let directory = DisposableDirectory::new();
    let store = open_store(&directory.path);
    assert_eq!(store.previous_shutdown(), ShutdownState::Unclean);
    let mut model = TabModel::new();
    open_tab(&mut model, 61);
    let expected = model.snapshot();
    let revision = store
        .save(0, expected.clone())
        .expect("synthetic state commits");
    store.shutdown().expect("first synthetic store closes");

    // A normal shutdown is clean; fallback Drop closes connections but leaves dirty evidence.
    let clean_restart = open_store(&directory.path);
    assert_eq!(clean_restart.previous_shutdown(), ShutdownState::Clean);
    drop(clean_restart);
    let dirty_restart = open_store(&directory.path);
    assert_eq!(dirty_restart.previous_shutdown(), ShutdownState::Unclean);
    assert_eq!(
        dirty_restart
            .load()
            .expect("unclean store validates")
            .snapshot,
        expected
    );
    dirty_restart
        .shutdown()
        .expect("validated synthetic store closes");

    // Neither absent nor malformed evidence can suppress the next startup's validation.
    let lifecycle = directory.path.join("syrinx.lifecycle");
    remove_file_if_present(&lifecycle);
    for malformed in [
        None,
        Some(b"".as_slice()),
        Some(b"synthetic-invalid-record".as_slice()),
    ] {
        if let Some(bytes) = malformed {
            checked_io(
                fs::write(&lifecycle, bytes),
                "write synthetic malformed lifecycle",
            );
        }
        let reopened = open_store(&directory.path);
        assert_eq!(reopened.previous_shutdown(), ShutdownState::Unclean);
        let stored = reopened
            .load()
            .expect("synthetic records survive lifecycle uncertainty");
        assert_eq!(stored.revision, revision);
        assert_eq!(stored.snapshot, expected);
        reopened
            .shutdown()
            .expect("synthetic store closes after validation");
    }
    directory.cleanup();
}

#[test]
fn pending_recovery_intent_refuses_startup_before_creating_a_missing_database() {
    let directory = DisposableDirectory::new();
    let intent = directory.path.join("syrinx.recovery-intent");
    checked_io(
        fs::write(&intent, SENTINEL),
        "write synthetic recovery intent",
    );

    // Any existing intent requires recovery handling, even when its bytes are unrecognized.
    let error = WindowsDirectoryLease::prepare(&directory.path)
        .expect_err("pending recovery must prevent ordinary startup");
    assert_eq!(error, LocationError::RecoveryPending);
    assert!(!exists(&directory.path.join("syrinx.db")));
    assert_eq!(
        checked_io(fs::read(&intent), "read synthetic intent"),
        SENTINEL
    );
    directory.cleanup();
}

#[test]
fn native_guard_refuses_hardlinked_control_files_without_modifying_their_targets() {
    for name in CONTROL_FILES {
        let directory = DisposableDirectory::new();
        let original = directory.path.join("synthetic-control-target.bin");
        let linked = directory.path.join(name);
        checked_io(
            fs::write(&original, SENTINEL),
            "write synthetic control target",
        );
        checked_io(
            fs::hard_link(&original, &linked),
            "hardlink synthetic control target",
        );

        // Control metadata must be checked before interpreting or rewriting its bytes.
        let error = WindowsDirectoryLease::prepare(&directory.path)
            .expect_err("hardlinked control file must be refused");
        assert_eq!(error, LocationError::HardLinkedFile);
        assert!(!exists(&directory.path.join("syrinx.db")));
        assert_eq!(
            checked_io(fs::read(&original), "read synthetic control target"),
            SENTINEL
        );
        checked_io(
            fs::remove_file(&linked),
            "remove synthetic control hardlink",
        );
        assert_eq!(
            checked_io(fs::read(&original), "recheck synthetic control target"),
            SENTINEL
        );
        checked_io(
            fs::remove_file(&original),
            "remove verified synthetic control target",
        );
        directory.cleanup();
    }
}

#[test]
fn native_guard_refuses_directories_in_place_of_control_files() {
    for name in CONTROL_FILES {
        let directory = DisposableDirectory::new();
        let control = directory.path.join(name);
        checked_io(
            fs::create_dir(&control),
            "create synthetic control directory",
        );
        let marker = control.join("sentinel.bin");
        checked_io(
            fs::write(&marker, SENTINEL),
            "write synthetic control sentinel",
        );

        let error = WindowsDirectoryLease::prepare(&directory.path)
            .expect_err("a control path must be a regular file");
        assert_eq!(error, LocationError::UnexpectedEntryKind);
        assert!(!exists(&directory.path.join("syrinx.db")));
        assert_eq!(
            checked_io(fs::read(&marker), "read synthetic control sentinel"),
            SENTINEL
        );
        checked_io(fs::remove_file(&marker), "remove verified control sentinel");
        checked_io(
            fs::remove_dir(&control),
            "remove synthetic control directory",
        );
        directory.cleanup();
    }
}

#[test]
fn native_guard_refuses_junctions_in_place_of_control_files() {
    for name in CONTROL_FILES {
        let directory = DisposableDirectory::new();
        let target = directory.path.join("synthetic-control-target");
        let link = directory.path.join(name);
        checked_io(
            fs::create_dir(&target),
            "create synthetic control junction target",
        );
        let marker = target.join("sentinel.bin");
        checked_io(
            fs::write(&marker, SENTINEL),
            "write synthetic control junction sentinel",
        );
        create_junction(&link, &target);

        let error = WindowsDirectoryLease::prepare(&directory.path)
            .expect_err("a control path must not redirect to another entry");
        assert_eq!(error, LocationError::ReparsePoint);
        assert!(!exists(&directory.path.join("syrinx.db")));
        assert_eq!(
            checked_io(fs::read(&marker), "read control junction sentinel"),
            SENTINEL
        );
        checked_io(fs::remove_dir(&link), "remove synthetic control junction");
        assert_eq!(
            checked_io(fs::read(&marker), "recheck control junction sentinel"),
            SENTINEL
        );
        checked_io(
            fs::remove_file(&marker),
            "remove verified control junction sentinel",
        );
        checked_io(
            fs::remove_dir(&target),
            "remove synthetic control junction target",
        );
        directory.cleanup();
    }
}

#[test]
fn refused_foreign_database_keeps_its_bytes_and_leaves_unclean_lifecycle_evidence() {
    let directory = DisposableDirectory::new();
    let store = open_store(&directory.path);
    store.shutdown().expect("synthetic seed database closes");
    let database = directory.path.join("syrinx.db");

    // Change only the SQLite header's application ID in this test-owned, closed database.
    let mut foreign_bytes = checked_io(fs::read(&database), "read synthetic database header");
    assert!(foreign_bytes.len() >= 100);
    foreign_bytes[68..72].copy_from_slice(&0x5359_4e54_u32.to_be_bytes());
    checked_io(
        fs::write(&database, &foreign_bytes),
        "write synthetic foreign database ID",
    );
    let lease = WindowsDirectoryLease::prepare(&directory.path)
        .expect("foreign synthetic file passes native path checks");
    let error = Store::open(Arc::new(lease)).expect_err("foreign application ID must be refused");
    assert_eq!(error, StoreError::ForeignDatabase);
    assert_eq!(
        checked_io(fs::read(&database), "recheck synthetic foreign database"),
        foreign_bytes
    );

    // Failed startup must release ownership without turning its lifecycle record clean.
    let lease = WindowsDirectoryLease::prepare(&directory.path)
        .expect("failed store startup releases its directory lease");
    let session = lease
        .session_control()
        .expect("native lease supplies session tracking")
        .begin()
        .expect("synthetic session starts after refusal");
    assert_eq!(session.previous_shutdown(), ShutdownState::Unclean);
    drop(session);
    drop(lease);
    directory.cleanup();
}

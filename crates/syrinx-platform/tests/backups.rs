//! Proves native snapshot publication and rotation against disposable SQLite stores.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use syrinx_core::{Activation, TabKind, TabMetadata, TabModel};
use syrinx_platform::WindowsDirectoryLease;
use syrinx_store::{BackupOutcome, DirectoryLease, Store, validate_backup};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
const DAY: i64 = 86_400;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("synthetic clock follows epoch")
            .as_nanos();
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "syrinx-backup-integration-{}-{stamp}-{sequence}",
            std::process::id()
        ));
        checked(fs::create_dir(&path));
        Self(path)
    }

    fn lease(&self) -> Arc<WindowsDirectoryLease> {
        Arc::new(WindowsDirectoryLease::prepare(&self.0).expect("guard synthetic location"))
    }

    fn cleanup(self) {
        // The test exclusively created this tree; refuse links instead of following them.
        remove_owned_tree(&self.0);
    }
}

fn checked<T>(result: io::Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("synthetic file operation failed: {:?}", error.kind()))
}

fn remove_owned_tree(path: &Path) {
    use std::os::windows::fs::MetadataExt;
    let metadata = checked(fs::symlink_metadata(path));
    assert_eq!(
        metadata.file_attributes() & 0x400,
        0,
        "never follow a reparse point"
    );
    if !metadata.is_dir() {
        checked(fs::remove_file(path));
        return;
    }
    for entry in checked(fs::read_dir(path)) {
        remove_owned_tree(&checked(entry).path());
    }
    checked(fs::remove_dir(path));
}

fn synthetic_model(number: u64) -> TabModel {
    let mut model = TabModel::new();
    let tab = model
        .open(
            TabKind::Web,
            TabMetadata {
                title: format!("Synthetic backup {number}"),
                address: format!("https://backup-{number}.test/"),
                favicon: None,
                badge: None,
            },
            Activation::Foreground,
        )
        .expect("allocate synthetic tab");
    model.save_page(tab).expect("save synthetic destination");
    model
}

#[test]
fn eight_idle_daily_snapshots_keep_seven_valid_copies_and_restart_preserves_schedule() {
    let fixture = Fixture::new();
    let lease = fixture.lease();
    let archive = lease.backup_archive().expect("native archive capability");
    let store = Store::open(lease.clone()).expect("open synthetic store");
    assert_eq!(
        store.backup_if_due(DAY, false).unwrap(),
        BackupOutcome::NotIdle
    );
    for day in 1..=8 {
        store
            .save(day - 1, synthetic_model(day).snapshot())
            .unwrap();
        let result = store.backup_if_due(day as i64 * DAY, true).unwrap();
        assert!(matches!(result, BackupOutcome::Created(metadata)
            if metadata.revision == day && metadata.deletion_generation == 0));
    }
    let mut candidates = archive.candidates().unwrap();
    candidates.sort_by_key(|candidate| candidate.metadata.created_at);
    assert_eq!(candidates.len(), 7);
    for (index, candidate) in candidates.iter().enumerate() {
        let expected_revision = index as u64 + 2;
        let verified = validate_backup(candidate.file.as_ref()).unwrap();
        assert_eq!(verified.revision, expected_revision);
        assert_eq!(
            verified.snapshot,
            synthetic_model(expected_revision).snapshot()
        );
    }
    drop(candidates);
    store.shutdown().unwrap();
    drop(archive);
    drop(lease);

    let lease = fixture.lease();
    let store = Store::open(lease).unwrap();
    assert_eq!(
        store.backup_if_due(8 * DAY + DAY - 1, true).unwrap(),
        BackupOutcome::NotDue
    );
    assert_eq!(
        store.backup_if_due(DAY, true).unwrap(),
        BackupOutcome::NotDue
    );
    assert!(matches!(
        store.backup_if_due(9 * DAY, true).unwrap(),
        BackupOutcome::Created(_)
    ));
    store.shutdown().unwrap();
    fixture.cleanup();
}

#[test]
fn interrupted_unpublished_output_never_counts_as_a_backup_or_gets_pruned() {
    let fixture = Fixture::new();
    let lease = fixture.lease();
    let archive = lease.backup_archive().unwrap();
    let store = Store::open(lease.clone()).unwrap();
    let staged = archive.reserve().unwrap();
    let incomplete_path = staged.database_path().to_owned();
    drop(staged);
    checked(fs::write(
        &incomplete_path,
        b"incomplete synthetic snapshot",
    ));
    assert!(archive.candidates().unwrap().is_empty());
    for day in 1..=8 {
        assert!(matches!(
            store.backup_if_due(day * DAY, true).unwrap(),
            BackupOutcome::Created(_)
        ));
    }
    assert_eq!(archive.candidates().unwrap().len(), 7);
    assert_eq!(
        checked(fs::read(&incomplete_path)),
        b"incomplete synthetic snapshot"
    );
    store.shutdown().unwrap();
    drop(archive);
    drop(lease);
    fixture.cleanup();
}

#[test]
fn a_corrupt_newest_snapshot_does_not_hide_an_older_valid_copy() {
    let fixture = Fixture::new();
    let lease = fixture.lease();
    let archive = lease.backup_archive().unwrap();
    let store = Store::open(lease.clone()).unwrap();
    store.save(0, synthetic_model(1).snapshot()).unwrap();
    store.backup_if_due(DAY, true).unwrap();
    store.save(1, synthetic_model(2).snapshot()).unwrap();
    store.backup_if_due(2 * DAY, true).unwrap();
    let mut candidates = archive.candidates().unwrap();
    candidates.sort_by_key(|candidate| candidate.metadata.created_at);
    let newest_id = candidates.last().unwrap().metadata.id;
    let newest_path = candidates.last().unwrap().file.database_path().to_owned();
    drop(candidates);
    store.shutdown().unwrap();
    drop(archive);
    drop(lease);
    checked(fs::write(&newest_path, b"synthetic damaged SQLite output"));
    let lease = fixture.lease();
    let archive = lease.backup_archive().unwrap();
    let store = Store::open(lease.clone()).unwrap();
    // A damaged latest copy cannot advance the last-successful scheduling evidence.
    assert!(matches!(
        store.backup_if_due(2 * DAY, true).unwrap(),
        BackupOutcome::Created(_)
    ));
    let candidates = archive.candidates().unwrap();
    let mut verified = Vec::new();
    for candidate in &candidates {
        let validation = validate_backup(candidate.file.as_ref());
        if candidate.metadata.id == newest_id {
            assert!(
                validation.is_err(),
                "damaged synthetic file must fail validation"
            );
        } else {
            verified.push(validation.unwrap());
        }
    }
    assert_eq!(verified.len(), 2);
    assert!(verified.iter().any(|snapshot| snapshot.revision == 1));
    assert!(verified.iter().any(|snapshot| snapshot.revision == 2));
    drop(candidates);
    store.shutdown().unwrap();
    drop(archive);
    drop(lease);
    fixture.cleanup();
}

#[test]
fn backup_bundles_support_the_extended_windows_path_used_by_sqlite() {
    let fixture = Fixture::new();
    let data = fixture.0.join("synthetic-backup-".repeat(12));
    let lease = Arc::new(WindowsDirectoryLease::prepare(&data).unwrap());
    let archive = lease.backup_archive().unwrap();
    let store = Store::open(lease.clone()).unwrap();
    store.save(0, synthetic_model(1).snapshot()).unwrap();
    assert!(matches!(
        store.backup_if_due(DAY, true).unwrap(),
        BackupOutcome::Created(_)
    ));
    let candidates = archive.candidates().unwrap();
    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    assert!(
        candidate
            .file
            .database_path()
            .to_str()
            .unwrap()
            .starts_with(r"\\?\")
    );
    assert_eq!(
        validate_backup(candidate.file.as_ref()).unwrap().snapshot,
        synthetic_model(1).snapshot()
    );
    drop(candidates);
    store.shutdown().unwrap();
    drop(archive);
    drop(lease);
    fixture.cleanup();
}

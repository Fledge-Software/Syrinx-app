//! Exercises guarded startup against disposable established and missing primary files.

#[cfg(windows)]
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use syrinx_core::{Activation, TabKind, TabMetadata, TabModel};
use syrinx_store::{BackupMetadata, BackupOutcome, DirectoryLease, Store, StoredSnapshot};

use super::*;
use crate::storage::tests::Fixture;

fn seed(fixture: &Fixture) -> (BackupMetadata, StoredSnapshot, PathBuf) {
    let lease = Arc::new(WindowsDirectoryLease::prepare(&fixture.root).expect("synthetic lease"));
    let archive = lease.backup_archive().expect("synthetic archive");
    let store = Store::open(lease).expect("synthetic store");
    let mut model = TabModel::new();
    model
        .open(
            TabKind::Web,
            TabMetadata {
                title: "Synthetic guarded startup".to_owned(),
                address: "https://startup.example.test/".to_owned(),
                ..TabMetadata::default()
            },
            Activation::Foreground,
        )
        .expect("synthetic tab");
    let snapshot = model.snapshot();
    let revision = store
        .save(0, snapshot.clone())
        .expect("save synthetic snapshot");
    let BackupOutcome::Created(metadata) = store
        .backup_if_due(86_400, true)
        .expect("back up synthetic snapshot")
    else {
        panic!("synthetic backup must be due");
    };
    store.shutdown().expect("close synthetic seed");
    let entries = archive.candidates().expect("synthetic candidates");
    assert_eq!(entries.len(), 1);
    let backup = entries[0].file.database_path().to_owned();
    (metadata, StoredSnapshot { revision, snapshot }, backup)
}

#[cfg(windows)]
#[test]
fn missing_primary_offer_retains_ownership_and_cancellation_preserves_absence() {
    let fixture = Fixture::new();
    let (metadata, expected, backup) = seed(&fixture);
    let primary = fixture.root.join(DATABASE_NAME);
    fs::remove_file(&primary).expect("remove only synthetic primary");
    let StartupStorage::Recovery(offer) = prepare_startup(&fixture.root, &DeletionLog::default())
        .expect("offer missing synthetic primary")
    else {
        panic!("missing primary must not silently initialize");
    };
    assert_eq!(offer.metadata(), metadata);
    assert!(!primary.exists());
    assert!(matches!(
        WindowsDirectoryLease::prepare(&fixture.root),
        Err(LocationError::StorageBusy)
    ));
    assert!(fs::write(&backup, b"synthetic competing replacement").is_err());
    drop(offer);
    assert!(!primary.exists());
    assert!(!fixture.root.join(session::RECOVERY_INTENT_NAME).exists());

    let StartupStorage::Recovery(offer) = prepare_startup(&fixture.root, &DeletionLog::default())
        .expect("reoffer after cancellation")
    else {
        panic!("cancellation must preserve recovery eligibility");
    };
    let outcome = offer
        .confirm()
        .expect("explicitly restore synthetic backup");
    assert_eq!(outcome.restored, metadata);
    assert_eq!(outcome.preserved_files, 0);
    let StartupStorage::Ready(lease) =
        prepare_startup(&fixture.root, &DeletionLog::default()).expect("reopen restored primary")
    else {
        panic!("confirmed primary must open normally");
    };
    let store = Store::open(Arc::new(lease)).expect("open restored synthetic store");
    assert_eq!(store.load().expect("read restored snapshot"), expected);
    store.shutdown().expect("close restored synthetic store");
}

#[test]
fn existing_empty_or_interrupted_initialization_never_creates_a_primary() {
    for control in [
        None,
        Some(b"".as_slice()),
        Some(b"synthetic malformed lifecycle".as_slice()),
    ] {
        let fixture = Fixture::new();
        if let Some(bytes) = control {
            fs::write(fixture.root.join(session::CONTROL_NAME), bytes).expect("synthetic control");
        }
        assert!(matches!(
            prepare_startup(&fixture.root, &DeletionLog::default()),
            Err(RecoveryError::NoValidBackup)
        ));
        assert!(!fixture.root.join(DATABASE_NAME).exists());
        if let Some(bytes) = control {
            assert_eq!(
                fs::read(fixture.root.join(session::CONTROL_NAME)).expect("read unchanged control"),
                bytes
            );
        }
    }
}

#[test]
fn missing_primary_with_invalid_backup_or_sidecar_is_not_fresh() {
    let fixture = Fixture::new();
    let (_, _, backup) = seed(&fixture);
    fs::remove_file(fixture.root.join(DATABASE_NAME)).expect("remove synthetic primary");
    fs::write(&backup, b"synthetic invalid backup").expect("damage synthetic backup");
    for name in SIDECAR_NAMES {
        fs::write(fixture.root.join(name), b"synthetic surviving sidecar")
            .expect("synthetic sidecar");
    }
    assert!(matches!(
        prepare_startup(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::NoValidBackup)
    ));
    assert!(!fixture.root.join(DATABASE_NAME).exists());
    for name in SIDECAR_NAMES {
        assert_eq!(
            fs::read(fixture.root.join(name)).expect("read preserved sidecar"),
            b"synthetic surviving sidecar"
        );
    }
}

#[test]
fn missing_primary_with_incomplete_archive_and_malformed_intent_fails_closed() {
    let fixture = Fixture::new();
    let incomplete = fixture.root.join("backups").join("b-0000000000000001");
    fs::create_dir_all(&incomplete).expect("synthetic incomplete archive");
    fs::write(
        incomplete.join(DATABASE_NAME),
        b"synthetic incomplete backup",
    )
    .expect("synthetic backup bytes");
    assert!(matches!(
        prepare_startup(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::NoValidBackup)
    ));
    assert!(!fixture.root.join(DATABASE_NAME).exists());
    let intent = fixture.root.join(session::RECOVERY_INTENT_NAME);
    fs::write(&intent, b"synthetic invalid intent").expect("synthetic interrupted intent");
    assert!(matches!(
        prepare_startup(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::InvalidIntent)
    ));
    assert!(!fixture.root.join(DATABASE_NAME).exists());
    assert_eq!(
        fs::read(intent).expect("read unchanged intent"),
        b"synthetic invalid intent"
    );
}

#[cfg(windows)]
#[test]
fn existing_primary_with_journal_follows_normal_store_open() {
    let fixture = Fixture::new();
    let (_, expected, _) = seed(&fixture);
    let primary = fixture.root.join(DATABASE_NAME);
    let before = fs::read(&primary).expect("read healthy primary");
    fs::write(
        fixture.root.join("syrinx.db-journal"),
        b"synthetic journal sibling",
    )
    .expect("synthetic journal");
    let StartupStorage::Ready(lease) = prepare_startup(&fixture.root, &DeletionLog::default())
        .expect("normal open must not require read-only journal recovery")
    else {
        panic!("existing primary must return its lease");
    };
    assert_eq!(fs::read(&primary).expect("read unchanged primary"), before);
    assert!(fs::rename(&primary, fixture.root.join("moved.db")).is_err());
    let store = Store::open(Arc::new(lease)).expect("normal SQLite journal handling");
    assert_eq!(store.load().expect("read synthetic state"), expected);
    store.shutdown().expect("close synthetic state");
}

#[test]
fn fresh_final_directory_initializes_and_reopens_normally() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("new-parent").join("State");
    let StartupStorage::Ready(lease) = prepare_startup(&destination, &DeletionLog::default())
        .expect("initialize new synthetic State")
    else {
        panic!("fresh directory must initialize");
    };
    let store = Store::open(Arc::new(lease)).expect("initialize synthetic store schema");
    let expected = store.load().expect("load fresh snapshot");
    store.shutdown().expect("close initialized store");
    let StartupStorage::Ready(lease) =
        prepare_startup(&destination, &DeletionLog::default()).expect("reopen established State")
    else {
        panic!("existing primary must open normally");
    };
    let store = Store::open(Arc::new(lease)).expect("reopen synthetic store");
    assert_eq!(store.load().expect("load reopened snapshot"), expected);
    store.shutdown().expect("close reopened store");
}

#[cfg(windows)]
#[test]
fn atomic_creation_retains_the_created_directory_until_release() {
    let fixture = Fixture::new();
    let parent = GuardedEntry::directory(&fixture.root).expect("retain synthetic parent");
    let destination = fixture.root.join("State");
    let file = native::create_directory(&parent.file, OsStr::new("State"))
        .expect("atomically create synthetic child");
    let child = GuardedEntry::from_directory_file(&destination, file)
        .expect("inspect the returned directory handle");
    assert!(fs::rename(&destination, fixture.root.join("replacement")).is_err());
    assert!(fs::remove_dir(&destination).is_err());
    assert_eq!(
        native::create_directory(&parent.file, OsStr::new("State"))
            .expect_err("strict native creation refuses an existing directory"),
        LocationError::Os {
            operation: LocationOperation::CreateDirectory,
            code: Some(
                windows::core::HRESULT::from_win32(
                    windows::Win32::Foundation::ERROR_ALREADY_EXISTS.0
                )
                .0
            ),
        }
    );
    child
        .revalidate(true, LocationOperation::OpenDirectory)
        .expect("same retained identity");
    drop(child);
    fs::rename(&destination, fixture.root.join("replacement")).expect("released child can move");
    fs::create_dir(&destination).expect("replacement possible only after release");
}

#[test]
fn racing_final_directory_is_not_adopted_as_fresh() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("State");
    let competing = destination.clone();
    let probe = OnSyncCheck::new(0, move || {
        fs::create_dir(&competing).expect("synthetic competing directory creator");
        fs::write(competing.join("owner"), b"synthetic competing entry").expect("competing bytes");
    });
    assert!(matches!(
        prepare_with_probe(&destination, &DeletionLog::default(), probe),
        Err(RecoveryError::Location(LocationError::Os {
            operation: LocationOperation::CreateDirectory,
            code: Some(_),
        }))
    ));
    assert_eq!(
        fs::read(destination.join("owner")).expect("preserved competing bytes"),
        b"synthetic competing entry"
    );
    assert!(!destination.join(DATABASE_NAME).exists());
    assert!(!destination.join(session::CONTROL_NAME).exists());
}

#[test]
fn racing_lifecycle_control_is_not_adopted_or_overwritten() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("State");
    let competing = destination.join(session::CONTROL_NAME);
    let probe = OnSyncCheck::new(1, move || {
        fs::write(&competing, b"synthetic competing lifecycle").expect("race lifecycle creation");
    });
    assert!(matches!(
        prepare_with_probe(&destination, &DeletionLog::default(), probe),
        Err(RecoveryError::Location(LocationError::Os {
            operation: LocationOperation::OpenControl,
            code: Some(_),
        }))
    ));
    assert_eq!(
        fs::read(destination.join(session::CONTROL_NAME)).expect("preserved control"),
        b"synthetic competing lifecycle"
    );
    assert!(!destination.join(DATABASE_NAME).exists());
}

#[test]
fn unexpected_entries_in_new_state_prevent_fresh_initialization() {
    for name in [
        DATABASE_NAME,
        "unknown",
        session::RECOVERY_INTENT_NAME,
        "syrinx.db-wal",
        "syrinx.db-shm",
        "syrinx.db-journal",
    ] {
        let fixture = Fixture::new();
        let destination = fixture.root.join("State");
        let (prepared, created) =
            prepare_directory_with_creation(&destination, Arc::new(native::NativeProbe), true)
                .expect("prepare new synthetic State");
        assert!(created);
        fs::write(destination.join(name), b"synthetic competing entry")
            .expect("add competing entry");
        assert!(matches!(
            acquire(prepared, created, &DeletionLog::default()),
            Err(RecoveryError::Location(LocationError::IdentityChanged))
        ));
        assert_eq!(
            fs::read(destination.join(name)).expect("preserve competing entry"),
            b"synthetic competing entry"
        );
        if name != DATABASE_NAME {
            assert!(!destination.join(DATABASE_NAME).exists());
        }
    }
}

#[test]
fn archive_directory_in_new_state_prevents_fresh_initialization() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("State");
    let (prepared, created) =
        prepare_directory_with_creation(&destination, Arc::new(native::NativeProbe), true)
            .expect("prepare new synthetic State");
    assert!(created);
    fs::create_dir(destination.join("backups")).expect("synthetic unexpected archive");
    assert!(matches!(
        acquire(prepared, created, &DeletionLog::default()),
        Err(RecoveryError::Location(LocationError::IdentityChanged))
    ));
    assert!(destination.join("backups").is_dir());
    assert!(!destination.join(DATABASE_NAME).exists());
}

#[test]
fn primary_creation_race_refuses_after_the_empty_inventory_check() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("State");
    let (prepared, created) =
        prepare_directory_with_creation(&destination, Arc::new(native::NativeProbe), true)
            .expect("prepare new synthetic State");
    assert!(created);
    assert!(contains_only_control(&prepared).expect("empty checked inventory"));
    fs::write(destination.join(DATABASE_NAME), b"synthetic racing primary")
        .expect("competing primary creator");
    assert!(matches!(
        create_fresh_primary(prepared),
        Err(RecoveryError::Location(LocationError::Os {
            operation: LocationOperation::CreateDatabase,
            code: Some(_),
        }))
    ));
    assert_eq!(
        fs::read(destination.join(DATABASE_NAME)).expect("unchanged racing primary"),
        b"synthetic racing primary"
    );
}

#[cfg(windows)]
#[test]
fn atomic_directory_creation_rejects_invalid_components_and_wrong_parent() {
    let fixture = Fixture::new();
    let parent = GuardedEntry::directory(&fixture.root).expect("retain synthetic parent");
    for name in [
        "",
        ".",
        "..",
        "child\\other",
        "child/other",
        "child:stream",
        "child\0name",
    ] {
        assert!(matches!(
            native::create_directory(&parent.file, OsStr::new(name)),
            Err(LocationError::InvalidPath)
        ));
    }
    assert!(matches!(
        native::create_directory(&parent.file, OsStr::new(&"x".repeat(32_768))),
        Err(LocationError::PathTooLong)
    ));
    let path = fixture.root.join("regular-file");
    fs::write(&path, b"synthetic parent of wrong kind").expect("synthetic regular file");
    let file = native::optional_file(&path, LocationOperation::OpenDatabase)
        .expect("open synthetic file")
        .expect("synthetic file exists");
    assert!(matches!(
        native::create_directory(&file, OsStr::new("State")),
        Err(LocationError::Os {
            operation: LocationOperation::CreateDirectory,
            code: Some(_)
        })
    ));
    assert_eq!(
        fs::read(&path).expect("preserved regular file"),
        b"synthetic parent of wrong kind"
    );
    assert!(!fixture.root.join("State").exists());
}

#[cfg(windows)]
#[test]
fn existing_junction_is_refused_without_following_or_initializing_its_target() {
    let fixture = Fixture::new();
    let target = fixture.root.join("target");
    fs::create_dir(&target).expect("synthetic junction target");
    fs::write(target.join("marker"), b"synthetic target marker").expect("synthetic marker");
    // Only fixed relative names reach the shell; its working directory is this owned fixture.
    let output = Command::new("cmd.exe")
        .current_dir(&fixture.root)
        .args(["/d", "/c", "mklink", "/J", "reparse", "target"])
        .output()
        .expect("create disposable directory junction");
    assert!(
        output.status.success(),
        "synthetic junction creation must succeed"
    );
    let parent = GuardedEntry::directory(&fixture.root).expect("retain synthetic parent");
    assert!(native::create_directory(&parent.file, OsStr::new("reparse")).is_err());
    assert!(matches!(
        prepare_startup(&fixture.root.join("reparse"), &DeletionLog::default()),
        Err(RecoveryError::Location(LocationError::ReparsePoint))
    ));
    assert!(!target.join(DATABASE_NAME).exists());
    assert!(!target.join(session::CONTROL_NAME).exists());
    assert_eq!(
        fs::read(target.join("marker")).expect("preserved target marker"),
        b"synthetic target marker"
    );
    fs::remove_dir(fixture.root.join("reparse")).expect("remove only the synthetic junction");
}

struct OnSyncCheck {
    check: usize,
    calls: AtomicUsize,
    action: Box<dyn Fn() + Send + Sync>,
}

impl OnSyncCheck {
    fn new(check: usize, action: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            check,
            calls: AtomicUsize::new(0),
            action: Box::new(action),
        })
    }
}

impl LocationProbe for OnSyncCheck {
    fn drive_type(&self, root: &Path) -> Result<u32, LocationError> {
        native::NativeProbe.drive_type(root)
    }

    fn sync_roots(&self) -> Result<Vec<PathBuf>, LocationError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == self.check {
            (self.action)();
        }
        Ok(Vec::new())
    }
}

#[test]
fn a_pending_or_completed_clear_does_not_block_normal_startup() {
    // The session finishes or replays clears after opening; startup itself is unaffected.
    let fixture = Fixture::new();
    let destination = fixture.root.join("State");
    let mut deletions = DeletionLog::default();
    let record = deletions
        .request(
            syrinx_store::deletion::DeletionCategory::History,
            syrinx_store::deletion::DeletionRange::new(10, 20).expect("synthetic interval"),
        )
        .expect("synthetic deletion request");
    deletions.accept(record).expect("accept synthetic deletion");
    assert!(matches!(
        prepare_startup(&destination, &deletions),
        Ok(StartupStorage::Ready(_))
    ));
    deletions
        .accept(deletions.completion().expect("synthetic completion"))
        .expect("complete synthetic deletion");
    assert!(matches!(
        prepare_startup(&destination, &deletions),
        Ok(StartupStorage::Ready(_))
    ));
}

#[test]
fn existing_unsafe_primary_and_busy_control_preserve_refusals() {
    let fixture = Fixture::new();
    let primary = fixture.root.join(DATABASE_NAME);
    fs::create_dir(&primary).expect("synthetic wrong-kind primary");
    assert!(matches!(
        prepare_startup(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::Location(LocationError::UnexpectedEntryKind))
    ));
    fs::remove_dir(&primary).expect("remove own synthetic empty directory");
    let lease = WindowsDirectoryLease::prepare(&fixture.root).expect("hold synthetic lease");
    assert!(matches!(
        prepare_startup(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::Location(LocationError::StorageBusy))
    ));
    drop(lease);
}

#[cfg(windows)]
#[test]
fn existing_empty_directory_acquisitions_retain_the_name_without_child_locks() {
    for optional in [false, true] {
        let fixture = Fixture::new();
        let destination = fixture.root.join("State");
        let moved = fixture.root.join("moved");
        fs::create_dir(&destination).expect("create synthetic empty directory");
        let guard = if optional {
            let file = native::optional_directory(&destination)
                .expect("retain existing directory")
                .expect("synthetic directory exists");
            GuardedEntry::from_directory_file(&destination, file).expect("check retained directory")
        } else {
            GuardedEntry::directory(&destination).expect("retain checked directory")
        };
        assert!(fs::rename(&destination, &moved).is_err());
        assert!(fs::remove_dir(&destination).is_err());
        guard
            .revalidate(true, LocationOperation::OpenDirectory)
            .expect("metadata probe remains compatible with retained directory");
        drop(guard);
        fs::rename(&destination, &moved).expect("released directory name can move");
        fs::remove_dir(&moved).expect("released empty directory can be removed");
    }
}

#[cfg(windows)]
#[test]
fn legacy_existing_primary_lease_retains_name_until_drop() {
    let fixture = Fixture::new();
    let primary = fixture.root.join(DATABASE_NAME);
    let moved = fixture.root.join("moved.db");
    fs::write(&primary, b"synthetic established primary").expect("synthetic existing primary");
    let lease =
        WindowsDirectoryLease::prepare(&fixture.root).expect("legacy existing primary lease");
    assert!(fs::rename(&primary, &moved).is_err());
    assert!(fs::remove_file(&primary).is_err());
    lease
        .revalidate()
        .expect("metadata probes can inspect the retained exclusive control");
    assert_eq!(
        fs::read(&primary).expect("unchanged synthetic primary"),
        b"synthetic established primary"
    );
    drop(lease);
    fs::rename(&primary, &moved).expect("released primary name can move");
    assert_eq!(
        fs::read(&moved).expect("preserved released primary"),
        b"synthetic established primary"
    );
}

#[cfg(windows)]
#[test]
fn legacy_database_creation_collision_retains_the_existing_file() {
    let fixture = Fixture::new();
    let primary = fixture.root.join(DATABASE_NAME);
    let moved = fixture.root.join("moved.db");
    fs::write(&primary, b"synthetic competing primary").expect("synthetic competing primary");
    let file = native::create_database(&primary).expect("legacy creation adopts existing primary");
    assert!(fs::rename(&primary, &moved).is_err());
    assert!(fs::remove_file(&primary).is_err());
    assert_eq!(
        fs::read(&primary).expect("preserved competing primary"),
        b"synthetic competing primary"
    );
    drop(file);
    fs::rename(&primary, &moved).expect("released competing primary can move");
}

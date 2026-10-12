//! Exercises confirmed recovery and each interrupted move with disposable synthetic stores.

use atlas_core::{Activation, TabKind, TabMetadata, TabModel};
use atlas_store::{BackupOutcome, Store};

use super::*;
use crate::storage::{WindowsDirectoryLease, tests::Fixture};

const DAMAGED: &[u8] = b"synthetic damaged SQLite primary";
const SIDECAR: &[u8] = b"synthetic sidecar bytes preserved by recovery";

fn seeded(fixture: &Fixture, count: u64) -> Vec<(BackupMetadata, StoredSnapshot, PathBuf)> {
    let lease = Arc::new(
        WindowsDirectoryLease::prepare(&fixture.root).expect("prepare synthetic location"),
    );
    let archive = lease.backup_archive().expect("native backup archive");
    let store = Store::open(lease.clone()).expect("open synthetic store");
    let mut model = TabModel::new();
    let mut backups = Vec::new();
    for number in 1..=count {
        model
            .open(
                TabKind::Web,
                TabMetadata {
                    title: format!("Synthetic recovery page {number}"),
                    address: format!("https://recovery-{number}.test/"),
                    favicon: None,
                    badge: None,
                },
                Activation::Foreground,
            )
            .expect("open synthetic tab");
        let revision = store
            .save(number - 1, model.snapshot())
            .expect("save synthetic state");
        let BackupOutcome::Created(metadata) = store
            .backup_if_due(number as i64 * 86_400, true)
            .expect("create synthetic backup")
        else {
            panic!("synthetic backup must be due");
        };
        backups.push((
            metadata,
            StoredSnapshot {
                revision,
                snapshot: model.snapshot(),
            },
            PathBuf::new(),
        ));
    }
    store.shutdown().expect("close synthetic store");
    for entry in archive.candidates().expect("list synthetic backups") {
        let backup = backups
            .iter_mut()
            .find(|backup| backup.0.id == entry.metadata.id)
            .expect("recognized synthetic backup");
        backup.2 = entry.file.database_path().to_owned();
    }
    backups
}

fn damage(fixture: &Fixture, sidecars: bool) {
    fs::write(fixture.root.join(DATABASE_NAME), DAMAGED).expect("damage only synthetic database");
    if sidecars {
        for name in SIDECAR_NAMES {
            fs::write(fixture.root.join(name), SIDECAR).expect("write synthetic sidecar");
        }
    }
}

fn restored(fixture: &Fixture, expected: &StoredSnapshot) {
    assert!(!fixture.root.join(RECOVERY_INTENT_NAME).exists());
    let lease =
        WindowsDirectoryLease::prepare(&fixture.root).expect("recovered location opens normally");
    let store = Store::open(Arc::new(lease)).expect("recovered SQLite opens");
    assert_eq!(
        store.load().expect("recovered synthetic snapshot"),
        *expected
    );
    store.shutdown().expect("close recovered synthetic store");
}

#[test]
fn newest_valid_offer_preserves_primary_and_sidecars_until_confirmation() {
    let fixture = Fixture::new();
    let backups = seeded(&fixture, 2);
    damage(&fixture, true);
    let offer =
        offer_recovery(&fixture.root, &DeletionLog::default()).expect("damaged primary gets offer");
    assert_eq!(offer.metadata(), backups[1].0);
    assert_eq!(
        fs::read(fixture.root.join(DATABASE_NAME)).expect("read synthetic primary"),
        DAMAGED
    );
    assert!(!fixture.root.join(RECOVERY_INTENT_NAME).exists());
    assert!(fs::write(&backups[1].2, b"replacement attempt").is_err());
    let result = offer.confirm().expect("confirmed recovery completes");
    assert_eq!(result.preserved_files, 4);
    restored(&fixture, &backups[1].1);
    let bundle = recovery_bundle(&fixture);
    assert_eq!(
        fs::read(bundle.join(DATABASE_NAME)).expect("preserved synthetic primary"),
        DAMAGED
    );
    for name in SIDECAR_NAMES {
        assert_eq!(
            fs::read(bundle.join(name)).expect("preserved synthetic sidecar"),
            SIDECAR
        );
    }
    assert!(backups.iter().all(|backup| backup.2.exists()));
}

#[test]
fn corrupt_newest_candidate_falls_back_but_foreign_primary_never_does() {
    let fixture = Fixture::new();
    let backups = seeded(&fixture, 2);
    fs::write(&backups[1].2, DAMAGED).expect("damage synthetic newest backup");
    damage(&fixture, false);
    let offer = offer_recovery(&fixture.root, &DeletionLog::default())
        .expect("offer older validated backup");
    assert_eq!(offer.metadata(), backups[0].0);
    offer.confirm().expect("restore older selected candidate");
    restored(&fixture, &backups[0].1);

    // A recognizable foreign SQLite file is refused, not classified as an Atlas corruption.
    let path = fixture.root.join(DATABASE_NAME);
    let mut foreign = fs::read(&path).expect("read synthetic SQLite bytes");
    foreign[68..72].copy_from_slice(&0x5359_4e54_u32.to_be_bytes());
    fs::write(&path, &foreign).expect("set synthetic foreign application ID");
    assert!(matches!(
        offer_recovery(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::Store(StoreError::ForeignDatabase))
    ));
    assert_eq!(
        fs::read(&path).expect("foreign primary remains unchanged"),
        foreign
    );
}

/// Marks a synthetic backup as written by a newer Atlas (schema version 3, one past current).
fn mark_newer_schema(path: &Path) {
    let mut bytes = fs::read(path).expect("read synthetic backup");
    bytes[60..64].copy_from_slice(&3u32.to_be_bytes());
    fs::write(path, &bytes).expect("mark synthetic backup as newer");
}

/// Rewrites a synthetic backup into the version 1 schema that predates saved folders.
fn downgrade_to_flat_schema(path: &Path) {
    let connection = rusqlite::Connection::open(path).expect("open synthetic backup");
    connection
        .execute_batch(
            "DROP TABLE saved_layout; DROP TABLE saved_folder_state; DROP TABLE saved_folders; PRAGMA user_version = 1;",
        )
        .expect("downgrade synthetic backup");
    assert!(connection.close().is_ok(), "close synthetic backup");
}

#[test]
fn newer_schema_backups_are_skipped_reported_and_left_unchanged() {
    let fixture = Fixture::new();
    let backups = seeded(&fixture, 2);
    mark_newer_schema(&backups[1].2);
    let newest = fs::read(&backups[1].2).expect("read newer synthetic backup");
    damage(&fixture, false);

    // The older usable backup is still offered instead of refusing every candidate.
    let offer =
        offer_recovery(&fixture.root, &DeletionLog::default()).expect("older backup is offered");
    assert_eq!(offer.metadata(), backups[0].0);
    drop(offer);
    assert_eq!(
        fs::read(&backups[1].2).expect("reread newer synthetic backup"),
        newest
    );

    // With only newer backups left, recovery says so rather than claiming none exist.
    mark_newer_schema(&backups[0].2);
    assert!(matches!(
        offer_recovery(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::OnlyNewerSchemaBackups { skipped: 2 })
    ));
}

#[test]
fn flat_schema_backup_restores_and_the_next_open_migrates_it() {
    let fixture = Fixture::new();
    let backups = seeded(&fixture, 1);
    downgrade_to_flat_schema(&backups[0].2);
    damage(&fixture, false);
    let offer =
        offer_recovery(&fixture.root, &DeletionLog::default()).expect("flat backup is offered");
    assert_eq!(offer.metadata(), backups[0].0);
    offer.confirm().expect("flat backup is restored");

    // Opening the store migrates the restored file and keeps its records.
    restored(&fixture, &backups[0].1);
    let connection = rusqlite::Connection::open(fixture.root.join(DATABASE_NAME))
        .expect("open restored synthetic database");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read restored schema version");
    assert_eq!(version, 2);
    assert!(
        connection.close().is_ok(),
        "close restored synthetic database"
    );
}

#[test]
fn healthy_primary_and_nonzero_or_pending_deletions_are_refused() {
    let fixture = Fixture::new();
    seeded(&fixture, 1);
    assert!(matches!(
        offer_recovery(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::HealthyPrimary)
    ));
    let mut deletions = DeletionLog::default();
    let record = deletions
        .request(
            atlas_store::deletion::DeletionCategory::History,
            atlas_store::deletion::DeletionRange::new(10, 20).expect("synthetic interval"),
        )
        .expect("synthetic request");
    deletions.accept(record).expect("accept synthetic request");
    assert!(matches!(
        offer_recovery(&fixture.root, &deletions),
        Err(RecoveryError::DeletionReplayRequired)
    ));
    deletions
        .accept(deletions.completion().expect("synthetic completion"))
        .expect("accept synthetic completion");
    assert!(matches!(
        offer_recovery(&fixture.root, &deletions),
        Err(RecoveryError::DeletionReplayRequired)
    ));
}

#[test]
fn healthy_header_with_a_hot_journal_is_not_misclassified_as_corrupt() {
    let fixture = Fixture::new();
    seeded(&fixture, 1);
    let primary = fixture.root.join(DATABASE_NAME);
    let before = fs::read(&primary).expect("read synthetic healthy database");
    fs::write(fixture.root.join("atlas.db-journal"), SIDECAR)
        .expect("write synthetic hot-journal trigger");
    assert!(matches!(
        offer_recovery(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::Store(StoreError::Database {
            code: Some(776),
            ..
        }))
    ));
    assert_eq!(
        fs::read(&primary).expect("healthy primary is unchanged"),
        before
    );
    assert_eq!(
        fs::read(fixture.root.join("atlas.db-journal")).expect("journal is unchanged"),
        SIDECAR
    );
    assert!(!fixture.root.join(RECOVERY_INTENT_NAME).exists());
}

#[test]
fn every_interrupted_stage_requires_a_new_offer_and_can_finish_the_same_source() {
    // Stop after intent, each of four preserved files, and the verified installation.
    for completed_moves in 0..=5 {
        let fixture = Fixture::new();
        let backups = seeded(&fixture, 1);
        damage(&fixture, true);
        let offer = offer_recovery(&fixture.root, &DeletionLog::default())
            .expect("initial damaged-primary offer");
        interrupt(offer, completed_moves);
        assert_eq!(
            WindowsDirectoryLease::prepare(&fixture.root)
                .expect_err("unfinished intent blocks normal opens"),
            LocationError::RecoveryPending
        );
        let renewed = offer_recovery(&fixture.root, &DeletionLog::default())
            .expect("known interruption produces renewed offer");
        assert_eq!(renewed.metadata(), backups[0].0);
        renewed
            .confirm()
            .expect("renewed confirmation finishes exact source");
        restored(&fixture, &backups[0].1);
        let preserved = recovery_bundle(&fixture);
        assert_eq!(
            fs::read(preserved.join(DATABASE_NAME)).expect("original primary preserved"),
            DAMAGED
        );
        for name in SIDECAR_NAMES {
            assert_eq!(
                fs::read(preserved.join(name)).expect("original sidecar preserved"),
                SIDECAR
            );
        }
        assert!(backups[0].2.exists());
    }
}

#[test]
fn guarded_startup_resumes_the_exact_pending_source_before_normal_open() {
    for completed_moves in [0, 4, 5] {
        let fixture = Fixture::new();
        let backups = seeded(&fixture, 1);
        damage(&fixture, true);
        let offer = offer_recovery(&fixture.root, &DeletionLog::default())
            .expect("initial synthetic offer");
        interrupt(offer, completed_moves);
        let crate::StartupStorage::Recovery(renewed) =
            crate::prepare_startup(&fixture.root, &DeletionLog::default())
                .expect("guarded startup must reconcile pending intent")
        else {
            panic!("pending recovery must require confirmation even after installation");
        };
        assert_eq!(renewed.metadata(), backups[0].0);
        renewed
            .confirm()
            .expect("confirm the same synthetic source");
        restored(&fixture, &backups[0].1);
    }
}

fn interrupt(offer: RecoveryOffer, completed_moves: usize) {
    let bundle = RecoveryBundle::create(&offer.prepared).expect("create synthetic recovery bundle");
    let stage = bundle
        .copy_source(&offer.source.file)
        .expect("copy synthetic candidate");
    validate_expected(&offer.prepared, &stage, &offer.expected)
        .expect("validate staged synthetic candidate");
    let intent = resume::Intent::new(
        &bundle,
        offer.backup.metadata,
        offer.source.identity,
        stage.identity,
        &offer.primary,
    )
    .write(&offer.prepared)
    .expect("flush synthetic intent");
    for (index, (name, file)) in bind_movable(offer.primary)
        .expect("bind synthetic originals")
        .into_iter()
        .enumerate()
    {
        if index < completed_moves {
            native::rename_no_replace(
                &file,
                &bundle.entry.path.join(name),
                LocationOperation::PreserveDatabase,
            )
            .expect("preserve synthetic original");
            file.sync_all().expect("flush synthetic preserved file");
        }
    }
    if completed_moves == 5 {
        install_stage(&offer.prepared, stage, &offer.expected)
            .expect("install synthetic replacement");
    }
    drop(intent);
}

#[test]
fn interrupted_recovery_refuses_changed_identity_without_replacing_it() {
    let fixture = Fixture::new();
    seeded(&fixture, 1);
    damage(&fixture, false);
    let offer =
        offer_recovery(&fixture.root, &DeletionLog::default()).expect("synthetic recovery offer");
    interrupt(offer, 0);
    let path = fixture.root.join(DATABASE_NAME);
    let original = fixture.root.join("synthetic-displaced.bin");
    fs::rename(&path, &original).expect("move synthetic original outside offered name");
    fs::write(&path, b"unexpected synthetic file").expect("create unexpected synthetic identity");
    assert!(matches!(
        offer_recovery(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::Location(LocationError::IdentityChanged))
    ));
    assert_eq!(
        fs::read(&path).expect("unexpected synthetic bytes"),
        b"unexpected synthetic file"
    );
    assert_eq!(
        fs::read(&original).expect("original synthetic bytes"),
        DAMAGED
    );
    assert!(fixture.root.join(RECOVERY_INTENT_NAME).exists());
}

#[test]
fn damaged_intent_never_creates_a_missing_primary() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join(RECOVERY_INTENT_NAME),
        b"synthetic incomplete intent",
    )
    .expect("create synthetic incomplete intent");
    assert!(matches!(
        offer_recovery(&fixture.root, &DeletionLog::default()),
        Err(RecoveryError::InvalidIntent)
    ));
    assert!(!fixture.root.join(DATABASE_NAME).exists());
}

#[test]
fn native_moves_do_not_replace_destinations_or_delete_nonempty_directories() {
    let fixture = Fixture::new();
    let source = fixture.root.join("synthetic-source");
    let target = fixture.root.join("synthetic-target");
    fs::write(&source, b"source").expect("write synthetic move source");
    fs::write(&target, b"target").expect("write synthetic move target");
    let file = native::open_movable(&source, LocationOperation::PreserveDatabase)
        .expect("open synthetic movable handle");
    assert!(
        native::rename_no_replace(&file, &target, LocationOperation::PreserveDatabase).is_err()
    );
    drop(file);
    assert_eq!(fs::read(source).expect("retained source"), b"source");
    assert_eq!(fs::read(target).expect("retained target"), b"target");
    let directory =
        GuardedEntry::directory(&fixture.root).expect("inspect synthetic nonempty directory");
    let identity = directory.identity;
    drop(directory);
    assert!(native::remove_checked_directory(&fixture.root, &identity).is_err());
    assert!(fixture.root.exists());
}

fn recovery_bundle(fixture: &Fixture) -> PathBuf {
    fs::read_dir(&fixture.root)
        .expect("enumerate synthetic recovery location")
        .map(|entry| entry.expect("synthetic entry").path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("recovery-"))
        })
        .expect("synthetic recovery bundle exists")
}

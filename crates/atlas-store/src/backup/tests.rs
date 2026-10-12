//! Injects publication failures into disposable snapshots without touching real storage.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;

use super::{BackupOutcome, BackupSchedule, validate_backup};
use crate::{
    BackupArchive, BackupEntry, BackupFile, BackupId, BackupMetadata, LocationError, ShutdownState,
    StoreError, schema,
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Archive {
    path: PathBuf,
    entries: Mutex<Vec<Option<BackupMetadata>>>,
    fault: AtomicU8,
    scans: AtomicU64,
}

struct File {
    archive: Arc<Archive>,
    id: BackupId,
    path: PathBuf,
}

impl File {
    fn new(archive: Arc<Archive>, id: BackupId) -> Self {
        let path = archive.path.join(format!("backup-{}.db", id.0));
        Self { archive, id, path }
    }
}

impl BackupArchive for Arc<Archive> {
    fn reserve(&self) -> Result<Box<dyn BackupFile>, LocationError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| LocationError::ControlPoisoned)?;
        let id = BackupId(entries.len() as u64 + 1);
        let file = File::new(Arc::clone(self), id);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file.path)
            .map_err(|_| LocationError::IdentityChanged)?;
        entries.push(None);
        Ok(Box::new(file))
    }

    fn candidates(&self) -> Result<Vec<BackupEntry>, LocationError> {
        self.scans.fetch_add(1, Ordering::SeqCst);
        if self.fault.load(Ordering::SeqCst) == 5 {
            return Err(LocationError::IdentityChanged);
        }
        let entries = self
            .entries
            .lock()
            .map_err(|_| LocationError::ControlPoisoned)?;
        Ok(entries
            .iter()
            .flatten()
            .map(|metadata| BackupEntry {
                metadata: *metadata,
                file: Box::new(File::new(Arc::clone(self), metadata.id)),
            })
            .collect())
    }
}

impl BackupFile for File {
    fn id(&self) -> BackupId {
        self.id
    }
    fn database_path(&self) -> &Path {
        &self.path
    }
    fn revalidate(&self) -> Result<(), LocationError> {
        // Fault one damages fresh output after VACUUM closes it, before validation opens it.
        if self.archive.fault.load(Ordering::SeqCst) == 1
            && fs::metadata(&self.path)
                .map_err(|_| LocationError::IdentityChanged)?
                .len()
                > 0
        {
            fs::write(&self.path, b"synthetic invalid SQLite output")
                .map_err(|_| LocationError::IdentityChanged)?;
            self.archive.fault.store(0, Ordering::SeqCst);
        }
        Ok(())
    }
    fn flush(&self) -> Result<(), LocationError> {
        if self.archive.fault.load(Ordering::SeqCst) == 2 {
            return Err(LocationError::IdentityChanged);
        }
        fs::OpenOptions::new()
            .write(true)
            .open(&self.path)
            .and_then(|file| file.sync_all())
            .map_err(|_| LocationError::IdentityChanged)
    }
    fn publish(&self, metadata: BackupMetadata) -> Result<(), LocationError> {
        // Fault three models marker bytes written successfully followed by a failed flush.
        self.archive
            .entries
            .lock()
            .map_err(|_| LocationError::ControlPoisoned)?[self.id.0 as usize - 1] = Some(metadata);
        if self.archive.fault.load(Ordering::SeqCst) == 3 {
            return Err(LocationError::IdentityChanged);
        }
        if self.archive.fault.load(Ordering::SeqCst) == 6 {
            fs::write(&self.path, b"synthetic damage during publication")
                .map_err(|_| LocationError::IdentityChanged)?;
        }
        Ok(())
    }
    fn remove(self: Box<Self>) -> Result<(), LocationError> {
        if self.archive.fault.load(Ordering::SeqCst) == 4 {
            return Err(LocationError::IdentityChanged);
        }
        fs::remove_file(&self.path).map_err(|_| LocationError::IdentityChanged)?;
        self.archive
            .entries
            .lock()
            .map_err(|_| LocationError::ControlPoisoned)?[self.id.0 as usize - 1] = None;
        Ok(())
    }
}

struct Fixture {
    connection: Connection,
    archive: Arc<Archive>,
}

impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test time follows epoch")
            .as_nanos();
        let number = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "atlas-backup-test-{}-{stamp}-{number}",
            std::process::id()
        ));
        assert!(
            fs::create_dir(&path).is_ok(),
            "create owned synthetic directory"
        );
        let mut connection = Connection::open(path.join("atlas.db"))
            .unwrap_or_else(|_| panic!("open synthetic database"));
        schema::configure_connection(&connection).expect("configure synthetic database");
        schema::prepare_writer(&mut connection, ShutdownState::Unclean)
            .expect("create synthetic schema");
        Self {
            connection,
            archive: Arc::new(Archive {
                path,
                entries: Mutex::new(Vec::new()),
                fault: AtomicU8::new(0),
                scans: AtomicU64::new(0),
            }),
        }
    }

    fn attempt(
        &mut self,
        schedule: &mut BackupSchedule,
        seconds: i64,
    ) -> Result<BackupOutcome, StoreError> {
        schedule.create_if_due(&mut self.connection, &self.archive, seconds, true)
    }

    fn finish(self) {
        assert!(self.connection.close().is_ok(), "close synthetic database");
        let count = self
            .archive
            .entries
            .lock()
            .expect("fixture entries accessible")
            .len();
        let mut names = vec!["atlas.db".to_owned()];
        names.extend((1..=count).map(|id| format!("backup-{id}.db")));
        // Only files with fixed names created by this fixture can be removed.
        for name in names {
            for suffix in ["", "-wal", "-shm", "-journal"] {
                match fs::remove_file(self.archive.path.join(format!("{name}{suffix}"))) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => panic!("remove owned synthetic file"),
                }
            }
        }
        assert!(
            fs::remove_dir(&self.archive.path).is_ok(),
            "remove empty synthetic directory"
        );
    }
}

#[test]
fn scheduling_is_idle_daily_clock_safe_and_cheap_until_restart() {
    let mut fixture = Fixture::new();
    let mut schedule = BackupSchedule::default();
    assert_eq!(
        schedule.create_if_due(&mut fixture.connection, &fixture.archive, 10, false),
        Ok(BackupOutcome::NotIdle)
    );
    assert_eq!(fixture.archive.scans.load(Ordering::SeqCst), 0);
    assert!(matches!(
        fixture.attempt(&mut schedule, 100_000),
        Ok(BackupOutcome::Created(_))
    ));
    let scans = fixture.archive.scans.load(Ordering::SeqCst);
    assert_eq!(
        fixture.attempt(&mut schedule, i64::MIN),
        Ok(BackupOutcome::NotDue)
    );
    assert_eq!(
        fixture.attempt(&mut schedule, 186_399),
        Ok(BackupOutcome::NotDue)
    );
    assert_eq!(fixture.archive.scans.load(Ordering::SeqCst), scans);
    let mut restarted = BackupSchedule::default();
    assert_eq!(
        fixture.attempt(&mut restarted, 186_399),
        Ok(BackupOutcome::NotDue)
    );
    assert!(fixture.archive.scans.load(Ordering::SeqCst) > scans);
    assert!(matches!(
        fixture.attempt(&mut restarted, 186_400),
        Ok(BackupOutcome::Created(_))
    ));
    fixture.finish();
}

#[test]
fn failed_validation_flush_and_uncertain_publication_preserve_output_and_retry() {
    for fault in [1, 2, 3] {
        let mut fixture = Fixture::new();
        let mut schedule = BackupSchedule::default();
        fixture
            .attempt(&mut schedule, 0)
            .expect("first backup succeeds");
        fixture.archive.fault.store(fault, Ordering::SeqCst);
        assert!(fixture.attempt(&mut schedule, 86_400).is_err());
        assert!(
            fixture.archive.path.join("backup-2.db").exists(),
            "uncertain output is preserved"
        );
        let first = File::new(Arc::clone(&fixture.archive), BackupId(1));
        fixture.archive.fault.store(0, Ordering::SeqCst);
        assert_eq!(
            validate_backup(&first)
                .expect("previous backup survives")
                .revision,
            0
        );
        assert!(matches!(
            fixture.attempt(&mut schedule, 86_400),
            Ok(BackupOutcome::Created(_))
        ));
        fixture.finish();
    }
}

#[test]
fn pruning_failure_keeps_new_snapshot_and_reports_the_failure() {
    let mut fixture = Fixture::new();
    let mut schedule = BackupSchedule::default();
    for day in 0..7 {
        fixture
            .attempt(&mut schedule, day * 86_400)
            .expect("backup succeeds");
    }
    fixture.archive.fault.store(4, Ordering::SeqCst);
    assert_eq!(
        fixture.attempt(&mut schedule, 7 * 86_400),
        Err(StoreError::Location(LocationError::IdentityChanged))
    );
    assert_eq!(
        fixture
            .archive
            .candidates()
            .expect("candidates remain")
            .len(),
        8
    );
    fixture.archive.fault.store(0, Ordering::SeqCst);
    fixture
        .attempt(&mut schedule, 7 * 86_400)
        .expect("failed attempt remains due");
    assert_eq!(
        fixture
            .archive
            .candidates()
            .expect("rotation completed")
            .len(),
        7
    );
    fixture.finish();
}

#[test]
fn corruption_during_publication_cannot_prune_previous_backups_or_advance_schedule() {
    let mut fixture = Fixture::new();
    let mut schedule = BackupSchedule::default();
    for day in 0..7 {
        fixture
            .attempt(&mut schedule, day * 86_400)
            .expect("backup succeeds");
    }
    fixture.archive.fault.store(6, Ordering::SeqCst);
    assert_eq!(
        fixture.attempt(&mut schedule, 7 * 86_400),
        Err(StoreError::BackupVerificationFailed)
    );
    for id in 1..=7 {
        assert!(
            fixture
                .archive
                .path
                .join(format!("backup-{id}.db"))
                .exists()
        );
    }
    fixture.archive.fault.store(0, Ordering::SeqCst);
    assert!(matches!(
        fixture.attempt(&mut schedule, 7 * 86_400),
        Ok(BackupOutcome::Created(_))
    ));
    fixture.finish();
}

#[test]
fn discovery_surfaces_io_and_unsupported_deletion_state_without_creating_output() {
    let mut fixture = Fixture::new();
    fixture.archive.fault.store(5, Ordering::SeqCst);
    assert_eq!(
        fixture.attempt(&mut BackupSchedule::default(), 0),
        Err(StoreError::Location(LocationError::IdentityChanged))
    );
    fixture.archive.fault.store(0, Ordering::SeqCst);
    fixture
        .attempt(&mut BackupSchedule::default(), 0)
        .expect("first backup succeeds");
    fixture
        .archive
        .entries
        .lock()
        .expect("fixture entries accessible")[0]
        .as_mut()
        .expect("published metadata")
        .deletion_generation = 1;
    assert_eq!(
        fixture.attempt(&mut BackupSchedule::default(), 86_400),
        Err(StoreError::UnsupportedDeletionGeneration)
    );
    assert_eq!(
        fixture
            .archive
            .entries
            .lock()
            .expect("fixture entries accessible")
            .len(),
        1
    );
    fixture.finish();
}

#[test]
fn future_schema_refusal_does_not_rewrite_the_backup() {
    let mut fixture = Fixture::new();
    fixture
        .attempt(&mut BackupSchedule::default(), 0)
        .expect("first backup succeeds");
    let file = File::new(Arc::clone(&fixture.archive), BackupId(1));
    let connection =
        Connection::open(file.database_path()).unwrap_or_else(|_| panic!("open synthetic backup"));
    connection
        .execute_batch("PRAGMA user_version = 3")
        .expect("create future schema marker");
    assert!(connection.close().is_ok(), "close synthetic connection");
    let original = fs::read(file.database_path()).expect("read synthetic bytes");
    assert_eq!(
        validate_backup(&file),
        Err(StoreError::FutureSchema { version: 3 })
    );
    assert_eq!(
        fs::read(file.database_path()).expect("read unchanged synthetic bytes"),
        original
    );
    fixture.finish();
}

//! Binds confirmed recovery to one checked backup and preserves displaced database files.

use std::{
    error::Error,
    fmt,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use syrinx_store::{
    BackupArchive, BackupEntry, BackupMetadata, DirectoryLease, LocationError, LocationOperation,
    StoreError, StoredSnapshot,
    deletion::{DeletionError, DeletionLog},
    replay_deletions, validate_backup, validate_database,
};

use super::{
    DATABASE_NAME, GuardedEntry, PreparedDirectory, SIDECAR_NAMES, backup::WindowsBackupArchive,
    native, normal_absolute_path, prepare_directory, session::RECOVERY_INTENT_NAME,
    sqlite_database_path,
};

static NEXT_RECOVERY: AtomicU64 = AtomicU64::new(0);

mod resume;

/// A dated, exclusive offer. Consuming it confirms this exact source and primary set.
pub struct RecoveryOffer {
    prepared: PreparedDirectory,
    backup: BackupEntry,
    source: GuardedEntry,
    // Boxed: a folder-bearing snapshot would otherwise dominate `StartupStorage`'s size.
    expected: Box<StoredSnapshot>,
    primary: Vec<PrimaryFile>,
    /// The journal the staged copy is replayed against before it may become live (D47).
    deletions: DeletionLog,
    continuation: Option<Box<resume::Continuation>>,
}

impl fmt::Debug for RecoveryOffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryOffer { location: [redacted], state: [redacted] }")
    }
}

/// Reports only structural recovery outcomes; displaced files remain local.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryOutcome {
    pub restored: BackupMetadata,
    pub preserved_files: usize,
}

/// No failure authorizes choosing another backup or discarding interrupted state.
#[derive(Debug)]
pub enum RecoveryError {
    Location(LocationError),
    Store(StoreError),
    NoValidBackup,
    /// Every usable-looking backup was written by a newer Syrinx; they are left untouched.
    OnlyNewerSchemaBackups {
        skipped: usize,
    },
    HealthyPrimary,
    /// The deletion journal cannot cover a backup's clears; no older copy is offered instead.
    Deletion(DeletionError),
    MetadataMismatch,
    ClockUnavailable,
    InvalidIntent,
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "recovery failed: {self:?}")
    }
}

impl Error for RecoveryError {}

impl From<LocationError> for RecoveryError {
    fn from(error: LocationError) -> Self {
        Self::Location(error)
    }
}

impl From<StoreError> for RecoveryError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

struct PrimaryFile {
    name: &'static str,
    entry: GuardedEntry,
}

/// Offers the newest valid snapshot only for an absent or demonstrably damaged primary.
/// `deletions` is the journal read beside the database; confirmation replays it (D47).
pub fn offer_recovery(
    directory: &Path,
    deletions: &DeletionLog,
) -> Result<RecoveryOffer, RecoveryError> {
    let prepared = prepare_directory(directory, Arc::new(native::WindowsProbe))?;
    offer_prepared(prepared, deletions)
}

pub(super) fn has_intent(prepared: &PreparedDirectory) -> Result<bool, LocationError> {
    Ok(native::optional_file(
        &prepared.path.join(RECOVERY_INTENT_NAME),
        LocationOperation::OpenRecoveryIntent,
    )?
    .is_some())
}

pub(super) fn offer_prepared(
    prepared: PreparedDirectory,
    deletions: &DeletionLog,
) -> Result<RecoveryOffer, RecoveryError> {
    if has_intent(&prepared)? {
        return resume::offer(prepared, deletions);
    }
    prepared.control.revalidate()?;
    let primary = capture_primary(&prepared)?;
    require_damaged_primary(&prepared, &primary)?;
    let archive = WindowsBackupArchive::new(prepared.clone());
    let mut candidates = archive.candidates()?;
    candidates.sort_by_key(|candidate| {
        std::cmp::Reverse((candidate.metadata.created_at, candidate.metadata.id))
    });

    // Hold the selected source against writes for the entire offer and confirmation.
    let mut newer_schema = 0;
    for backup in candidates {
        // A journal that lacks a backup's clears is damaged; refuse rather than pick an older copy.
        deletions
            .unapplied(backup.metadata.deletion_generation)
            .map_err(RecoveryError::Deletion)?;
        let path = normal_absolute_path(backup.file.database_path())?;
        let source = GuardedEntry::regular_file(
            path.clone(),
            native::open_readonly(&path, LocationOperation::ReadBackup)?,
        )?;
        match validate_backup(backup.file.as_ref()) {
            Ok(expected) => {
                if expected.revision != backup.metadata.revision {
                    continue;
                }
                return Ok(RecoveryOffer {
                    prepared,
                    backup,
                    source,
                    expected: Box::new(expected),
                    primary,
                    deletions: deletions.clone(),
                    continuation: None,
                });
            }
            Err(error) if damaged_candidate(&error) => continue,
            // A newer Syrinx's backup cannot be restored here; skip it rather than refuse all.
            Err(StoreError::FutureSchema { .. }) => newer_schema += 1,
            Err(error) => return Err(error.into()),
        }
    }
    if newer_schema > 0 {
        return Err(RecoveryError::OnlyNewerSchemaBackups {
            skipped: newer_schema,
        });
    }
    Err(RecoveryError::NoValidBackup)
}

impl RecoveryOffer {
    pub fn metadata(&self) -> BackupMetadata {
        self.backup.metadata
    }

    /// The caller must obtain the owner's confirmation before consuming the offer.
    pub fn confirm(mut self) -> Result<RecoveryOutcome, RecoveryError> {
        if let Some(continuation) = self.continuation.take() {
            return resume::confirm(self, *continuation);
        }
        self.prepared.revalidate()?;
        self.prepared.control.revalidate()?;
        self.backup.file.revalidate()?;
        self.source
            .revalidate(false, LocationOperation::ReadBackup)?;
        revalidate_primary(&self.prepared, &self.primary)?;
        require_damaged_primary(&self.prepared, &self.primary)?;

        // Copy into a fresh local bundle; the source backup is never moved or rewritten.
        let bundle = RecoveryBundle::create(&self.prepared)?;
        let stage = bundle.copy_source(&self.source.file)?;
        validate_expected(&self.prepared, &stage, &self.expected)?;

        // Cleared history is removed from the copy before the intent can expose it (D47).
        let stage = replay_stage(
            &self.prepared,
            &bundle,
            stage,
            &self.deletions,
            self.backup.metadata.deletion_generation,
        )?;
        validate_expected(&self.prepared, &stage, &self.expected)?;
        let intent = resume::Intent::new(
            &bundle,
            self.backup.metadata,
            self.source.identity,
            stage.identity,
            &self.primary,
        )
        .write(&self.prepared)?;

        // Bind all movable handles before changing any primary name.
        let files = bind_movable(self.primary)?;
        let preserved_files = files.len();
        for (name, file) in files {
            native::rename_no_replace(
                &file,
                &bundle.entry.path.join(name),
                LocationOperation::PreserveDatabase,
            )?;
            file.sync_all()
                .map_err(|error| native::io_error(LocationOperation::PreserveDatabase, error))?;
        }
        install_stage(&self.prepared, stage, &self.expected)?;

        // A surviving intent blocks normal startup whenever installation or verification fails.
        complete_intent(intent)?;
        Ok(RecoveryOutcome {
            restored: self.backup.metadata,
            preserved_files,
        })
    }
}

fn capture_primary(prepared: &PreparedDirectory) -> Result<Vec<PrimaryFile>, RecoveryError> {
    let mut primary = Vec::new();
    for name in std::iter::once(DATABASE_NAME).chain(SIDECAR_NAMES) {
        let path = prepared.path.join(name);
        let Some(probe) = native::optional_file(&path, LocationOperation::OpenDatabase)? else {
            continue;
        };
        super::check_entry(&native::entry_info(&probe)?, false)?;
        drop(probe);
        let file = native::open_readonly(&path, LocationOperation::OpenDatabase)?;
        primary.push(PrimaryFile {
            name,
            entry: GuardedEntry::regular_file(path, file)?,
        });
    }
    Ok(primary)
}

fn revalidate_primary(
    prepared: &PreparedDirectory,
    primary: &[PrimaryFile],
) -> Result<(), RecoveryError> {
    // A newly appeared or disappeared sidecar invalidates the exact offered primary set.
    for name in std::iter::once(DATABASE_NAME).chain(SIDECAR_NAMES) {
        let current =
            native::optional_file(&prepared.path.join(name), LocationOperation::OpenDatabase)?;
        match (primary.iter().find(|file| file.name == name), current) {
            (None, None) => {}
            (Some(file), Some(current)) => {
                let info = native::entry_info(&current)?;
                super::check_entry(&info, false)?;
                if info.identity != file.entry.identity {
                    return Err(LocationError::IdentityChanged.into());
                }
                file.entry
                    .revalidate(false, LocationOperation::OpenDatabase)?;
            }
            _ => return Err(LocationError::IdentityChanged.into()),
        }
    }
    Ok(())
}

fn require_damaged_primary(
    prepared: &PreparedDirectory,
    primary: &[PrimaryFile],
) -> Result<(), RecoveryError> {
    let Some(main) = primary.iter().find(|file| file.name == DATABASE_NAME) else {
        return Ok(());
    };
    // A hot journal can stop read-only SQLite before it reports an obviously broken main header.
    if broken_header(&main.entry.file)? {
        return Ok(());
    }
    let lease = ValidationLease::new(prepared, &main.entry);
    match validate_database(&lease) {
        Ok(_) => Err(RecoveryError::HealthyPrimary),
        Err(error) if damaged_data(&error) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn broken_header(file: &File) -> Result<bool, RecoveryError> {
    let length = file
        .metadata()
        .map_err(|error| native::io_error(LocationOperation::OpenDatabase, error))?
        .len();
    if length < 100 {
        return Ok(true);
    }
    let mut file = file;
    let mut header = [0; 16];
    file.rewind()
        .and_then(|_| file.read_exact(&mut header))
        .map_err(|error| native::io_error(LocationOperation::OpenDatabase, error))?;
    Ok(&header != b"SQLite format 3\0")
}

fn damaged_data(error: &StoreError) -> bool {
    match error {
        StoreError::IntegrityCheckFailed
        | StoreError::InvalidEncoding(_)
        | StoreError::InvalidSnapshot(_) => true,
        StoreError::Database {
            code: Some(code), ..
        } => matches!(code & 0xff, 11 | 26),
        _ => false,
    }
}

fn damaged_candidate(error: &StoreError) -> bool {
    damaged_data(error)
        || matches!(
            error,
            StoreError::ForeignDatabase | StoreError::SchemaMismatch
        )
}

fn complete_intent(intent: GuardedEntry) -> Result<(), RecoveryError> {
    let identity = intent.identity;
    let path = intent.path.clone();
    drop(intent);
    native::remove_checked_file(&path, &identity)?;
    Ok(())
}

struct ValidationLease<'a> {
    prepared: &'a PreparedDirectory,
    entry: &'a GuardedEntry,
    sqlite_path: PathBuf,
}

impl<'a> ValidationLease<'a> {
    fn new(prepared: &'a PreparedDirectory, entry: &'a GuardedEntry) -> Self {
        Self {
            prepared,
            entry,
            sqlite_path: sqlite_database_path(&entry.path),
        }
    }
}

impl DirectoryLease for ValidationLease<'_> {
    fn database_path(&self) -> &Path {
        &self.sqlite_path
    }
    fn revalidate(&self) -> Result<(), LocationError> {
        self.prepared.revalidate()?;
        self.entry
            .revalidate(false, LocationOperation::OpenDatabase)
    }
}

struct RecoveryBundle {
    entry: GuardedEntry,
    stamp: u128,
    sequence: u64,
}

impl RecoveryBundle {
    fn create(prepared: &PreparedDirectory) -> Result<Self, RecoveryError> {
        prepared.revalidate()?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| RecoveryError::ClockUnavailable)?
            .as_nanos();
        let sequence = NEXT_RECOVERY
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| LocationError::BackupIdentityExhausted)?;
        let path = prepared
            .path
            .join(format!("recovery-{stamp:032x}-{sequence:016x}"));
        fs::create_dir(&path)
            .map_err(|error| native::io_error(LocationOperation::CreateBackup, error))?;
        Ok(Self {
            entry: GuardedEntry::directory(&path)?,
            stamp,
            sequence,
        })
    }

    fn copy_source(&self, source: &File) -> Result<GuardedEntry, RecoveryError> {
        self.entry
            .revalidate(true, LocationOperation::OpenDirectory)?;
        let path = self.entry.path.join("replacement.db");
        let mut output = native::create_new_file(&path, LocationOperation::CreateBackup, false)?;
        let mut input = source;
        input
            .seek(SeekFrom::Start(0))
            .map_err(|error| native::io_error(LocationOperation::ReadBackup, error))?;
        io::copy(&mut input, &mut output)
            .map_err(|error| native::io_error(LocationOperation::CreateBackup, error))?;
        output
            .sync_all()
            .map_err(|error| native::io_error(LocationOperation::FlushBackup, error))?;
        let identity = native::entry_info(&output)?.identity;
        drop(output);
        let entry = GuardedEntry::regular_file(
            path.clone(),
            native::open_readonly(&path, LocationOperation::ReadBackup)?,
        )?;
        if entry.identity != identity {
            return Err(LocationError::IdentityChanged.into());
        }
        Ok(entry)
    }
}

/// Replays the journal on the staged copy and rebinds the same file afterwards.
fn replay_stage(
    prepared: &PreparedDirectory,
    bundle: &RecoveryBundle,
    stage: GuardedEntry,
    deletions: &DeletionLog,
    recorded_generation: u64,
) -> Result<GuardedEntry, RecoveryError> {
    let path = stage.path.clone();
    let identity = stage.identity;
    // SQLite needs write sharing, which the retained read-only guard excludes.
    drop(stage);
    let lease = StageLease {
        prepared,
        bundle: &bundle.entry,
        named: path.clone(),
        sqlite_path: sqlite_database_path(&path),
        identity,
    };
    replay_deletions(&lease, deletions, recorded_generation)?;
    let entry = GuardedEntry::regular_file(
        path.clone(),
        native::open_readonly(&path, LocationOperation::ReadBackup)?,
    )?;
    if entry.identity != identity {
        return Err(LocationError::IdentityChanged.into());
    }
    // The bundle must again hold only its expected names before an intent refers to it.
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = path.clone().into_os_string();
        sidecar.push(suffix);
        if native::optional_file(Path::new(&sidecar), LocationOperation::ReadBackup)?.is_some() {
            return Err(LocationError::IdentityChanged.into());
        }
    }
    Ok(entry)
}

/// Lets SQLite write the staged copy while its directory and identity stay checked.
struct StageLease<'a> {
    prepared: &'a PreparedDirectory,
    bundle: &'a GuardedEntry,
    named: PathBuf,
    sqlite_path: PathBuf,
    identity: super::FileIdentity,
}

impl DirectoryLease for StageLease<'_> {
    fn database_path(&self) -> &Path {
        &self.sqlite_path
    }

    fn revalidate(&self) -> Result<(), LocationError> {
        self.prepared.revalidate()?;
        self.bundle
            .revalidate(true, LocationOperation::OpenDirectory)?;
        // A metadata-only probe does not conflict with SQLite's own open handle.
        let named = native::optional_file(&self.named, LocationOperation::ReadBackup)?
            .ok_or(LocationError::IdentityChanged)?;
        let info = native::entry_info(&named)?;
        super::check_entry(&info, false)?;
        if info.identity != self.identity {
            return Err(LocationError::IdentityChanged);
        }
        Ok(())
    }
}

fn validate_expected(
    prepared: &PreparedDirectory,
    stage: &GuardedEntry,
    expected: &StoredSnapshot,
) -> Result<(), RecoveryError> {
    if validate_database(&ValidationLease::new(prepared, stage))? != *expected {
        return Err(RecoveryError::MetadataMismatch);
    }
    Ok(())
}

fn bind_movable(primary: Vec<PrimaryFile>) -> Result<Vec<(&'static str, File)>, RecoveryError> {
    let mut files = Vec::with_capacity(primary.len());
    for primary in primary {
        let path = primary.entry.path.clone();
        let identity = primary.entry.identity;
        drop(primary.entry);
        let file = native::open_movable(&path, LocationOperation::PreserveDatabase)?;
        if native::entry_info(&file)?.identity != identity {
            return Err(LocationError::IdentityChanged.into());
        }
        files.push((primary.name, file));
    }
    Ok(files)
}

fn install_stage(
    prepared: &PreparedDirectory,
    stage: GuardedEntry,
    expected: &StoredSnapshot,
) -> Result<(), RecoveryError> {
    require_absent(
        prepared,
        std::iter::once(DATABASE_NAME).chain(SIDECAR_NAMES),
    )?;
    let path = stage.path.clone();
    let identity = stage.identity;
    drop(stage);
    let file = native::open_movable(&path, LocationOperation::InstallDatabase)?;
    if native::entry_info(&file)?.identity != identity {
        return Err(LocationError::IdentityChanged.into());
    }
    let destination = prepared.path.join(DATABASE_NAME);
    native::rename_no_replace(&file, &destination, LocationOperation::InstallDatabase)?;
    file.sync_all()
        .map_err(|error| native::io_error(LocationOperation::InstallDatabase, error))?;
    drop(file);
    let installed = GuardedEntry::regular_file(
        destination.clone(),
        native::open_readonly(&destination, LocationOperation::OpenDatabase)?,
    )?;
    if installed.identity != identity {
        return Err(LocationError::IdentityChanged.into());
    }
    validate_expected(prepared, &installed, expected)?;
    require_absent(prepared, SIDECAR_NAMES)
}

fn require_absent(
    prepared: &PreparedDirectory,
    names: impl IntoIterator<Item = &'static str>,
) -> Result<(), RecoveryError> {
    for name in names {
        if native::optional_file(
            &prepared.path.join(name),
            LocationOperation::InstallDatabase,
        )?
        .is_some()
        {
            return Err(LocationError::IdentityChanged.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

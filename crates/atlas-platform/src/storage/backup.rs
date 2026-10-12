//! Keeps incomplete snapshots separate from published, identity-checked backup bundles.

#[cfg(test)]
mod tests;

use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use atlas_store::{
    BackupArchive, BackupEntry, BackupFile, BackupId, BackupMetadata, LocationError,
    LocationOperation,
};

use super::{
    DATABASE_NAME, FileIdentity, GuardedEntry, PreparedDirectory, native, refuse_root_identities,
    sqlite_database_path, sync_root_identities,
};

const ARCHIVE_NAME: &str = "backups";
const COMPLETE_NAME: &str = "complete";
const MARKER_MAGIC: &[u8; 8] = b"ATLBKP01";
const PAYLOAD_SIZE: usize = 40;
const MARKER_SIZE: usize = PAYLOAD_SIZE * 2;

pub(super) struct WindowsBackupArchive {
    prepared: PreparedDirectory,
}

struct ArchiveGuard {
    prepared: PreparedDirectory,
    directory: GuardedEntry,
    sync_roots: Vec<FileIdentity>,
}

struct WindowsBackupFile {
    id: BackupId,
    sqlite_path: PathBuf,
    archive: Arc<ArchiveGuard>,
    directory: GuardedEntry,
    fresh: bool,
    state: Mutex<BundleState>,
}

struct BundleState {
    database: Option<GuardedEntry>,
    marker: Option<GuardedEntry>,
}

impl WindowsBackupArchive {
    pub(super) fn new(prepared: PreparedDirectory) -> Self {
        Self { prepared }
    }

    fn open_archive(&self, create: bool) -> Result<Option<Arc<ArchiveGuard>>, LocationError> {
        // Enumerate once at the maintenance boundary, then retain this checked identity set.
        self.prepared.revalidate_entries()?;
        let sync_roots = sync_root_identities(self.prepared.probe.as_ref())?;
        refuse_root_identities(&self.prepared.directories, &sync_roots)?;
        let path = self.prepared.path.join(ARCHIVE_NAME);
        let file = match native::optional_directory(&path)? {
            Some(file) => file,
            None if create => {
                super::create_directory(&path)?;
                native::open_retained_directory(&path)?
            }
            None => return Ok(None),
        };
        let archive = Arc::new(ArchiveGuard {
            prepared: self.prepared.clone(),
            directory: GuardedEntry::from_directory_file(&path, file)?,
            sync_roots,
        });
        archive.revalidate()?;
        Ok(Some(archive))
    }
}

impl BackupArchive for WindowsBackupArchive {
    fn reserve(&self) -> Result<Box<dyn BackupFile>, LocationError> {
        // Create one uniquely named bundle; collisions never adopt an existing directory.
        let archive = self
            .open_archive(true)?
            .ok_or(LocationError::InvalidBackup)?;
        for _ in 0..16 {
            let id = next_id()?;
            let path = archive.directory.path.join(bundle_name(id));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Box::new(WindowsBackupFile::reserve(archive, id, path)?)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(native::io_error(LocationOperation::CreateBackup, error)),
            }
        }
        Err(LocationError::BackupIdentityExhausted)
    }

    fn candidates(&self) -> Result<Vec<BackupEntry>, LocationError> {
        let Some(archive) = self.open_archive(false)? else {
            return Ok(Vec::new());
        };
        let entries = fs::read_dir(&archive.directory.path)
            .map_err(|error| native::io_error(LocationOperation::EnumerateBackups, error))?;
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry
                .map_err(|error| native::io_error(LocationOperation::EnumerateBackups, error))?;
            let Some(id) = entry.file_name().to_str().and_then(parse_bundle_name) else {
                continue;
            };
            // Incomplete and malformed bundles remain on disk, never becoming deletion targets.
            match WindowsBackupFile::candidate(Arc::clone(&archive), id, entry.path()) {
                Ok(Some(candidate)) => candidates.push(candidate),
                Ok(None) | Err(LocationError::InvalidBackup) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(candidates)
    }
}

impl ArchiveGuard {
    fn revalidate(&self) -> Result<(), LocationError> {
        // Registrations after this operation began remain the documented guard limitation.
        self.prepared.revalidate_entries()?;
        self.directory
            .revalidate(true, LocationOperation::OpenDirectory)?;
        refuse_root_identities(std::slice::from_ref(&self.directory), &self.sync_roots)
    }
}

impl WindowsBackupFile {
    fn reserve(
        archive: Arc<ArchiveGuard>,
        id: BackupId,
        path: PathBuf,
    ) -> Result<Self, LocationError> {
        let directory = GuardedEntry::directory(&path)?;
        refuse_root_identities(std::slice::from_ref(&directory), &archive.sync_roots)?;
        let database_path = path.join(DATABASE_NAME);
        let database = GuardedEntry::regular_file(
            database_path.clone(),
            native::create_new_file(&database_path, LocationOperation::CreateBackup, true)?,
        )?;
        Ok(Self {
            id,
            sqlite_path: sqlite_database_path(&database_path),
            archive,
            directory,
            fresh: true,
            state: Mutex::new(BundleState {
                database: Some(database),
                marker: None,
            }),
        })
    }

    fn candidate(
        archive: Arc<ArchiveGuard>,
        id: BackupId,
        path: PathBuf,
    ) -> Result<Option<BackupEntry>, LocationError> {
        let directory = GuardedEntry::directory(&path)?;
        let marker_path = path.join(COMPLETE_NAME);
        let Some(inspected) = native::optional_file(&marker_path, LocationOperation::ReadBackup)?
        else {
            return Ok(None);
        };
        let inspected = GuardedEntry::regular_file(marker_path.clone(), inspected)?;
        let marker = GuardedEntry::regular_file(
            marker_path.clone(),
            native::open_readonly(&marker_path, LocationOperation::ReadBackup)?,
        )?;
        if inspected.identity != marker.identity {
            return Err(LocationError::IdentityChanged);
        }
        let Some(metadata) = read_marker(&marker, id)? else {
            return Ok(None);
        };
        let database_path = path.join(DATABASE_NAME);
        let Some(inspected_database) =
            native::optional_file(&database_path, LocationOperation::ReadBackup)?
        else {
            return Ok(None);
        };
        let inspected_database =
            GuardedEntry::regular_file(database_path.clone(), inspected_database)?;
        let database = GuardedEntry::regular_file(
            database_path.clone(),
            native::open_readonly(&database_path, LocationOperation::ReadBackup)?,
        )?;
        if inspected_database.identity != database.identity {
            return Err(LocationError::IdentityChanged);
        }
        let file = Self {
            id,
            sqlite_path: sqlite_database_path(&database_path),
            archive,
            directory,
            fresh: false,
            state: Mutex::new(BundleState {
                database: Some(database),
                marker: Some(marker),
            }),
        };
        file.revalidate()?;
        Ok(Some(BackupEntry {
            metadata,
            file: Box::new(file),
        }))
    }

    fn revalidate_directory(&self) -> Result<(), LocationError> {
        self.archive.revalidate()?;
        self.directory
            .revalidate(true, LocationOperation::OpenDirectory)?;
        refuse_root_identities(
            std::slice::from_ref(&self.directory),
            &self.archive.sync_roots,
        )?;
        check_bundle_names(&self.directory.path)
    }
}

impl BackupFile for WindowsBackupFile {
    fn id(&self) -> BackupId {
        self.id
    }

    fn database_path(&self) -> &Path {
        &self.sqlite_path
    }

    fn revalidate(&self) -> Result<(), LocationError> {
        self.revalidate_directory()?;
        let state = self
            .state
            .lock()
            .map_err(|_| LocationError::BackupStatePoisoned)?;
        state
            .database
            .as_ref()
            .ok_or(LocationError::InvalidBackup)?
            .revalidate(false, LocationOperation::ReadBackup)?;
        if let Some(marker) = &state.marker {
            marker.revalidate(false, LocationOperation::ReadBackup)?;
        } else if native::optional_file(
            &self.directory.path.join(COMPLETE_NAME),
            LocationOperation::ReadBackup,
        )?
        .is_some()
        {
            return Err(LocationError::InvalidBackup);
        }
        Ok(())
    }

    fn flush(&self) -> Result<(), LocationError> {
        self.revalidate()?;
        let state = self
            .state
            .lock()
            .map_err(|_| LocationError::BackupStatePoisoned)?;
        if !self.fresh || state.marker.is_some() {
            return Err(LocationError::InvalidBackup);
        }
        state
            .database
            .as_ref()
            .ok_or(LocationError::InvalidBackup)?
            .file
            .sync_all()
            .map_err(|error| native::io_error(LocationOperation::FlushBackup, error))
    }

    fn publish(&self, metadata: BackupMetadata) -> Result<(), LocationError> {
        if metadata.id != self.id || metadata.deletion_generation != 0 {
            return Err(LocationError::InvalidBackup);
        }
        self.revalidate()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| LocationError::BackupStatePoisoned)?;
        if !self.fresh || state.marker.is_some() {
            return Err(LocationError::InvalidBackup);
        }
        // Release write sharing only after SQLite has closed, then bind the same file read-only.
        state
            .database
            .as_ref()
            .ok_or(LocationError::InvalidBackup)?
            .file
            .sync_all()
            .map_err(|error| native::io_error(LocationOperation::FlushBackup, error))?;
        freeze_entry(&mut state.database)?;
        let path = self.directory.path.join(COMPLETE_NAME);
        let marker = GuardedEntry::regular_file(
            path.clone(),
            native::create_new_file(&path, LocationOperation::PublishBackup, false)?,
        )?;
        state.marker = Some(marker);
        let mut file = &state
            .marker
            .as_ref()
            .ok_or(LocationError::InvalidBackup)?
            .file;
        file.write_all(&encode_marker(metadata))
            .map_err(|error| native::io_error(LocationOperation::PublishBackup, error))?;
        file.sync_all()
            .map_err(|error| native::io_error(LocationOperation::FlushBackup, error))?;
        freeze_entry(&mut state.marker)?;
        Ok(())
    }

    fn remove(self: Box<Self>) -> Result<(), LocationError> {
        self.revalidate()?;
        let Self {
            state,
            directory,
            archive,
            ..
        } = *self;
        let state = state
            .into_inner()
            .map_err(|_| LocationError::BackupStatePoisoned)?;
        // Invalidate eligibility first, then remove only the exact retained file identities.
        if let Some(marker) = state.marker {
            let (path, identity) = (marker.path.clone(), marker.identity);
            drop(marker);
            native::remove_checked_file(&path, &identity)?;
        }
        let database = state.database.ok_or(LocationError::InvalidBackup)?;
        let (path, identity) = (database.path.clone(), database.identity);
        drop(database);
        native::remove_checked_file(&path, &identity)?;
        let (path, identity) = (directory.path.clone(), directory.identity);
        drop(directory);
        let result = native::remove_checked_directory(&path, &identity);
        drop(archive);
        result
    }
}

fn freeze_entry(entry: &mut Option<GuardedEntry>) -> Result<(), LocationError> {
    let writable = entry.take().ok_or(LocationError::InvalidBackup)?;
    let (path, identity) = (writable.path.clone(), writable.identity);
    drop(writable);
    let readonly = GuardedEntry::regular_file(
        path.clone(),
        native::open_readonly(&path, LocationOperation::ReadBackup)?,
    )?;
    if readonly.identity != identity {
        return Err(LocationError::IdentityChanged);
    }
    *entry = Some(readonly);
    Ok(())
}

fn check_bundle_names(path: &Path) -> Result<(), LocationError> {
    for entry in fs::read_dir(path)
        .map_err(|error| native::io_error(LocationOperation::ReadBackup, error))?
    {
        let entry =
            entry.map_err(|error| native::io_error(LocationOperation::ReadBackup, error))?;
        let name = entry.file_name();
        if name != DATABASE_NAME && name != COMPLETE_NAME {
            return Err(LocationError::InvalidBackup);
        }
    }
    Ok(())
}

fn read_marker(
    marker: &GuardedEntry,
    id: BackupId,
) -> Result<Option<BackupMetadata>, LocationError> {
    let mut file = &marker.file;
    let length = file
        .metadata()
        .map_err(|error| native::io_error(LocationOperation::ReadBackup, error))?
        .len();
    if length != MARKER_SIZE as u64 {
        return Ok(None);
    }
    let mut bytes = [0; MARKER_SIZE];
    match file.read_exact(&mut bytes) {
        Ok(()) => Ok(decode_marker(&bytes, id)),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(error) => Err(native::io_error(LocationOperation::ReadBackup, error)),
    }
}

fn encode_marker(metadata: BackupMetadata) -> [u8; MARKER_SIZE] {
    let mut bytes = [0; MARKER_SIZE];
    bytes[..8].copy_from_slice(MARKER_MAGIC);
    bytes[8..16].copy_from_slice(&metadata.id.0.to_be_bytes());
    bytes[16..24].copy_from_slice(&metadata.created_at.to_be_bytes());
    bytes[24..32].copy_from_slice(&metadata.revision.to_be_bytes());
    bytes[32..40].copy_from_slice(&metadata.deletion_generation.to_be_bytes());
    // A complemented duplicate rejects torn records without an unbounded parser.
    for index in 0..PAYLOAD_SIZE {
        bytes[PAYLOAD_SIZE + index] = !bytes[index];
    }
    bytes
}

fn decode_marker(bytes: &[u8; MARKER_SIZE], id: BackupId) -> Option<BackupMetadata> {
    if &bytes[..8] != MARKER_MAGIC
        || (0..PAYLOAD_SIZE).any(|index| bytes[PAYLOAD_SIZE + index] != !bytes[index])
    {
        return None;
    }
    let metadata = BackupMetadata {
        id: BackupId(u64::from_be_bytes(marker_field(bytes, 8))),
        created_at: i64::from_be_bytes(marker_field(bytes, 16)),
        revision: u64::from_be_bytes(marker_field(bytes, 24)),
        deletion_generation: u64::from_be_bytes(marker_field(bytes, 32)),
    };
    (metadata.id == id && id.0 != 0).then_some(metadata)
}

fn marker_field(bytes: &[u8; MARKER_SIZE], offset: usize) -> [u8; 8] {
    let mut field = [0; 8];
    field.copy_from_slice(&bytes[offset..offset + 8]);
    field
}

fn bundle_name(id: BackupId) -> String {
    format!("b-{:016x}", id.0)
}

fn parse_bundle_name(name: &str) -> Option<BackupId> {
    if name.len() != 18
        || !name.starts_with("b-")
        || !name[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let value = match u64::from_str_radix(&name[2..], 16) {
        Ok(value) => value,
        Err(_) => return None,
    };
    (value != 0).then_some(BackupId(value))
}

fn next_id() -> Result<BackupId, LocationError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| LocationError::BackupIdentityExhausted)?
        .as_micros();
    let now = u64::try_from(now).map_err(|_| LocationError::BackupIdentityExhausted)?;
    let previous = NEXT
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |previous| {
            previous.checked_add(1).map(|next| next.max(now))
        })
        .map_err(|_| LocationError::BackupIdentityExhausted)?;
    Ok(BackupId(
        previous
            .checked_add(1)
            .ok_or(LocationError::BackupIdentityExhausted)?
            .max(now),
    ))
}

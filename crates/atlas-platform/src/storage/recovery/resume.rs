//! Reconciles known interrupted moves without choosing a different backup or replacing unknown files.

use std::io::{Read, Write};

use atlas_store::BackupId;

use super::*;
use crate::storage::FileIdentity;

const PAYLOAD_BYTES: usize = 212;
const INTENT_BYTES: usize = PAYLOAD_BYTES * 2;
const MAGIC: &[u8; 16] = b"ATLAS-RECOVER-01";

pub(super) struct Continuation {
    bundle: RecoveryBundle,
    intent_entry: GuardedEntry,
    intent: Intent,
    stage: GuardedEntry,
    installed: bool,
}

pub(super) struct Intent {
    stamp: u128,
    sequence: u64,
    metadata: BackupMetadata,
    source: FileIdentity,
    stage: FileIdentity,
    primary: [Option<FileIdentity>; 4],
}

impl Intent {
    pub(super) fn new(
        bundle: &RecoveryBundle,
        metadata: BackupMetadata,
        source: FileIdentity,
        stage: FileIdentity,
        primary: &[PrimaryFile],
    ) -> Self {
        let mut identities = [None; 4];
        for (index, name) in std::iter::once(DATABASE_NAME)
            .chain(SIDECAR_NAMES)
            .enumerate()
        {
            identities[index] = primary
                .iter()
                .find(|file| file.name == name)
                .map(|file| file.entry.identity);
        }
        Self {
            stamp: bundle.stamp,
            sequence: bundle.sequence,
            metadata,
            source,
            stage,
            primary: identities,
        }
    }

    pub(super) fn write(
        &self,
        prepared: &PreparedDirectory,
    ) -> Result<GuardedEntry, RecoveryError> {
        let path = prepared.path.join(RECOVERY_INTENT_NAME);
        let mut file =
            native::create_new_file(&path, LocationOperation::OpenRecoveryIntent, false)?;
        file.write_all(&self.encode())
            .map_err(|error| native::io_error(LocationOperation::WriteControl, error))?;
        file.sync_all()
            .map_err(|error| native::io_error(LocationOperation::FlushControl, error))?;
        Ok(GuardedEntry::regular_file(path, file)?)
    }

    fn encode(&self) -> Vec<u8> {
        // Duplicated inverted bytes detect incomplete writes; this is not authentication.
        let mut bytes = Vec::with_capacity(INTENT_BYTES);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.stamp.to_be_bytes());
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        bytes.extend_from_slice(&self.metadata.id.0.to_be_bytes());
        bytes.extend_from_slice(&self.metadata.created_at.to_be_bytes());
        bytes.extend_from_slice(&self.metadata.revision.to_be_bytes());
        encode_identity(&mut bytes, self.source);
        encode_identity(&mut bytes, self.stage);
        for identity in self.primary {
            bytes.push(u8::from(identity.is_some()));
            encode_identity(
                &mut bytes,
                identity.unwrap_or(FileIdentity {
                    volume: 0,
                    id: [0; 16],
                }),
            );
        }
        for index in 0..PAYLOAD_BYTES {
            bytes.push(!bytes[index]);
        }
        bytes
    }

    fn read(entry: &GuardedEntry) -> Result<Self, RecoveryError> {
        let mut file = &entry.file;
        if file
            .metadata()
            .map_err(|error| native::io_error(LocationOperation::ReadControl, error))?
            .len()
            != INTENT_BYTES as u64
        {
            return Err(RecoveryError::InvalidIntent);
        }
        let mut bytes = [0; INTENT_BYTES];
        file.read_exact(&mut bytes)
            .map_err(|error| native::io_error(LocationOperation::ReadControl, error))?;
        Self::decode(&bytes)
    }

    fn decode(bytes: &[u8]) -> Result<Self, RecoveryError> {
        if bytes.len() != INTENT_BYTES {
            return Err(RecoveryError::InvalidIntent);
        }
        if &bytes[..16] != MAGIC
            || bytes[..PAYLOAD_BYTES]
                .iter()
                .zip(&bytes[PAYLOAD_BYTES..])
                .any(|(first, second)| *first != !second)
        {
            return Err(RecoveryError::InvalidIntent);
        }
        let mut primary = [None; 4];
        for (index, identity) in primary.iter_mut().enumerate() {
            let offset = 112 + index * 25;
            let decoded = decode_identity(bytes, offset + 1)?;
            *identity = match bytes[offset] {
                0 if decoded
                    == (FileIdentity {
                        volume: 0,
                        id: [0; 16],
                    }) =>
                {
                    None
                }
                1 => Some(decoded),
                _ => return Err(RecoveryError::InvalidIntent),
            };
        }
        Ok(Self {
            stamp: u128::from_be_bytes(number(bytes, 16)?),
            sequence: u64::from_be_bytes(number(bytes, 32)?),
            metadata: BackupMetadata {
                id: BackupId(u64::from_be_bytes(number(bytes, 40)?)),
                created_at: i64::from_be_bytes(number(bytes, 48)?),
                revision: u64::from_be_bytes(number(bytes, 56)?),
                deletion_generation: 0,
            },
            source: decode_identity(bytes, 64)?,
            stage: decode_identity(bytes, 88)?,
            primary,
        })
    }

    fn open_bundle(&self, prepared: &PreparedDirectory) -> Result<RecoveryBundle, RecoveryError> {
        // The path comes only from parsed numeric fields and fixed format text.
        let path = prepared.path.join(format!(
            "recovery-{:032x}-{:016x}",
            self.stamp, self.sequence
        ));
        Ok(RecoveryBundle {
            entry: GuardedEntry::directory(&path)?,
            stamp: self.stamp,
            sequence: self.sequence,
        })
    }
}

pub(super) fn offer(prepared: PreparedDirectory) -> Result<RecoveryOffer, RecoveryError> {
    let path = prepared.path.join(RECOVERY_INTENT_NAME);
    let intent_entry = GuardedEntry::regular_file(
        path.clone(),
        native::open_readonly(&path, LocationOperation::OpenRecoveryIntent)?,
    )?;
    let intent = Intent::read(&intent_entry)?;
    let bundle = intent.open_bundle(&prepared)?;
    let (primary, stage, installed) = reconcile(&prepared, &bundle, &intent)?;
    let archive = WindowsBackupArchive::new(prepared.clone());
    let backup = archive
        .candidates()?
        .into_iter()
        .find(|entry| entry.metadata.id == intent.metadata.id)
        .ok_or(RecoveryError::NoValidBackup)?;
    if backup.metadata != intent.metadata {
        return Err(RecoveryError::MetadataMismatch);
    }
    let source_path = normal_absolute_path(backup.file.database_path())?;
    let source = GuardedEntry::regular_file(
        source_path.clone(),
        native::open_readonly(&source_path, LocationOperation::ReadBackup)?,
    )?;
    if source.identity != intent.source {
        return Err(LocationError::IdentityChanged.into());
    }
    let expected = validate_backup(backup.file.as_ref())?;
    if expected.revision != backup.metadata.revision {
        return Err(RecoveryError::MetadataMismatch);
    }
    validate_expected(&prepared, &stage, &expected)?;
    Ok(RecoveryOffer {
        prepared,
        backup,
        source,
        expected: Box::new(expected),
        primary,
        continuation: Some(Box::new(Continuation {
            bundle,
            intent_entry,
            intent,
            stage,
            installed,
        })),
    })
}

pub(super) fn confirm(
    offer: RecoveryOffer,
    continuation: Continuation,
) -> Result<RecoveryOutcome, RecoveryError> {
    offer.prepared.revalidate()?;
    offer
        .source
        .revalidate(false, LocationOperation::ReadBackup)?;
    offer.backup.file.revalidate()?;
    continuation
        .intent_entry
        .revalidate(false, LocationOperation::OpenRecoveryIntent)?;
    // Reconcile again under the original offer's retained guards before any further move.
    let checked = reconcile(&offer.prepared, &continuation.bundle, &continuation.intent)?;
    if checked.2 != continuation.installed {
        return Err(LocationError::IdentityChanged.into());
    }
    drop(checked);
    validate_expected(&offer.prepared, &continuation.stage, &offer.expected)?;
    let preserved_files = offer.primary.len();
    let mut pending = Vec::new();
    for primary in offer.primary {
        if primary.entry.path.parent() == Some(offer.prepared.path.as_path()) {
            pending.push(primary);
        }
    }
    for (name, file) in bind_movable(pending)? {
        native::rename_no_replace(
            &file,
            &continuation.bundle.entry.path.join(name),
            LocationOperation::PreserveDatabase,
        )?;
        file.sync_all()
            .map_err(|error| native::io_error(LocationOperation::PreserveDatabase, error))?;
    }
    if continuation.installed {
        validate_expected(&offer.prepared, &continuation.stage, &offer.expected)?;
        require_absent(&offer.prepared, SIDECAR_NAMES)?;
    } else {
        install_stage(&offer.prepared, continuation.stage, &offer.expected)?;
    }
    complete_intent(continuation.intent_entry)?;
    Ok(RecoveryOutcome {
        restored: offer.backup.metadata,
        preserved_files,
    })
}

fn reconcile(
    prepared: &PreparedDirectory,
    bundle: &RecoveryBundle,
    intent: &Intent,
) -> Result<(Vec<PrimaryFile>, GuardedEntry, bool), RecoveryError> {
    bundle
        .entry
        .revalidate(true, LocationOperation::OpenDirectory)?;
    require_bundle_names(&bundle.entry.path)?;
    let mut root = Vec::new();
    let mut preserved = Vec::new();
    for name in std::iter::once(DATABASE_NAME).chain(SIDECAR_NAMES) {
        root.push(optional_guard(&prepared.path.join(name))?);
        preserved.push(optional_guard(&bundle.entry.path.join(name))?);
    }
    let staged = optional_guard(&bundle.entry.path.join("replacement.db"))?;
    let installed = root[0]
        .as_ref()
        .is_some_and(|entry| entry.identity == intent.stage);
    let stage = if installed {
        if staged.is_some() {
            return Err(RecoveryError::InvalidIntent);
        }
        root[0].take().ok_or(RecoveryError::InvalidIntent)?
    } else {
        let entry = staged.ok_or(RecoveryError::InvalidIntent)?;
        if entry.identity != intent.stage {
            return Err(LocationError::IdentityChanged.into());
        }
        entry
    };
    let mut primary = Vec::new();
    for (index, name) in std::iter::once(DATABASE_NAME)
        .chain(SIDECAR_NAMES)
        .enumerate()
    {
        let entry = match (
            intent.primary[index],
            root[index].take(),
            preserved[index].take(),
        ) {
            (None, None, None) => continue,
            (Some(identity), Some(entry), None) | (Some(identity), None, Some(entry))
                if entry.identity == identity =>
            {
                entry
            }
            _ => return Err(LocationError::IdentityChanged.into()),
        };
        primary.push(PrimaryFile { name, entry });
    }
    Ok((primary, stage, installed))
}

fn optional_guard(path: &Path) -> Result<Option<GuardedEntry>, RecoveryError> {
    let Some(file) = native::optional_file(path, LocationOperation::PreserveDatabase)? else {
        return Ok(None);
    };
    super::super::check_entry(&native::entry_info(&file)?, false)?;
    drop(file);
    Ok(Some(GuardedEntry::regular_file(
        path.to_owned(),
        native::open_readonly(path, LocationOperation::PreserveDatabase)?,
    )?))
}

fn require_bundle_names(path: &Path) -> Result<(), RecoveryError> {
    for entry in fs::read_dir(path)
        .map_err(|error| native::io_error(LocationOperation::PreserveDatabase, error))?
    {
        let entry =
            entry.map_err(|error| native::io_error(LocationOperation::PreserveDatabase, error))?;
        let name = entry.file_name();
        if !std::iter::once(DATABASE_NAME)
            .chain(SIDECAR_NAMES)
            .chain(["replacement.db"])
            .any(|allowed| name == allowed)
        {
            return Err(RecoveryError::InvalidIntent);
        }
    }
    Ok(())
}

fn encode_identity(bytes: &mut Vec<u8>, identity: FileIdentity) {
    bytes.extend_from_slice(&identity.volume.to_be_bytes());
    bytes.extend_from_slice(&identity.id);
}

fn decode_identity(bytes: &[u8], offset: usize) -> Result<FileIdentity, RecoveryError> {
    Ok(FileIdentity {
        volume: u64::from_be_bytes(number(bytes, offset)?),
        id: number(bytes, offset + 8)?,
    })
}

fn number<const SIZE: usize>(bytes: &[u8], offset: usize) -> Result<[u8; SIZE], RecoveryError> {
    bytes
        .get(offset..offset + SIZE)
        .ok_or(RecoveryError::InvalidIntent)?
        .try_into()
        .map_err(|_| RecoveryError::InvalidIntent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_roundtrip_refuses_every_truncation_byte_flip_and_extra_byte() {
        let identity = FileIdentity {
            volume: u64::MAX,
            id: [37; 16],
        };
        let intent = Intent {
            stamp: u128::MAX,
            sequence: u64::MAX,
            metadata: BackupMetadata {
                id: BackupId(91),
                created_at: i64::MIN,
                revision: u64::MAX,
                deletion_generation: 0,
            },
            source: identity,
            stage: FileIdentity {
                volume: 7,
                id: [81; 16],
            },
            primary: [Some(identity), None, Some(identity), None],
        };
        let bytes = intent.encode();
        assert_eq!(bytes.len(), INTENT_BYTES);
        let decoded = Intent::decode(&bytes).expect("synthetic fixed frame decodes");
        assert_eq!(decoded.stamp, intent.stamp);
        assert_eq!(decoded.sequence, intent.sequence);
        assert_eq!(decoded.metadata, intent.metadata);
        assert!(decoded.source == intent.source);
        assert!(decoded.stage == intent.stage);
        assert!(decoded.primary == intent.primary);
        for end in 0..bytes.len() {
            assert!(matches!(
                Intent::decode(&bytes[..end]),
                Err(RecoveryError::InvalidIntent)
            ));
        }
        for index in 0..bytes.len() {
            let mut damaged = bytes.clone();
            damaged[index] ^= 1;
            assert!(matches!(
                Intent::decode(&damaged),
                Err(RecoveryError::InvalidIntent)
            ));
        }
        let mut extended = bytes;
        extended.push(0);
        assert!(matches!(
            Intent::decode(&extended),
            Err(RecoveryError::InvalidIntent)
        ));
    }
}

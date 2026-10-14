//! Publishes fully checked idle snapshots before rotating older valid copies.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::error::{database_error, with_cleanup};
use crate::{
    BackupArchive, BackupEntry, BackupFile, BackupMetadata, DirectoryLease, LocationError,
    StoreError, StoreOperation, StoredSnapshot, schema,
};

const DAY_SECONDS: i64 = 86_400;
const RETAINED_BACKUPS: usize = 7;

/// The host supplies idle state and UTC time; storage creates no timer or background poller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupOutcome {
    NotIdle,
    NotDue,
    Created(BackupMetadata),
}

#[derive(Default)]
pub(crate) struct BackupSchedule {
    discovered: bool,
    latest: Option<i64>,
}

/// Validates a guarded file without modifying it. Publication and deletion metadata are separate.
pub fn validate_backup(file: &dyn BackupFile) -> Result<StoredSnapshot, StoreError> {
    validate_checked_file(file.database_path(), || file.revalidate())
}

/// Checks a guarded staged or installed database without claiming a session or enabling writes.
pub fn validate_database(lease: &dyn DirectoryLease) -> Result<StoredSnapshot, StoreError> {
    validate_checked_file(lease.database_path(), || lease.revalidate())
}

fn validate_checked_file(
    path: &Path,
    revalidate: impl Fn() -> Result<(), LocationError>,
) -> Result<StoredSnapshot, StoreError> {
    revalidate()?;
    let mut connection = Connection::open_with_flags_and_vfs(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        "win32-longpath",
    )
    .map_err(|error| database_error(StoreOperation::Open, error))?;
    let result = schema::validate_backup(&mut connection).and_then(|snapshot| {
        revalidate()?;
        Ok(snapshot)
    });
    let closed = connection.close().map_err(|(connection, error)| {
        let error = database_error(StoreOperation::Close, error);
        drop(connection);
        error
    });
    match result {
        Ok(snapshot) => closed.map(|()| snapshot),
        Err(error) => Err(with_cleanup(error, closed)),
    }
}

impl BackupSchedule {
    pub(crate) fn create_if_due(
        &mut self,
        connection: &mut Connection,
        archive: &dyn BackupArchive,
        now_utc_seconds: i64,
        idle: bool,
    ) -> Result<BackupOutcome, StoreError> {
        if !idle {
            return Ok(BackupOutcome::NotIdle);
        }
        self.discover(archive)?;
        if let Some(latest) = self.latest {
            // A backwards clock cannot advance the schedule; subtraction uses a wider representation.
            if i128::from(now_utc_seconds) - i128::from(latest) < i128::from(DAY_SECONDS) {
                return Ok(BackupOutcome::NotDue);
            }
        }
        self.create(connection, archive, now_utc_seconds)
            .map(BackupOutcome::Created)
    }

    /// Takes one snapshot outside the daily schedule, as before an update installs (P1-39b).
    /// D13's rotation still keeps seven, so the oldest valid copy may be removed.
    pub(crate) fn create_now(
        &mut self,
        connection: &mut Connection,
        archive: &dyn BackupArchive,
        now_utc_seconds: i64,
    ) -> Result<BackupMetadata, StoreError> {
        self.discover(archive)?;
        self.create(connection, archive, now_utc_seconds)
    }

    /// Learns the newest valid backup once, so the schedule survives a restart.
    fn discover(&mut self, archive: &dyn BackupArchive) -> Result<(), StoreError> {
        if !self.discovered {
            let existing = valid_candidates(archive)?;
            self.latest = existing.first().map(|entry| entry.metadata.created_at);
            self.discovered = true;
        }
        Ok(())
    }

    /// Writes, validates and publishes one snapshot, then rotates older valid copies.
    fn create(
        &mut self,
        connection: &mut Connection,
        archive: &dyn BackupArchive,
        now_utc_seconds: i64,
    ) -> Result<BackupMetadata, StoreError> {
        // VACUUM INTO must run outside a transaction; the writer queue excludes concurrent saves.
        let file = archive.reserve()?;
        // A failed publication may already have written its marker; preserve the output on error.
        let metadata = write_snapshot(connection, file.as_ref(), now_utc_seconds)?;
        drop(file);

        // Recollect after publication; never delete a corrupt, unrecognized or unfinished bundle.
        let valid = valid_candidates(archive)?;
        if !valid.iter().any(|entry| entry.metadata == metadata) {
            return Err(StoreError::BackupVerificationFailed);
        }
        for entry in valid.into_iter().skip(RETAINED_BACKUPS) {
            entry.file.remove()?;
        }
        self.latest = Some(now_utc_seconds);
        Ok(metadata)
    }
}

fn write_snapshot(
    connection: &mut Connection,
    file: &dyn BackupFile,
    now_utc_seconds: i64,
) -> Result<BackupMetadata, StoreError> {
    file.revalidate()?;
    let path = file
        .database_path()
        .to_str()
        .ok_or(StoreError::InvalidBackupPath)?;
    connection
        .execute("VACUUM main INTO ?1", [path])
        .map_err(|error| database_error(StoreOperation::Backup, error))?;
    let snapshot = validate_backup(file)?;
    file.flush()?;
    let metadata = BackupMetadata {
        id: file.id(),
        created_at: now_utc_seconds,
        revision: snapshot.revision,
        // The copy carries the same generation, which recovery checks before replaying past it.
        deletion_generation: crate::history::applied_generation(connection)?,
    };
    // Completion is the final write; an interrupted or failed validation never becomes eligible.
    file.publish(metadata)?;
    Ok(metadata)
}

fn valid_candidates(archive: &dyn BackupArchive) -> Result<Vec<BackupEntry>, StoreError> {
    let mut valid = Vec::new();
    for entry in archive.candidates()? {
        if entry.metadata.id != entry.file.id() {
            continue;
        }
        match validate_backup(entry.file.as_ref()) {
            Ok(snapshot) if snapshot.revision == entry.metadata.revision => valid.push(entry),
            Ok(_) => {}
            Err(error) if invalid_candidate(&error) => {}
            // A newer Syrinx's backup is skipped and never pruned, so a downgrade cannot lose it;
            // recovery reports such skips (`RecoveryError::OnlyNewerSchemaBackups`).
            Err(StoreError::FutureSchema { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    valid.sort_by_key(|entry| std::cmp::Reverse((entry.metadata.created_at, entry.metadata.id)));
    Ok(valid)
}

fn invalid_candidate(error: &StoreError) -> bool {
    match error {
        StoreError::ForeignDatabase
        | StoreError::SchemaMismatch
        | StoreError::IntegrityCheckFailed
        | StoreError::InvalidEncoding(_)
        | StoreError::InvalidSnapshot(_) => true,
        StoreError::Database {
            code: Some(code), ..
        } => {
            matches!(
                code & 0xff,
                rusqlite::ffi::SQLITE_CORRUPT | rusqlite::ffi::SQLITE_NOTADB
            )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;

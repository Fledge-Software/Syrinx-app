//! Acquires startup storage without treating a missing established primary as a new workspace.

use atlas_store::deletion::DeletionLog;

use super::*;

#[cfg(test)]
mod tests;

/// Retains exclusive storage ownership until normal opening or confirmed recovery.
#[derive(Debug)]
pub enum StartupStorage {
    Ready(WindowsDirectoryLease),
    Recovery(RecoveryOffer),
}

/// Initializes only a directory created by this call; missing established state needs recovery.
pub fn prepare_startup(
    directory: &Path,
    deletions: &DeletionLog,
) -> Result<StartupStorage, RecoveryError> {
    prepare_with_probe(directory, deletions, Arc::new(native::WindowsProbe))
}

fn prepare_with_probe(
    directory: &Path,
    deletions: &DeletionLog,
    probe: Arc<dyn LocationProbe>,
) -> Result<StartupStorage, RecoveryError> {
    recovery::require_zero_generation(deletions)?;
    let (prepared, created_final) = prepare_directory_with_creation(directory, probe, true)?;
    acquire(prepared, created_final)
}

fn acquire(
    prepared: PreparedDirectory,
    created_final: bool,
) -> Result<StartupStorage, RecoveryError> {
    // Only an exclusively created final directory may initialize a primary.
    if created_final {
        if !contains_only_control(&prepared)? {
            return Err(LocationError::IdentityChanged.into());
        }
        return create_fresh_primary(prepared);
    }
    // Interrupted recovery keeps its exact source, even if an installed primary is present.
    if recovery::has_intent(&prepared)? {
        return recovery::offer_prepared(prepared).map(StartupStorage::Recovery);
    }
    prepared.control.revalidate()?;
    let path = prepared.path.join(DATABASE_NAME);
    if let Some(file) = native::optional_retained_file(&path, LocationOperation::OpenDatabase)? {
        let database = GuardedEntry::regular_file(path, file)?;
        return WindowsDirectoryLease::from_prepared(prepared, database)
            .map(StartupStorage::Ready)
            .map_err(RecoveryError::from);
    }

    recovery::offer_prepared(prepared).map(StartupStorage::Recovery)
}

fn create_fresh_primary(prepared: PreparedDirectory) -> Result<StartupStorage, RecoveryError> {
    let path = prepared.path.join(DATABASE_NAME);
    let file = native::create_new_file(&path, LocationOperation::CreateDatabase, true)?;
    let database = GuardedEntry::regular_file(path, file)?;
    WindowsDirectoryLease::from_prepared(prepared, database)
        .map(StartupStorage::Ready)
        .map_err(RecoveryError::from)
}

fn contains_only_control(prepared: &PreparedDirectory) -> Result<bool, LocationError> {
    prepared.revalidate()?;
    let entries = fs::read_dir(&prepared.path)
        .map_err(|error| native::io_error(LocationOperation::OpenDirectory, error))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| native::io_error(LocationOperation::OpenDirectory, error))?;
        if entry.file_name() != OsStr::new(session::CONTROL_NAME) {
            return Ok(false);
        }
    }
    prepared.control.revalidate()?;
    Ok(true)
}

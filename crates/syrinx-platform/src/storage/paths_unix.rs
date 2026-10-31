//! Spells and checks storage paths on Linux and macOS, where there are no drives or DOS names.

use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
};

use syrinx_store::LocationError;

use super::{LOCAL_DRIVE, LocationProbe};

/// Keeps one absolute spelling with no `.` or `..`, so identity checks see one name.
pub(super) fn normal_absolute_path(path: &Path) -> Result<PathBuf, LocationError> {
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return Err(LocationError::InvalidPath);
    }
    let mut normalized = PathBuf::from("/");
    for component in components {
        let Component::Normal(name) = component else {
            return Err(LocationError::InvalidPath);
        };
        validate_component(name)?;
        normalized.push(name);
    }
    Ok(normalized)
}

/// SQLite opens the ordinary path; Unix has no long-path spelling to choose.
pub(super) fn sqlite_database_path(path: &Path) -> PathBuf {
    path.to_owned()
}

pub(super) fn validate_component(name: &OsStr) -> Result<(), LocationError> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes == b"."
        || bytes == b".."
        || bytes.iter().any(|byte| *byte == b'/' || *byte < 32)
    {
        return Err(LocationError::InvalidPath);
    }
    Ok(())
}

/// Checks the filesystem that holds the deepest existing part of `path`, not only `/`.
pub(super) fn require_local_drive(
    path: &Path,
    probe: &dyn LocationProbe,
) -> Result<(), LocationError> {
    let existing = path
        .ancestors()
        .find(|ancestor| ancestor.exists())
        .ok_or(LocationError::InvalidPath)?;
    if probe.drive_type(existing)? != LOCAL_DRIVE {
        return Err(LocationError::NonLocalDrive);
    }
    Ok(())
}

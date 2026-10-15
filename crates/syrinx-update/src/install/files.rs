//! File operations that refuse links and replace only folders Syrinx made.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use super::InstallError;
use super::error::io_error;
use super::layout::PACKAGE_RECORD;
use crate::path::PackagePath;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
/// Windows' "used by another process" error, which a closing program clears shortly.
const ERROR_SHARING_VIOLATION: i32 = 32;
/// How often a refused rename is retried.
const RENAME_RETRY: Duration = Duration::from_millis(100);

pub(crate) fn is_reparse_point(metadata: &Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// The entry's own metadata, or `None` when nothing is there.
fn entry_metadata(path: &Path, step: &'static str) -> Result<Option<Metadata>, InstallError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(step)(error)),
    }
}

/// Whether anything, a link included, is at `path`.
pub(crate) fn entry_present(path: &Path, step: &'static str) -> Result<bool, InstallError> {
    Ok(entry_metadata(path, step)?.is_some())
}

/// Create `path` if missing, then require a plain directory rather than a link or a file.
pub(crate) fn ensure_plain_directory(path: &Path, what: &'static str) -> Result<(), InstallError> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error("creating an update folder")(error)),
    }
    let metadata = entry_metadata(path, "inspecting an update folder")?;
    match metadata {
        Some(metadata) if metadata.is_dir() && !is_reparse_point(&metadata) => Ok(()),
        _ => Err(InstallError::UnexpectedEntry { what }),
    }
}

/// Whether `path` is an empty folder or one holding P1-37's package record: a program
/// folder Syrinx's tooling made. Links are never ours.
pub(crate) fn is_program_folder(path: &Path) -> Result<bool, InstallError> {
    let Some(metadata) = entry_metadata(path, "inspecting a program folder")? else {
        return Ok(false);
    };
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Ok(false);
    }
    let record = entry_metadata(&path.join(PACKAGE_RECORD), "inspecting a program folder")?;
    if let Some(record) = record {
        return Ok(record.is_file() && !is_reparse_point(&record));
    }
    let mut entries = fs::read_dir(path).map_err(io_error("listing a program folder"))?;
    Ok(entries.next().is_none())
}

/// Remove a program folder Syrinx made; refuse anything else. Nothing there is fine.
pub(crate) fn remove_program_folder(path: &Path, what: &'static str) -> Result<(), InstallError> {
    if entry_metadata(path, "inspecting a program folder")?.is_none() {
        return Ok(());
    }
    if !is_program_folder(path)? {
        return Err(InstallError::UnexpectedEntry { what });
    }
    fs::remove_dir_all(path).map_err(io_error("removing an old program folder"))
}

/// Remove a folder inside `Updates`, refusing a link in its place. Nothing there is fine.
pub(crate) fn remove_update_folder(path: &Path, what: &'static str) -> Result<(), InstallError> {
    let Some(metadata) = entry_metadata(path, "inspecting an update folder")? else {
        return Ok(());
    };
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err(InstallError::UnexpectedEntry { what });
    }
    fs::remove_dir_all(path).map_err(io_error("removing an update folder"))
}

pub(crate) fn remove_file_if_present(path: &Path, step: &'static str) -> Result<(), InstallError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(step)(error)),
    }
}

/// Read a small local file whole, or `None` when it does not exist.
pub(crate) fn read_small(
    path: &Path,
    limit: usize,
    step: &'static str,
) -> Result<Option<Vec<u8>>, InstallError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(step)(error)),
    };
    let mut bytes = Vec::new();
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io_error(step))?;
    if bytes.len() > limit {
        return Err(InstallError::Record { what: step });
    }
    Ok(Some(bytes))
}

/// Write through a temporary file flushed to disk, then rename it over `path`.
pub(crate) fn write_atomically(
    path: &Path,
    bytes: &[u8],
    step: &'static str,
) -> Result<(), InstallError> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut file = File::create(&temporary).map_err(io_error(step))?;
    file.write_all(bytes).map_err(io_error(step))?;
    file.sync_all().map_err(io_error(step))?;
    drop(file);
    fs::rename(&temporary, path).map_err(io_error(step))
}

/// Write a new file that must not exist yet, and flush it to disk.
pub(crate) fn write_new(path: &Path, bytes: &[u8], step: &'static str) -> Result<(), InstallError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error(step))?;
    file.write_all(bytes).map_err(io_error(step))?;
    file.sync_all().map_err(io_error(step))
}

/// Copy exactly `size` bytes from `reader` into a new file at `path`, flushed to disk.
///
/// Reading stops one byte past `size`, so a longer source is refused, not truncated.
pub(crate) fn copy_exact(
    reader: &mut impl Read,
    path: &Path,
    size: u64,
    step: &'static str,
) -> Result<bool, InstallError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error(step))?;
    let copied =
        io::copy(&mut reader.take(size.saturating_add(1)), &mut file).map_err(io_error(step))?;
    file.sync_all().map_err(io_error(step))?;
    Ok(copied == size)
}

/// `root` joined with a checked package path, creating its parent folders.
pub(crate) fn prepare_destination(
    root: &Path,
    path: &PackagePath,
    step: &'static str,
) -> Result<PathBuf, InstallError> {
    let destination = path
        .as_str()
        .split('/')
        .fold(root.to_path_buf(), |joined, part| joined.join(part));
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(io_error(step))?;
    }
    Ok(destination)
}

/// Rename, retrying while a just-closed program still holds the folder.
pub(crate) fn rename_patiently(
    from: &Path,
    to: &Path,
    patience: Duration,
    step: &'static str,
) -> Result<(), InstallError> {
    let deadline = Instant::now() + patience;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if is_transient(&error) && Instant::now() < deadline => {
                thread::sleep(RENAME_RETRY);
            }
            Err(error) => return Err(io_error(step)(error)),
        }
    }
}

fn is_transient(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied
        || error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
}

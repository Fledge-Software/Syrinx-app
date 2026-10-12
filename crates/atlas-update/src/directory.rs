//! A staging-directory payload that refuses links and holds deny-write handles until dropped.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::error::UpdateError;
use crate::path::PackagePath;
use crate::payload::{Payload, read_error};

/// Share only reading, so no other handle can write, rename or delete while one is held.
const FILE_SHARE_READ: u32 = 0x0000_0001;
/// Required to open a directory handle.
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
/// Open a link itself rather than its target, so the attribute check below sees it.
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
/// Most entries a staging directory may hold before listing stops.
const MAX_ENTRIES: usize = 65_536;

/// Payload files under one staging directory; directory handles are held for its lifetime.
pub struct DirectoryPayload {
    root: PathBuf,
    directories: Vec<File>,
}

impl DirectoryPayload {
    /// Open and hold the staging directory, refusing it if it is itself a link.
    pub fn open(root: &Path) -> Result<Self, UpdateError> {
        let handle = open_plain(root, "payload root", true)?;
        Ok(Self {
            root: root.to_path_buf(),
            directories: vec![handle],
        })
    }

    /// Join a `/`-separated relative name onto the root one component at a time.
    fn resolve(&self, relative: &str) -> PathBuf {
        let mut path = self.root.clone();
        path.extend(
            relative
                .split('/')
                .filter(|component| !component.is_empty()),
        );
        path
    }

    /// List one directory, holding each subdirectory and queueing it for listing.
    fn list_directory(
        &mut self,
        relative: &str,
        names: &mut Vec<String>,
        pending: &mut Vec<String>,
    ) -> Result<(), UpdateError> {
        let directory = self.resolve(relative);
        let entries = fs::read_dir(&directory).map_err(|error| read_error(relative, &error))?;
        for entry in entries {
            let entry = entry.map_err(|error| read_error(relative, &error))?;
            let name = entry_name(relative, &entry)?;
            if names.len() == MAX_ENTRIES {
                return Err(UpdateError::TooLarge {
                    what: "payload listing",
                    limit: MAX_ENTRIES as u64,
                });
            }
            // DirEntry metadata on Windows describes the entry itself, never a link's target.
            let metadata = entry
                .metadata()
                .map_err(|error| read_error(&name, &error))?;
            if is_reparse_point(&metadata) || !(metadata.is_dir() || metadata.is_file()) {
                return Err(UpdateError::UnsafeEntry { name });
            }
            if metadata.is_dir() {
                let handle = open_plain(&entry.path(), &name, true)?;
                self.directories.push(handle);
                pending.push(name.clone());
            }
            names.push(name);
        }
        Ok(())
    }
}

impl Payload for DirectoryPayload {
    type File = File;

    fn names(&mut self) -> Result<Vec<String>, UpdateError> {
        let mut names = Vec::new();
        let mut pending = vec![String::new()];
        while let Some(relative) = pending.pop() {
            self.list_directory(&relative, &mut names, &mut pending)?;
        }
        Ok(names)
    }

    fn open(&mut self, path: &PackagePath) -> Result<Option<File>, UpdateError> {
        let full = self.resolve(path.as_str());
        match open_plain(&full, path.as_str(), false) {
            Err(UpdateError::Read {
                kind: io::ErrorKind::NotFound,
                ..
            }) => Ok(None),
            Err(error) => Err(error),
            Ok(file) => Ok(Some(file)),
        }
    }
}

/// The entry's `/`-separated relative name; non-Unicode names are refused.
fn entry_name(relative: &str, entry: &fs::DirEntry) -> Result<String, UpdateError> {
    let name = entry
        .file_name()
        .into_string()
        .map_err(|raw| UpdateError::UnsafeEntry {
            name: raw.to_string_lossy().into_owned(),
        })?;
    if relative.is_empty() {
        return Ok(name);
    }
    Ok(format!("{relative}/{name}"))
}

/// Open read-only with deny-write sharing, then refuse links and the wrong kind of entry.
fn open_plain(path: &Path, name: &str, directory: bool) -> Result<File, UpdateError> {
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
    if directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS;
    }
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(flags)
        .open(path)
        .map_err(|error| read_error(name, &error))?;
    let metadata = handle
        .metadata()
        .map_err(|error| read_error(name, &error))?;
    if is_reparse_point(&metadata) || metadata.is_dir() != directory {
        return Err(UpdateError::UnsafeEntry {
            name: name.to_owned(),
        });
    }
    Ok(handle)
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

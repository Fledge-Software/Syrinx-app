//! Checks payload files against a verified manifest and keeps the exact handles it hashed.

use std::collections::BTreeSet;
use std::io::{self, Read, Seek, SeekFrom};

use crate::cng::Sha256;
use crate::error::UpdateError;
use crate::manifest::FileEntry;
use crate::path::PackagePath;

/// Bytes read from a payload file per step.
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Where payload files come from, such as `DirectoryPayload`'s staging directory.
///
/// Verification holds only for the bytes it read. Install from the handles in the returned
/// `VerifiedPayload`, never by opening the files again.
pub trait Payload {
    type File: Read + Seek;

    /// Every entry the source holds, files and directories alike, as `/`-separated names in
    /// their stored letter case. Links, junctions and other reparse points must be refused.
    fn names(&mut self) -> Result<Vec<String>, UpdateError>;

    /// Open a listed file for reading while refusing writers, or return `None` when absent.
    /// The handle must be the plain file itself, never one reached through a link.
    fn open(&mut self, path: &PackagePath) -> Result<Option<Self::File>, UpdateError>;
}

/// A payload file whose hashed handle is rewound and ready to install from.
pub struct VerifiedFile<F> {
    entry: FileEntry,
    reader: F,
}

impl<F> VerifiedFile<F> {
    pub fn entry(&self) -> &FileEntry {
        &self.entry
    }

    /// The same handle that was hashed, positioned at its start.
    pub fn reader(&mut self) -> &mut F {
        &mut self.reader
    }
}

/// Every verified file, with the source kept alive so its directory guards stay held.
pub struct VerifiedPayload<P: Payload> {
    files: Vec<VerifiedFile<P::File>>,
    _source: P,
}

impl<P: Payload> VerifiedPayload<P> {
    pub fn files_mut(&mut self) -> &mut [VerifiedFile<P::File>] {
        &mut self.files
    }
}

/// Refuse extras, then hash each listed file and keep its handle.
pub(crate) fn verify<P: Payload>(
    files: &[FileEntry],
    mut payload: P,
) -> Result<VerifiedPayload<P>, UpdateError> {
    refuse_unlisted(files, &mut payload)?;
    let mut verified = Vec::with_capacity(files.len());
    for entry in files {
        verified.push(verify_file(entry, &mut payload)?);
    }
    Ok(VerifiedPayload {
        files: verified,
        _source: payload,
    })
}

/// Refuse any name that is not a listed file or one of their directories, in exact case.
fn refuse_unlisted<P: Payload>(files: &[FileEntry], payload: &mut P) -> Result<(), UpdateError> {
    let mut allowed: BTreeSet<&str> = BTreeSet::new();
    for entry in files {
        let path = entry.path().as_str();
        allowed.insert(path);
        allowed.extend(path.match_indices('/').map(|(index, _)| &path[..index]));
    }
    let unlisted = payload
        .names()?
        .into_iter()
        .find(|name| !allowed.contains(name.as_str()));
    match unlisted {
        Some(name) => Err(UpdateError::UnlistedFile { name }),
        None => Ok(()),
    }
}

/// Hash one file, check it against its entry and rewind the handle for installation.
fn verify_file<P: Payload>(
    entry: &FileEntry,
    payload: &mut P,
) -> Result<VerifiedFile<P::File>, UpdateError> {
    let path = entry.path().as_str();
    let mut file = payload
        .open(entry.path())?
        .ok_or_else(|| UpdateError::MissingFile {
            path: path.to_owned(),
        })?;
    let (length, digest) = hash_bounded(&mut file, entry.size(), path)?;
    if length != entry.size() {
        return Err(UpdateError::SizeMismatch {
            path: path.to_owned(),
            expected: entry.size(),
        });
    }
    if &digest != entry.sha256() {
        return Err(UpdateError::HashMismatch {
            path: path.to_owned(),
        });
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| read_error(path, &error))?;
    Ok(VerifiedFile {
        entry: entry.clone(),
        reader: file,
    })
}

/// Hash up to `limit + 1` bytes, returning how many were read.
fn hash_bounded(
    file: &mut impl Read,
    limit: u64,
    path: &str,
) -> Result<(u64, [u8; 32]), UpdateError> {
    let mut reader = file.take(limit.saturating_add(1));
    let mut hash = Sha256::new()?;
    let mut buffer = vec![0u8; READ_CHUNK_BYTES];
    let mut length: u64 = 0;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(read_error(path, &error)),
        };
        let chunk = buffer.get(..count).ok_or_else(|| UpdateError::Read {
            what: path.to_owned(),
            kind: io::ErrorKind::InvalidData,
        })?;
        hash.update(chunk)?;
        length = length.saturating_add(count as u64);
    }
    Ok((length, hash.finish()?))
}

/// Read a whole input of at most `limit` bytes, refusing anything longer.
pub fn read_bounded(
    reader: impl Read,
    limit: usize,
    what: &'static str,
) -> Result<Vec<u8>, UpdateError> {
    let mut bytes = Vec::new();
    reader
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| read_error(what, &error))?;
    if bytes.len() > limit {
        return Err(UpdateError::TooLarge {
            what,
            limit: limit as u64,
        });
    }
    Ok(bytes)
}

pub(crate) fn read_error(what: &str, error: &io::Error) -> UpdateError {
    UpdateError::Read {
        what: what.to_owned(),
        kind: error.kind(),
    }
}

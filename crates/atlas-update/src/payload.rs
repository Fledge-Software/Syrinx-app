//! Checks payload files against a verified manifest by streaming, with bounded reads.

use std::collections::BTreeSet;
use std::io::{self, Read};

use crate::cng::Sha256;
use crate::error::UpdateError;
use crate::manifest::FileEntry;

/// Bytes read from a payload file per step.
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Where payload files come from; P1-39 will provide a staging-directory source.
pub trait Payload {
    type File: Read;

    /// Every file name the source holds, so unlisted extras can be refused.
    fn names(&mut self) -> io::Result<Vec<String>>;

    /// Open a listed file, or return `None` when it is absent.
    fn open(&mut self, path: &str) -> io::Result<Option<Self::File>>;
}

/// Refuse extras, then check each listed file's size and hash.
pub(crate) fn verify<P: Payload>(files: &[FileEntry], payload: &mut P) -> Result<(), UpdateError> {
    refuse_unlisted(files, payload)?;
    for entry in files {
        verify_file(entry, payload)?;
    }
    Ok(())
}

/// Refuse any name the manifest does not list exactly.
fn refuse_unlisted<P: Payload>(files: &[FileEntry], payload: &mut P) -> Result<(), UpdateError> {
    let listed: BTreeSet<&str> = files.iter().map(|entry| entry.path().as_str()).collect();
    let names = payload
        .names()
        .map_err(|error| read_error("payload listing", &error))?;
    match names
        .into_iter()
        .find(|name| !listed.contains(name.as_str()))
    {
        Some(name) => Err(UpdateError::UnlistedFile { name }),
        None => Ok(()),
    }
}

/// Stream one file through SHA-256, reading at most one byte past its declared size.
fn verify_file<P: Payload>(entry: &FileEntry, payload: &mut P) -> Result<(), UpdateError> {
    let path = entry.path().as_str();
    let file = payload
        .open(path)
        .map_err(|error| read_error(path, &error))?
        .ok_or_else(|| UpdateError::MissingFile {
            path: path.to_owned(),
        })?;
    let (length, digest) = hash_bounded(file, entry.size(), path)?;
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
    Ok(())
}

/// Hash up to `limit + 1` bytes, returning how many were read.
fn hash_bounded(file: impl Read, limit: u64, path: &str) -> Result<(u64, [u8; 32]), UpdateError> {
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

fn read_error(what: &str, error: &io::Error) -> UpdateError {
    UpdateError::Read {
        what: what.to_owned(),
        kind: error.kind(),
    }
}

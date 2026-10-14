//! Where staging reads a package from: a folder the owner chose, or the update host (P1-39c).
//!
//! Staging trusts no source; every byte a source returns is verified before it is installed.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::InstallError;
use super::error::io_error;
use crate::error::UpdateError;
use crate::manifest::FileEntry;
use crate::payload::read_bounded;
use crate::version::Version;

/// One of a package's small signed files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SignedFile {
    /// `latest`: the signed latest-version statement.
    Statement,
    /// `latest.sig`.
    StatementSignature,
    /// The stated version's `manifest`.
    Manifest(Version),
    /// The stated version's `manifest.sig`.
    ManifestSignature(Version),
}

impl SignedFile {
    /// The file's name within a package folder.
    pub fn name(self) -> &'static str {
        match self {
            Self::Statement => "latest",
            Self::StatementSignature => "latest.sig",
            Self::Manifest(_) => "manifest",
            Self::ManifestSignature(_) => "manifest.sig",
        }
    }
}

impl fmt::Display for SignedFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// A package to stage, read file by file. Staging verifies everything a source returns.
pub trait PackageSource {
    /// Read `file` whole, refusing more than `limit` bytes.
    fn read_signed(&self, file: SignedFile, limit: usize) -> Result<Vec<u8>, InstallError>;

    /// Open the payload file `entry` of `version` for reading. Staging reads at most one byte
    /// past `entry.size()`, so a source may also refuse a longer body itself.
    fn open_file(
        &self,
        version: Version,
        entry: &FileEntry,
    ) -> Result<Box<dyn Read + '_>, InstallError>;
}

/// The folder `cargo xtask package` and `cargo xtask sign-update` wrote (P1-39b).
pub struct FolderSource {
    folder: PathBuf,
}

impl FolderSource {
    pub fn new(folder: &Path) -> Self {
        Self {
            folder: folder.to_path_buf(),
        }
    }
}

impl PackageSource for FolderSource {
    fn read_signed(&self, file: SignedFile, limit: usize) -> Result<Vec<u8>, InstallError> {
        let opened = match File::open(self.folder.join(file.name())) {
            Ok(opened) => opened,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(InstallError::NotAPackage);
            }
            Err(error) => return Err(io_error("reading the update package")(error)),
        };
        Ok(read_bounded(opened, limit, file.name())?)
    }

    fn open_file(
        &self,
        _version: Version,
        entry: &FileEntry,
    ) -> Result<Box<dyn Read + '_>, InstallError> {
        let path = entry.path().as_str();
        let from = path
            .split('/')
            .fold(self.folder.clone(), |joined, part| joined.join(part));
        match File::open(&from) {
            Ok(file) => Ok(Box::new(file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(UpdateError::MissingFile {
                    path: path.to_owned(),
                }
                .into())
            }
            Err(error) => Err(io_error("reading the update package")(error)),
        }
    }
}

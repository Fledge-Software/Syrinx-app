//! The fixed deny list (note 10, section 3): no folder grant may equal or contain Syrinx's own
//! folders, the system's, a drive root or the user-profile root. One check serves grant time
//! and every later open, and runs on canonical paths that Syrinx resolved itself.

use std::fmt;
use std::path::{Path, PathBuf};

/// A folder the deny list protects. Folder paths are owner data, so only the kind is ever shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protected {
    /// Syrinx's data folder, which holds its database.
    SyrinxData,
    /// The folder of D13's daily snapshots.
    Snapshots,
    /// The web engine's user-data folder, which holds the owner's signed-in sessions.
    WebViewData,
    /// `%WINDIR%`.
    System,
    /// `%ProgramFiles%` and its 32-bit twin.
    ProgramFiles,
    /// The root of the owner's user profile. A folder inside it may be granted.
    UserProfile,
}

impl Protected {
    /// Whether a folder inside this one is refused too. Only the profile's insides are the
    /// owner's own folders, such as a vault under Documents.
    fn refuses_inside(self) -> bool {
        !matches!(self, Self::UserProfile)
    }
}

/// Why a folder may not be granted or opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRefusal {
    /// The folder could not be resolved: it does not exist or cannot be read.
    Unresolved,
    NotAFolder,
    /// A drive root, or the file system's root.
    Root,
    Protected(Protected),
}

impl fmt::Display for FolderRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unresolved => write!(formatter, "the folder could not be found"),
            Self::NotAFolder => write!(formatter, "it is not a folder"),
            Self::Root => write!(formatter, "a whole drive cannot be granted"),
            Self::Protected(kind) => write!(formatter, "it overlaps a protected folder ({kind:?})"),
        }
    }
}

impl std::error::Error for FolderRefusal {}

/// The protected folders, each kept in its canonical form.
#[derive(Default, Clone)]
pub struct DenyList {
    protected: Vec<(PathBuf, Protected)>,
}

impl fmt::Debug for DenyList {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Paths never enter diagnostics (D39).
        formatter
            .debug_struct("DenyList")
            .field("protected", &self.protected.len())
            .finish()
    }
}

impl DenyList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Protect `folder`, which need not exist yet: its deepest existing ancestor is resolved
    /// and the rest kept as given. A relative path names no folder and is ignored.
    pub fn protect(&mut self, kind: Protected, folder: &Path) {
        if !folder.is_absolute() {
            return;
        }
        self.protected.push((resolve_existing_prefix(folder), kind));
    }

    /// Resolve `folder`, following links and junctions, and refuse it if it is a root or
    /// overlaps a protected folder. Returns the canonical folder to grant or open.
    pub fn check(&self, folder: &Path) -> Result<PathBuf, FolderRefusal> {
        let canonical = std::fs::canonicalize(folder).map_err(|_| FolderRefusal::Unresolved)?;
        if !canonical.is_dir() {
            return Err(FolderRefusal::NotAFolder);
        }
        if canonical.parent().is_none() {
            return Err(FolderRefusal::Root);
        }
        if let Some(kind) = self.overlap(&canonical) {
            return Err(FolderRefusal::Protected(kind));
        }
        Ok(canonical)
    }

    /// The first protected folder `canonical` equals or contains, or lies inside when that
    /// is refused too.
    fn overlap(&self, canonical: &Path) -> Option<Protected> {
        self.protected
            .iter()
            .find(|(protected, kind)| {
                protected.starts_with(canonical)
                    || (kind.refuses_inside() && canonical.starts_with(protected))
            })
            .map(|(_, kind)| *kind)
    }
}

/// `path` with its deepest existing ancestor canonicalized and the missing rest appended.
fn resolve_existing_prefix(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            return missing
                .iter()
                .rev()
                .fold(canonical, |joined, part| joined.join(part));
        }
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
            return path.to_path_buf();
        };
        missing.push(name.to_owned());
        existing = parent;
    }
}

#[cfg(test)]
mod tests;

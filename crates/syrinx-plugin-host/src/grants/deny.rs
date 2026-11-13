//! The fixed deny list (note 10, section 3): no folder grant may equal or contain Syrinx's own
//! folders, the system's, a drive root or the user-profile root, or lie inside Syrinx's or
//! the system's folders (inside the profile is allowed). One check serves grant time and every later open, and runs on
//! canonical paths that Syrinx resolved itself.

use std::ffi::OsStr;
use std::fmt;
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf, Prefix};

/// A folder the deny list protects. Folder paths are owner data, so only the kind is ever shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protected {
    /// Syrinx's data folder, which holds its database.
    SyrinxData,
    /// The web engine's user-data folder, which holds the owner's signed-in sessions.
    WebViewData,
    /// `%WINDIR%`.
    System,
    /// `%ProgramFiles%` and its 32-bit twin.
    ProgramFiles,
    /// The root of the owner's user profile. A folder inside it may be granted.
    UserProfile,
}

impl fmt::Display for Protected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SyrinxData => "Syrinx's data folder",
            Self::WebViewData => "the web engine's profile folder",
            Self::System => "the Windows folder",
            Self::ProgramFiles => "the Program Files folder",
            Self::UserProfile => "your user folder",
        })
    }
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
    /// A name for this computer's own drives that a path comparison cannot see through: a
    /// share on this computer, an administrative share, or a device path.
    LocalAlias,
    Protected(Protected),
}

impl fmt::Display for FolderRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unresolved => write!(formatter, "the folder could not be found"),
            Self::NotAFolder => write!(formatter, "it is not a folder"),
            Self::Root => write!(formatter, "a whole drive cannot be granted"),
            Self::LocalAlias => write!(
                formatter,
                "it reaches this computer's drives by a network name"
            ),
            Self::Protected(kind) => write!(formatter, "it overlaps {kind}"),
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
        if is_local_alias(&canonical) {
            return Err(FolderRefusal::LocalAlias);
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

/// Whether a canonical path reaches this computer's drives under a name that never
/// `starts_with` the protected `C:\` forms: a share on this computer (`\\localhost\…`, its
/// own name or a loopback address), any administrative share (`C$`, `ADMIN$`), or a device
/// path (`\\?\GLOBALROOT\…`, `\\.\…`). Only Windows paths carry these prefixes. A share on
/// another computer is left to the owner.
fn is_local_alias(canonical: &Path) -> bool {
    let Some(Component::Prefix(prefix)) = canonical.components().next() else {
        return false;
    };
    match prefix.kind() {
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            is_this_computer(server) || share.to_string_lossy().ends_with('$')
        }
        Prefix::Verbatim(_) | Prefix::DeviceNS(_) => true,
        Prefix::VerbatimDisk(_) | Prefix::Disk(_) => false,
    }
}

/// A server name that is this computer: a loopback name, a loopback address in any spelling
/// (`127.*`, `::1`, an IPv4-mapped `::ffff:127.*`, and Windows' `*.ipv6-literal.net` forms of
/// them), or its own NetBIOS name. Its LAN address and its DNS name are not caught here; only
/// comparing file identities closes those (the PR's "For the owner", item 2).
fn is_this_computer(server: &OsStr) -> bool {
    let server = server.to_string_lossy().to_ascii_lowercase();
    if matches!(server.as_str(), "localhost" | "." | "?") {
        return true;
    }
    if loopback_address(&server) {
        return true;
    }
    std::env::var("COMPUTERNAME").is_ok_and(|name| name.to_ascii_lowercase() == server)
}

/// Whether `server` is a loopback address, written plainly, in brackets, or as a Windows
/// IPv6 literal name (`0--1.ipv6-literal.net`: `-` for `:`, `s` before a zone).
fn loopback_address(server: &str) -> bool {
    let literal = server
        .strip_suffix(".ipv6-literal.net")
        .map(|name| name.replace('-', ":").replace('s', "%"));
    let text = literal.as_deref().unwrap_or(server);
    let text = text.trim_start_matches('[').trim_end_matches(']');
    let text = text.split('%').next().unwrap_or(text);
    match text.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => address.is_loopback(),
        Ok(IpAddr::V6(address)) => {
            address.is_loopback() || address.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        Err(_) => false,
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

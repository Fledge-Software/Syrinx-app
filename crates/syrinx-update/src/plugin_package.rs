//! A plugin package's signed file list (`package`, #134): the update manifest's file rules
//! under its own header, with the plugin's id, version and world inside the signed bytes.

use crate::check_signature;
use crate::error::{ManifestProblem, UpdateError, manifest_error};
use crate::keys::{KeyId, KeySet};
use crate::manifest::{
    FileEntry, MAX_MANIFEST_BYTES, manifest_body, parse_file_lines, parse_version_field,
    push_file_lines,
};
use crate::path::PackagePath;
use crate::payload::{self, Payload, VerifiedPayload};
use crate::signature::{DetachedSignature, SignatureKind};
use crate::version::Version;

/// The signed file list's name in a plugin package folder.
pub const PACKAGE_FILE: &str = "package";
/// Its detached signature, magic `SPP1`.
pub const PACKAGE_SIGNATURE_FILE: &str = "package.sig";
/// Longest plugin id, matching the plugin host's manifest rule.
pub const MAX_PLUGIN_ID_BYTES: usize = 32;

/// Required first line; an update manifest's `syrinx-update-manifest 1` can never pass for it.
const HEADER: &str = "syrinx-plugin-package 1";
const ID_FIELD: &str = "id ";
const VERSION_FIELD: &str = "version ";
const WORLD_FIELD: &str = "world ";
/// Lines before the first `file` line.
const HEADER_LINES: usize = 4;
const WORLD_PREFIX: &str = "syrinx:plugin/";
const MAX_WORLD_BYTES: usize = 128;
/// Precompiled native code; a package never carries it (note 10, section 5).
const PRECOMPILED_EXTENSION: &str = ".cwasm";

/// A parsed `package` list whose bytes are known to be canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPackageList {
    id: String,
    version: Version,
    world: String,
    files: Vec<FileEntry>,
}

impl PluginPackageList {
    /// Builds a list for a release tool or test, sorting files and applying every parse rule.
    pub fn new(
        id: &str,
        version: Version,
        world: &str,
        mut files: Vec<FileEntry>,
    ) -> Result<Self, UpdateError> {
        files.sort_by(|left, right| left.path().as_str().cmp(right.path().as_str()));
        let candidate = Self {
            id: id.to_owned(),
            version,
            world: world.to_owned(),
            files,
        };
        Self::parse(&candidate.to_bytes())
    }

    /// Parses canonical `package` bytes, refusing anything else.
    pub fn parse(bytes: &[u8]) -> Result<Self, UpdateError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(UpdateError::TooLarge {
                what: "plugin package list",
                limit: MAX_MANIFEST_BYTES as u64,
            });
        }
        let body = manifest_body(bytes)?;
        let mut lines = body
            .split('\n')
            .enumerate()
            .map(|(index, line)| (index + 1, line));

        // The header, id, version and world, in their fixed order.
        let header = lines.next().map(|(_, line)| line);
        if header != Some(HEADER) {
            return Err(manifest_error(1, ManifestProblem::Header));
        }
        let id = parse_id(lines.next())?;
        let version = parse_version_field(lines.next(), 3, VERSION_FIELD, "version")?;
        let world = parse_world(lines.next())?;

        let files = parse_file_lines(lines, HEADER_LINES)?;
        refuse_reserved_paths(&files)?;
        let list = Self {
            id,
            version,
            world,
            files,
        };
        if list.to_bytes() != bytes {
            return Err(manifest_error(0, ManifestProblem::NonCanonical));
        }
        Ok(list)
    }

    /// The unique canonical rendering, which is what gets signed.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut text = format!(
            "{HEADER}\n{ID_FIELD}{}\n{VERSION_FIELD}{}\n{WORLD_FIELD}{}\n",
            self.id, self.version, self.world
        );
        push_file_lines(&mut text, &self.files);
        text.into_bytes()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub fn world(&self) -> &str {
        &self.world
    }

    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }
}

fn parse_id(line: Option<(usize, &str)>) -> Result<String, UpdateError> {
    let id = line
        .and_then(|(_, text)| text.strip_prefix(ID_FIELD))
        .ok_or(manifest_error(2, ManifestProblem::Field("id")))?;
    let valid = !id.is_empty()
        && id.len() <= MAX_PLUGIN_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid {
        return Err(manifest_error(2, ManifestProblem::PluginId));
    }
    Ok(id.to_owned())
}

/// A `syrinx:plugin/<world>@<version>` name in lowercase ASCII.
fn parse_world(line: Option<(usize, &str)>) -> Result<String, UpdateError> {
    let world = line
        .and_then(|(_, text)| text.strip_prefix(WORLD_FIELD))
        .ok_or(manifest_error(4, ManifestProblem::Field("world")))?;
    let allowed = |byte: u8| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b':' | b'/' | b'@' | b'.' | b'-')
    };
    let valid = world.len() <= MAX_WORLD_BYTES
        && world.starts_with(WORLD_PREFIX)
        && world.contains('@')
        && world.bytes().all(allowed);
    if !valid {
        return Err(manifest_error(4, ManifestProblem::World));
    }
    Ok(world.to_owned())
}

/// The list may not name its own signed files, nor any precompiled `.cwasm`.
fn refuse_reserved_paths(files: &[FileEntry]) -> Result<(), UpdateError> {
    let reserved = |path: &PackagePath| {
        let folded = path.folded();
        folded == PACKAGE_FILE
            || folded == PACKAGE_SIGNATURE_FILE
            || folded.ends_with(PRECOMPILED_EXTENSION)
    };
    let position = files.iter().position(|entry| reserved(entry.path()));
    match position {
        Some(index) => Err(manifest_error(
            HEADER_LINES + index + 1,
            ManifestProblem::ReservedPath,
        )),
        None => Ok(()),
    }
}

/// A list whose signature and form have been accepted; nothing about the plugin is trusted yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPluginPackage {
    list: PluginPackageList,
    key_id: KeyId,
}

impl VerifiedPluginPackage {
    pub fn list(&self) -> &PluginPackageList {
        &self.list
    }

    /// Which pinned key signed the list; trust on first use records it (#136).
    pub fn key_id(&self) -> KeyId {
        self.key_id
    }

    /// Refuse a package signed for another id than the slot it was found in.
    pub fn check_slot(&self, expected_id: &str) -> Result<(), UpdateError> {
        if self.list.id == expected_id {
            return Ok(());
        }
        Err(UpdateError::WrongPlugin {
            expected: expected_id.to_owned(),
            signed: self.list.id.clone(),
        })
    }

    /// Refuse a version lower than or equal to the installed one (a replay or rollback).
    pub fn check_newer(&self, installed: Version) -> Result<(), UpdateError> {
        if self.list.version > installed {
            return Ok(());
        }
        Err(UpdateError::NotNewer {
            installed,
            offered: self.list.version,
        })
    }

    /// Check that the folder holds exactly the listed files, besides `package` and
    /// `package.sig`, with their sizes and hashes. Install from the returned handles.
    pub fn verify_payload<P: Payload>(
        &self,
        payload: P,
    ) -> Result<VerifiedPayload<PluginPayload<P>>, UpdateError> {
        payload::verify(self.list.files(), PluginPayload { inner: payload })
    }
}

/// Verify `package.sig` before `package` is parsed, against the plugin's pinned keys.
pub fn verify_plugin_package(
    package_bytes: &[u8],
    signature_bytes: &[u8],
    keys: &KeySet,
) -> Result<VerifiedPluginPackage, UpdateError> {
    if package_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::TooLarge {
            what: "plugin package list",
            limit: MAX_MANIFEST_BYTES as u64,
        });
    }
    let signature = DetachedSignature::parse(signature_bytes, SignatureKind::PluginPackage)?;
    let key = keys
        .find(signature.key_id())
        .ok_or(UpdateError::UnknownKey)?;
    check_signature(key, package_bytes, &signature)?;

    // Only signed bytes reach the parser.
    let list = PluginPackageList::parse(package_bytes)?;
    Ok(VerifiedPluginPackage {
        list,
        key_id: signature.key_id(),
    })
}

/// A plugin package folder, whose top level also holds the two signed files.
pub struct PluginPayload<P> {
    inner: P,
}

impl<P: Payload> Payload for PluginPayload<P> {
    type File = P::File;

    fn names(&mut self) -> Result<Vec<String>, UpdateError> {
        let signed = [PACKAGE_FILE, PACKAGE_SIGNATURE_FILE];
        let mut names = self.inner.names()?;
        names.retain(|name| !signed.contains(&name.as_str()));
        Ok(names)
    }

    fn open(&mut self, path: &PackagePath) -> Result<Option<Self::File>, UpdateError> {
        self.inner.open(path)
    }
}

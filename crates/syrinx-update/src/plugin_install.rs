//! Stages a plugin package from a folder the owner chose and places it in
//! `Plugins/<id>/<version>/` (#140): only verified bytes are copied, and a failed step leaves
//! no folder behind. Which publisher may sign is decided here; what the plugin asks for is the
//! plugin host's to judge.

use std::fmt;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use crate::cng;
use crate::directory::DirectoryPayload;
use crate::error::UpdateError;
use crate::install::InstallError;
use crate::install::error::io_error;
use crate::install::files::{
    copy_exact, ensure_plain_directory, entry_present, prepare_destination, remove_update_folder,
    write_new,
};
use crate::keys::{KeySet, PinnedKey};
use crate::manifest::{FileEntry, MAX_MANIFEST_BYTES};
use crate::payload::read_bounded;
use crate::plugin_package::{
    PACKAGE_FILE, PACKAGE_SIGNATURE_FILE, PluginPackageList, read_unsigned_plugin_package,
    verify_new_plugin_package, verify_plugin_package,
};
use crate::signature::SIGNATURE_BYTES;
use crate::version::Version;

/// Installed plugin packages, one folder per id and version, beside the database.
pub const PLUGINS_FOLDER: &str = "Plugins";
/// Where a package is copied and checked before it is placed.
pub const PLUGIN_STAGING_FOLDER: &str = "PluginStaging";

/// How an install record names a publisher's P-256 key: this prefix, then its point in hex.
const KEY_PREFIX: &str = "p256:";
/// How an install record names a development plugin no key signed.
const UNSIGNED_RECORD: &str = "unsigned-development";

/// Who signed a package, as its install record keeps it (trust on first use, decision 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Publisher {
    Key(PinnedKey),
    /// An unsigned development plugin, allowed only in the owner's developer mode and shown
    /// with a persistent "unsigned development plugin" label.
    Unsigned,
}

impl Publisher {
    /// The text an install record's `signer_key` holds.
    pub fn to_record(&self) -> String {
        match self {
            Self::Key(key) => format!("{KEY_PREFIX}{}", hex(key.point())),
            Self::Unsigned => UNSIGNED_RECORD.to_owned(),
        }
    }

    /// Read an install record's `signer_key`; anything else is refused, never guessed.
    pub fn from_record(text: &str) -> Result<Self, PluginInstallError> {
        if text == UNSIGNED_RECORD {
            return Ok(Self::Unsigned);
        }
        let point = text
            .strip_prefix(KEY_PREFIX)
            .and_then(unhex)
            .ok_or(PluginInstallError::RecordedPublisher)?;
        Ok(Self::Key(PinnedKey::new(point)))
    }
}

/// Why a plugin package was not staged or placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginInstallError {
    /// The signature, list or files were refused.
    Package(UpdateError),
    /// A file step failed or found something Syrinx did not make.
    Files(InstallError),
    /// The update is not signed by the key recorded at first install.
    DifferentPublisher,
    /// The folder is unsigned and developer mode is off.
    Unsigned,
    /// A signed first install came without the publisher key the owner chose to trust.
    NoPublisherKey,
    /// The install record's publisher is damaged.
    RecordedPublisher,
    /// This version of the plugin already has a folder.
    AlreadyPlaced { version: Version },
}

impl fmt::Display for PluginInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => write!(formatter, "{error}"),
            Self::Files(error) => write!(formatter, "{error}"),
            Self::DifferentPublisher => {
                formatter.write_str("This update is signed by a different publisher")
            }
            Self::Unsigned => formatter
                .write_str("this plugin is not signed; unsigned plugins need developer mode"),
            Self::NoPublisherKey => formatter.write_str("no publisher key was given to trust"),
            Self::RecordedPublisher => formatter.write_str("the recorded publisher is damaged"),
            Self::AlreadyPlaced { version } => {
                write!(formatter, "version {version} is already in place")
            }
        }
    }
}

impl std::error::Error for PluginInstallError {}

impl From<UpdateError> for PluginInstallError {
    fn from(error: UpdateError) -> Self {
        Self::Package(error)
    }
}

impl From<InstallError> for PluginInstallError {
    fn from(error: InstallError) -> Self {
        Self::Files(error)
    }
}

/// A listed file's bytes and the SHA-256 they were checked against, so an install record
/// names exactly the bytes that load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    pub bytes: Vec<u8>,
    pub sha256: [u8; 32],
}

/// The installed version an update replaces, as its install record names it.
#[derive(Debug, Clone, Copy)]
pub struct Installed<'a> {
    /// The expected slot comes from here, never from the package (#140).
    pub id: &'a str,
    pub version: Version,
    pub publisher: Publisher,
}

/// A folder the owner chose, and what Syrinx already knows about the plugin in it.
pub struct PluginOffer<'a> {
    pub folder: &'a Path,
    /// The install record of the version this updates; `None` for a first install.
    pub installed: Option<Installed<'a>>,
    /// For a first install, the publisher key the owner chose to trust; an update ignores it.
    pub first_use: Option<PinnedKey>,
    /// The owner's developer-mode setting, off by default.
    pub developer_mode: bool,
}

/// The data folder's plugin folders.
#[derive(Debug, Clone)]
pub struct PluginFolders {
    root: PathBuf,
}

impl PluginFolders {
    /// `root` is the local data folder beside the database, under its storage lease.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Where `version` of plugin `id` is installed.
    pub fn version_folder(&self, id: &str, version: Version) -> PathBuf {
        self.plugin_folder(id).join(version.to_string())
    }

    /// Remove every installed version of plugin `id`, as uninstalling does.
    pub fn remove_plugin(&self, id: &str) -> Result<(), PluginInstallError> {
        Ok(remove_update_folder(
            &self.plugin_folder(id),
            "an installed plugin's folder",
        )?)
    }

    /// Remove one version's folder: a declined update, or a version no longer needed.
    pub fn remove_version(&self, id: &str, version: Version) -> Result<(), PluginInstallError> {
        Ok(remove_update_folder(
            &self.version_folder(id, version),
            "an installed plugin version's folder",
        )?)
    }

    /// Remove every version of plugin `id` but `keep`, once `keep` has activated once, so the
    /// previous version stays for a rollback until then. A held newer version is kept too.
    pub fn remove_older_versions(&self, id: &str, keep: Version) -> Result<(), PluginInstallError> {
        for version in self.versions(id)? {
            if version < keep {
                self.remove_version(id, version)?;
            }
        }
        Ok(())
    }

    /// The versions with a folder for plugin `id`; a name that is not a version is ignored.
    pub fn versions(&self, id: &str) -> Result<Vec<Version>, PluginInstallError> {
        const STEP: &str = "listing a plugin's versions";
        let folder = self.plugin_folder(id);
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io_error(STEP)(error).into()),
        };
        let mut versions = Vec::new();
        for entry in entries {
            let name = entry.map_err(io_error(STEP))?.file_name();
            if let Some(version) = name.to_str().and_then(Version::parse) {
                versions.push(version);
            }
        }
        versions.sort();
        Ok(versions)
    }

    fn plugins(&self) -> PathBuf {
        self.root.join(PLUGINS_FOLDER)
    }

    fn plugin_folder(&self, id: &str) -> PathBuf {
        self.plugins().join(id)
    }

    fn staging(&self) -> PathBuf {
        self.root.join(PLUGIN_STAGING_FOLDER)
    }
}

/// A verified copy of a package in `PluginStaging`, removed when dropped unless placed.
pub struct StagedPlugin {
    folder: PathBuf,
    list: PluginPackageList,
    publisher: Publisher,
    /// Set once the folder has moved to its version folder, so dropping removes nothing.
    placed: bool,
}

impl StagedPlugin {
    /// The verified list: the signed id, version and world, and every file's hash.
    pub fn list(&self) -> &PluginPackageList {
        &self.list
    }

    /// Who signed it, as the install record keeps it.
    pub fn publisher(&self) -> Publisher {
        self.publisher
    }

    /// One listed file, read from the staged copy and checked again against the list: the bytes
    /// returned are exactly the ones the hash beside them names.
    pub fn read_listed(&self, path: &str, limit: usize) -> Result<ListedFile, PluginInstallError> {
        read_listed(&self.folder, &self.list, path, limit)
    }

    /// Move the staged copy to `Plugins/<id>/<version>/`. A failure leaves no folder there.
    pub fn place(mut self, folders: &PluginFolders) -> Result<PlacedPlugin, PluginInstallError> {
        let (id, version) = (self.list.id().to_owned(), self.list.version());
        ensure_plain_directory(&folders.plugins(), "the plugins folder")?;
        ensure_plain_directory(&folders.plugin_folder(&id), "a plugin's folder")?;
        let destination = folders.version_folder(&id, version);
        if entry_present(&destination, "inspecting a plugin version's folder")? {
            return Err(PluginInstallError::AlreadyPlaced { version });
        }
        fs::rename(&self.folder, &destination).map_err(io_error("placing a plugin"))?;
        self.placed = true;
        Ok(PlacedPlugin {
            folder: destination,
            list: self.list.clone(),
            publisher: self.publisher,
            kept: false,
        })
    }
}

impl Drop for StagedPlugin {
    fn drop(&mut self) {
        if self.placed {
            return;
        }
        // The owner said no, or a check failed: nothing of the package stays.
        if remove_update_folder(&self.folder, "a staged plugin").is_err() {
            eprintln!("a staged plugin folder could not be removed");
        }
    }
}

/// A package in its version folder, removed when dropped unless `keep` was called, so any
/// failure after placing rolls the folder back.
pub struct PlacedPlugin {
    folder: PathBuf,
    list: PluginPackageList,
    publisher: Publisher,
    kept: bool,
}

impl PlacedPlugin {
    pub fn list(&self) -> &PluginPackageList {
        &self.list
    }

    pub fn publisher(&self) -> Publisher {
        self.publisher
    }

    /// One listed file from the version folder, checked again against the list.
    pub fn read_listed(&self, path: &str, limit: usize) -> Result<ListedFile, PluginInstallError> {
        read_listed(&self.folder, &self.list, path, limit)
    }

    /// The install succeeded, or the update is held for the owner: keep the folder.
    pub fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for PlacedPlugin {
    fn drop(&mut self) {
        if self.kept {
            return;
        }
        if remove_update_folder(&self.folder, "a placed plugin").is_err() {
            eprintln!("a plugin version folder could not be rolled back");
        }
    }
}

/// Verify the package in `offer.folder` against the publisher its record or the owner trusts,
/// then copy exactly the verified bytes into `PluginStaging` and verify the copy.
pub fn stage_plugin(
    folders: &PluginFolders,
    offer: &PluginOffer<'_>,
) -> Result<StagedPlugin, PluginInstallError> {
    let SignedFiles {
        list,
        publisher,
        signature,
    } = verify_signed_files(offer)?;
    ensure_plain_directory(&folders.staging(), "the plugin staging folder")?;
    let folder = folders
        .staging()
        .join(format!("{}-{}", list.id(), list.version()));
    remove_update_folder(&folder, "a leftover staged plugin")?;
    fs::create_dir(&folder).map_err(io_error("creating a staged plugin"))?;
    let staged = StagedPlugin {
        folder,
        list,
        publisher,
        placed: false,
    };
    copy_verified(offer.folder, &staged, signature.as_deref())?;
    staged
        .list
        .verify_payload(DirectoryPayload::open(&staged.folder)?)?;
    Ok(staged)
}

/// A package's checked list, who signed it, and the signature kept beside the copy so the
/// version folder can be checked again later.
struct SignedFiles {
    list: PluginPackageList,
    publisher: Publisher,
    signature: Option<Vec<u8>>,
}

/// The package's list, checked against the publisher it must come from.
fn verify_signed_files(offer: &PluginOffer<'_>) -> Result<SignedFiles, PluginInstallError> {
    let package = read_signed_file(offer.folder, PACKAGE_FILE, MAX_MANIFEST_BYTES)?
        .ok_or(InstallError::NotAPackage)?;
    let signature = read_signed_file(offer.folder, PACKAGE_SIGNATURE_FILE, SIGNATURE_BYTES)?;
    let (list, publisher) = match &signature {
        Some(signature) => verify_signed_package(offer, &package, signature)?,
        None => read_unsigned_package(offer, &package)?,
    };
    Ok(SignedFiles {
        list,
        publisher,
        signature,
    })
}

/// A signed package: an update must come from the recorded key, a first install from the
/// key the owner chose.
fn verify_signed_package(
    offer: &PluginOffer<'_>,
    package: &[u8],
    signature: &[u8],
) -> Result<(PluginPackageList, Publisher), PluginInstallError> {
    let Some(installed) = offer.installed else {
        let key = offer.first_use.ok_or(PluginInstallError::NoPublisherKey)?;
        let keys = KeySet::new(&[key])?;
        let verified = verify_new_plugin_package(package, signature, &keys)?;
        return Ok((verified.list().clone(), Publisher::Key(key)));
    };
    // An unsigned development plugin has no key a signed update could match.
    let Publisher::Key(recorded) = installed.publisher else {
        return Err(PluginInstallError::DifferentPublisher);
    };
    let keys = KeySet::new(&[recorded])?;
    let verified = verify_plugin_package(
        package,
        signature,
        &keys,
        installed.id,
        Some(installed.version),
    )
    .map_err(|error| match error {
        UpdateError::UnknownKey => PluginInstallError::DifferentPublisher,
        other => PluginInstallError::Package(other),
    })?;
    Ok((verified.list().clone(), Publisher::Key(recorded)))
}

/// A folder without `package.sig`: only in developer mode, and never over a signed plugin.
fn read_unsigned_package(
    offer: &PluginOffer<'_>,
    package: &[u8],
) -> Result<(PluginPackageList, Publisher), PluginInstallError> {
    if !offer.developer_mode {
        return Err(PluginInstallError::Unsigned);
    }
    let installed = match offer.installed {
        None => None,
        Some(installed) if installed.publisher == Publisher::Unsigned => {
            Some((installed.id, installed.version))
        }
        Some(_) => return Err(PluginInstallError::DifferentPublisher),
    };
    let list = read_unsigned_plugin_package(package, installed)?;
    Ok((list, Publisher::Unsigned))
}

/// Read `package` or `package.sig` whole and bounded, or `None` when absent.
fn read_signed_file(
    folder: &Path,
    name: &'static str,
    limit: usize,
) -> Result<Option<Vec<u8>>, PluginInstallError> {
    let file = match File::open(folder.join(name)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("reading a plugin package")(error).into()),
    };
    Ok(Some(read_bounded(file, limit, name)?))
}

/// Copy each listed file from the handle that was hashed, then the signed files themselves.
fn copy_verified(
    source: &Path,
    staged: &StagedPlugin,
    signature: Option<&[u8]>,
) -> Result<(), PluginInstallError> {
    const STEP: &str = "copying a plugin into staging";
    let mut payload = staged
        .list
        .verify_payload(DirectoryPayload::open(source)?)?;
    for file in payload.files_mut() {
        let entry = file.entry().clone();
        let destination = prepare_destination(&staged.folder, entry.path(), STEP)?;
        if !copy_exact(file.reader(), &destination, entry.size(), STEP)? {
            return Err(UpdateError::SizeMismatch {
                path: entry.path().as_str().to_owned(),
                expected: entry.size(),
            }
            .into());
        }
    }
    write_new(
        &staged.folder.join(PACKAGE_FILE),
        &staged.list.to_bytes(),
        STEP,
    )?;
    if let Some(signature) = signature {
        write_new(&staged.folder.join(PACKAGE_SIGNATURE_FILE), signature, STEP)?;
    }
    Ok(())
}

/// A listed file read back and checked: its size and SHA-256 must be the list's.
fn read_listed(
    folder: &Path,
    list: &PluginPackageList,
    path: &str,
    limit: usize,
) -> Result<ListedFile, PluginInstallError> {
    let entry = listed(list, path)?;
    let joined = entry
        .path()
        .as_str()
        .split('/')
        .fold(folder.to_path_buf(), |joined, part| joined.join(part));
    let file = File::open(&joined).map_err(io_error("reading an installed plugin file"))?;
    let bytes = read_bounded(file, limit, "an installed plugin file")?;
    let matches = bytes.len() as u64 == entry.size() && &cng::sha256(&bytes)? == entry.sha256();
    if !matches {
        return Err(UpdateError::HashMismatch {
            path: path.to_owned(),
        }
        .into());
    }
    Ok(ListedFile {
        bytes,
        sha256: *entry.sha256(),
    })
}

fn listed<'a>(
    list: &'a PluginPackageList,
    path: &str,
) -> Result<&'a FileEntry, PluginInstallError> {
    list.files()
        .iter()
        .find(|entry| entry.path().as_str() == path)
        .ok_or_else(|| {
            UpdateError::MissingFile {
                path: path.to_owned(),
            }
            .into()
        })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A 64-byte point from 128 lowercase hex digits; anything else is `None`.
fn unhex(text: &str) -> Option<[u8; 64]> {
    let digits = text.as_bytes();
    if digits.len() != 128 {
        return None;
    }
    let mut point = [0u8; 64];
    for (byte, pair) in point.iter_mut().zip(digits.chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        if pair.bytes().any(|digit| digit.is_ascii_uppercase()) {
            return None;
        }
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(point)
}

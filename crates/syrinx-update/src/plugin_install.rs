//! Stages a plugin package from a folder the owner chose and places it in
//! `Plugins/<id>/<version>/` (#140): only verified bytes are copied, a failed step leaves no
//! folder behind, and a placed folder can be verified again later from its own kept files.
//! Whether the right publisher signed is decided here, and only here; what the plugin asks for
//! is the plugin host's to judge.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::cng;
use crate::directory::DirectoryPayload;
use crate::error::UpdateError;
use crate::install::InstallError;
use crate::install::error::io_error;
use crate::install::files::{
    copy_exact, ensure_plain_directory, prepare_destination, remove_update_folder, write_new,
};
use crate::keys::{KeySet, PinnedKey};
use crate::manifest::{FileEntry, MAX_MANIFEST_BYTES};
use crate::path::PackagePath;
use crate::payload::{Payload, read_bounded};
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

/// Why a plugin package was not staged, placed or found again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginInstallError {
    /// The signature, list or files were refused.
    Package(UpdateError),
    /// A file step failed or found something Syrinx did not make.
    Files(InstallError),
    /// The package is not signed by the publisher its install record names.
    DifferentPublisher,
    /// The folder is unsigned and developer mode is off.
    Unsigned,
    /// A signed first install came without the publisher key the owner chose to trust.
    NoPublisherKey,
    /// The install record's publisher is damaged.
    RecordedPublisher,
    /// A new plugin's id already has an installed version's folder.
    AlreadyInstalled,
    /// An update would replace the installed version's own folder.
    InstalledVersion,
    /// An update's copy was handed to the new-plugin path.
    NotANewPlugin,
    /// Another install or update of this plugin is still under way.
    InstallInProgress,
    /// An installed version's folder holds another version than its record names.
    NotRecordedVersion { recorded: Version, found: Version },
}

impl fmt::Display for PluginInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => write!(formatter, "{error}"),
            Self::Files(error) => write!(formatter, "{error}"),
            // The one place this owner-facing sentence is written.
            Self::DifferentPublisher => {
                formatter.write_str("This update is signed by a different publisher")
            }
            Self::Unsigned => formatter
                .write_str("this plugin is not signed; unsigned plugins need developer mode"),
            Self::NoPublisherKey => formatter.write_str("no publisher key was given to trust"),
            Self::RecordedPublisher => formatter.write_str("the recorded publisher is damaged"),
            Self::AlreadyInstalled => formatter.write_str("this plugin is already installed"),
            Self::InstalledVersion => formatter.write_str("this version is the installed one"),
            Self::NotANewPlugin => formatter.write_str("this package updates an installed plugin"),
            Self::InstallInProgress => {
                formatter.write_str("this plugin is already being installed or updated")
            }
            Self::NotRecordedVersion { recorded, found } => write!(
                formatter,
                "the folder for version {recorded} holds version {found}"
            ),
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

/// A listed file's bytes and the SHA-256 they were checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    pub bytes: Vec<u8>,
    pub sha256: [u8; 32],
}

/// An installed version, as its install record names it.
#[derive(Debug, Clone, Copy)]
pub struct Installed<'a> {
    /// The expected slot comes from here, never from the package (#140).
    pub id: &'a str,
    pub version: Version,
    pub publisher: Publisher,
}

/// What a package is checked against.
#[derive(Debug, Clone, Copy)]
pub enum Basis<'a> {
    /// A plugin new to Syrinx, with the publisher key the owner chose to trust if the folder is
    /// signed (the package format carries only the key's identifier).
    New { trusted: Option<PinnedKey> },
    /// An update of the installed version: the same id, a strictly higher version, and the
    /// recorded publisher.
    Update(Installed<'a>),
}

/// A folder the owner chose, what it is checked against, and the owner's developer mode.
pub struct PluginOffer<'a> {
    pub folder: &'a Path,
    pub basis: Basis<'a>,
    /// The owner's developer-mode setting, off by default.
    pub developer_mode: bool,
}

/// What a folder must prove, as `check_package_files` reads it.
#[derive(Clone, Copy)]
enum Expect<'a> {
    New(Option<PinnedKey>),
    Newer(Installed<'a>),
    /// The installed version itself, found again at start-up.
    Exactly(Installed<'a>),
}

impl<'a> Expect<'a> {
    /// The install record checked against; `None` for a new plugin.
    fn installed(self) -> Option<Installed<'a>> {
        match self {
            Self::New(_) => None,
            Self::Newer(installed) | Self::Exactly(installed) => Some(installed),
        }
    }

    /// The version the package must be newer than: only an update's.
    fn older_than(self) -> Option<Version> {
        match self {
            Self::Newer(installed) => Some(installed.version),
            Self::New(_) | Self::Exactly(_) => None,
        }
    }
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

    /// Remove every version of plugin `id` older than `keep`, once `keep` has activated once,
    /// so the previous version stays for a rollback until then. A held newer version stays.
    pub fn remove_older_versions(&self, id: &str, keep: Version) -> Result<(), PluginInstallError> {
        for version in self.versions(id)? {
            if version < keep {
                self.remove_version(id, version)?;
            }
        }
        Ok(())
    }

    /// The versions with a folder for plugin `id`, oldest first; other names are ignored.
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

/// One install flow's exclusive hold on a plugin id, from staging until the placed copy is kept
/// or dropped, so two installs or updates of one plugin never overlap: what the host reviewed
/// stays true until the copy is placed. Syrinx runs as one instance (its instance lock), so a
/// guard within the process suffices. Holds are keyed by the plugin's folder, so separate data
/// folders never block each other.
struct InstallGuard {
    key: PathBuf,
}

/// The plugin folders whose ids an install flow holds.
static HELD: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

impl InstallGuard {
    fn take(folders: &PluginFolders, id: &str) -> Result<Self, PluginInstallError> {
        let key = folders.plugin_folder(id);
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        if !held.insert(key.clone()) {
            return Err(PluginInstallError::InstallInProgress);
        }
        Ok(Self { key })
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        held.remove(&self.key);
    }
}

/// A verified copy of a package in `PluginStaging`, removed when dropped unless placed.
pub struct StagedPlugin {
    folder: PathBuf,
    list: PluginPackageList,
    publisher: Publisher,
    /// The installed version an update replaces; `None` for a new plugin.
    installed: Option<Version>,
    /// Set once the folder has moved to its version folder, so dropping removes nothing.
    placed: bool,
    /// This flow's hold on the plugin id, handed to the placed copy.
    guard: Option<InstallGuard>,
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

    /// Place a new plugin's copy once the plugin host found no install record for its id
    /// (`OwnerRuntime::review_new`). Any `Plugins/<id>/` already there was left by a crash
    /// between placing and recording, so it is removed first and never blocks the install.
    /// This flow's install guard keeps another install of the id from recording meanwhile.
    pub fn place_unrecorded(
        self,
        folders: &PluginFolders,
    ) -> Result<PlacedPlugin, PluginInstallError> {
        if self.installed.is_some() {
            return Err(PluginInstallError::NotANewPlugin);
        }
        folders.remove_plugin(self.list.id())?;
        self.place(folders)
    }

    /// Move an update's staged copy to `Plugins/<id>/<version>/`; a new plugin goes through
    /// `place_unrecorded` instead.
    ///
    /// A folder an earlier offer of this same update left is replaced. The installed version's
    /// folder is never touched: a new plugin whose id already has a version folder is refused,
    /// and so is an update whose destination is the installed version's. A failure leaves no
    /// folder there.
    pub fn place(mut self, folders: &PluginFolders) -> Result<PlacedPlugin, PluginInstallError> {
        let (id, version) = (self.list.id().to_owned(), self.list.version());
        match self.installed {
            None if !folders.versions(&id)?.is_empty() => {
                return Err(PluginInstallError::AlreadyInstalled);
            }
            // Defence in depth: staging already refuses an update that is not newer.
            Some(installed) if installed == version => {
                return Err(PluginInstallError::InstalledVersion);
            }
            _ => {}
        }
        ensure_plain_directory(&folders.plugins(), "the plugins folder")?;
        ensure_plain_directory(&folders.plugin_folder(&id), "a plugin's folder")?;
        let destination = folders.version_folder(&id, version);
        remove_update_folder(&destination, "an earlier offer of this version")?;
        fs::rename(&self.folder, &destination).map_err(io_error("placing a plugin"))?;
        self.placed = true;
        Ok(PlacedPlugin {
            folder: destination,
            list: self.list.clone(),
            publisher: self.publisher,
            kept: false,
            _guard: self.guard.take(),
        })
    }
}

impl Drop for StagedPlugin {
    fn drop(&mut self) {
        if self.placed {
            return;
        }
        // The owner said no, or a check failed: nothing of the package stays.
        if let Err(error) = remove_update_folder(&self.folder, "a staged plugin") {
            eprintln!("a staged plugin folder could not be removed: {error}");
        }
    }
}

/// A package in its version folder. One just placed is removed when dropped unless `keep` was
/// called, so any failure after placing rolls the folder back; one found again is kept.
pub struct PlacedPlugin {
    folder: PathBuf,
    list: PluginPackageList,
    publisher: Publisher,
    kept: bool,
    /// The install flow's hold on the plugin id, held only to be released when this is kept
    /// or dropped; `None` for a folder found again.
    _guard: Option<InstallGuard>,
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

    /// The install succeeded, or the update is held for the owner: keep the folder, and end the
    /// install flow.
    pub fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for PlacedPlugin {
    fn drop(&mut self) {
        if self.kept {
            return;
        }
        if let Err(error) = remove_update_folder(&self.folder, "a placed plugin") {
            eprintln!("a plugin version folder could not be rolled back: {error}");
        }
    }
}

/// Verify the package in `offer.folder` against its basis, then copy exactly the verified bytes
/// into `PluginStaging` and verify the copy.
pub fn stage_plugin(
    folders: &PluginFolders,
    offer: &PluginOffer<'_>,
) -> Result<StagedPlugin, PluginInstallError> {
    let expect = match offer.basis {
        Basis::New { trusted } => Expect::New(trusted),
        Basis::Update(installed) => Expect::Newer(installed),
    };
    let mut source = DirectoryPayload::open(offer.folder)?;
    let files = check_package_files(&mut source, expect, offer.developer_mode)?;
    let guard = InstallGuard::take(folders, files.list.id())?;
    ensure_plain_directory(&folders.staging(), "the plugin staging folder")?;
    let folder = folders
        .staging()
        .join(format!("{}-{}", files.list.id(), files.list.version()));
    remove_update_folder(&folder, "a leftover staged plugin")?;
    fs::create_dir(&folder).map_err(io_error("creating a staged plugin"))?;
    let staged = StagedPlugin {
        folder,
        list: files.list,
        publisher: files.publisher,
        installed: expect.installed().map(|installed| installed.version),
        placed: false,
        guard: Some(guard),
    };
    copy_verified(source, &staged, files.signature.as_deref())?;
    staged
        .list
        .verify_payload(DirectoryPayload::open(&staged.folder)?)?;
    Ok(staged)
}

/// The installed version's folder, verified again from its kept files against its record, so
/// Syrinx loads at start-up only what the record names.
pub fn open_installed(
    folders: &PluginFolders,
    installed: Installed<'_>,
    developer_mode: bool,
) -> Result<PlacedPlugin, PluginInstallError> {
    let folder = folders.version_folder(installed.id, installed.version);
    verify_placed(folder, Expect::Exactly(installed), developer_mode)
}

/// The update waiting for the owner, if any, with the waiting folders settled: the newest
/// version folder newer than the record that still verifies against the recorded publisher.
/// It survives a restart because it is its own signed folder; nothing about it is recorded
/// until the owner accepts. A newer folder whose contents are refused (signature, hash, slot,
/// version or publisher) is removed, and so is every older waiting folder once a newer one is
/// offered, so none is left stuck. An unsigned folder while developer mode is off is kept and
/// skipped: an older waiting folder may then be offered instead. A failure to read or check a
/// folder is returned and removes nothing.
pub fn settle_pending_update(
    folders: &PluginFolders,
    installed: Installed<'_>,
    developer_mode: bool,
) -> Result<Option<PlacedPlugin>, PluginInstallError> {
    let mut newer: Vec<Version> = folders
        .versions(installed.id)?
        .into_iter()
        .filter(|version| *version > installed.version)
        .collect();
    let mut pending = None;
    while let Some(version) = newer.pop() {
        if pending.is_some() {
            folders.remove_version(installed.id, version)?;
            continue;
        }
        let folder = folders.version_folder(installed.id, version);
        match verify_placed(folder, Expect::Newer(installed), developer_mode) {
            Ok(placed) => pending = Some(placed),
            Err(PluginInstallError::Unsigned) => {}
            Err(error) if is_content_refusal(&error) => {
                folders.remove_version(installed.id, version)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(pending)
}

/// Whether a folder was refused for what it holds, so it can never be offered, rather than
/// for a failure to read it.
fn is_content_refusal(error: &PluginInstallError) -> bool {
    match error {
        PluginInstallError::Package(error) => verifier_refused(error),
        PluginInstallError::DifferentPublisher | PluginInstallError::NotRecordedVersion { .. } => {
            true
        }
        PluginInstallError::Files(_)
        | PluginInstallError::Unsigned
        | PluginInstallError::NoPublisherKey
        | PluginInstallError::RecordedPublisher
        | PluginInstallError::AlreadyInstalled
        | PluginInstallError::InstalledVersion
        | PluginInstallError::NotANewPlugin
        | PluginInstallError::InstallInProgress => false,
    }
}

/// Whether the verifier refused what a folder holds (its signature, list, files, slot or
/// version) rather than failing to read or check it. Every variant is named, so a new one is
/// classified before it compiles.
fn verifier_refused(error: &UpdateError) -> bool {
    match error {
        UpdateError::TooLarge { .. }
        | UpdateError::SignatureLength { .. }
        | UpdateError::SignatureFormat
        | UpdateError::UnknownKey
        | UpdateError::BadSignature
        | UpdateError::Manifest { .. }
        | UpdateError::NotNewer { .. }
        | UpdateError::InstalledTooOld { .. }
        | UpdateError::UnlistedFile { .. }
        | UpdateError::MissingFile { .. }
        | UpdateError::SizeMismatch { .. }
        | UpdateError::HashMismatch { .. }
        | UpdateError::UnsafeEntry { .. }
        | UpdateError::WrongPlugin { .. } => true,
        // The environment, a key, or the update statement's rules, which a plugin folder
        // never reaches: report, never delete.
        UpdateError::Read { .. }
        | UpdateError::Crypto { .. }
        | UpdateError::InvalidKey { .. }
        | UpdateError::DuplicateKey
        | UpdateError::Statement { .. }
        | UpdateError::StatementExpired { .. }
        | UpdateError::StatementIssuedAhead { .. }
        | UpdateError::StatementReplayed { .. }
        | UpdateError::StatementSerialReused { .. }
        | UpdateError::ManifestNotStated
        | UpdateError::StatementVersionMismatch { .. }
        | UpdateError::Clock
        | UpdateError::UnsuitableSigningKey => false,
    }
}

/// A version folder checked in place against `expect`; it is kept when dropped.
fn verify_placed(
    folder: PathBuf,
    expect: Expect<'_>,
    developer_mode: bool,
) -> Result<PlacedPlugin, PluginInstallError> {
    let mut payload = DirectoryPayload::open(&folder)?;
    let files = check_package_files(&mut payload, expect, developer_mode)?;
    files.list.verify_payload(payload)?;
    Ok(PlacedPlugin {
        folder,
        list: files.list,
        publisher: files.publisher,
        kept: true,
        _guard: None,
    })
}

/// A package's checked list, who signed it, and the signature kept beside the copy so the
/// version folder can be checked again later.
struct PackageFiles {
    list: PluginPackageList,
    publisher: Publisher,
    signature: Option<Vec<u8>>,
}

/// Read `package` and `package.sig` through the folder's held handles and check them.
fn check_package_files<P: Payload>(
    payload: &mut P,
    expect: Expect<'_>,
    developer_mode: bool,
) -> Result<PackageFiles, PluginInstallError> {
    let package = read_package_file(payload, PACKAGE_FILE, MAX_MANIFEST_BYTES)?
        .ok_or(InstallError::NotAPackage)?;
    let signature = read_package_file(payload, PACKAGE_SIGNATURE_FILE, SIGNATURE_BYTES)?;
    let (list, publisher) = match &signature {
        Some(signature) => check_signed(&package, signature, expect)?,
        None => check_unsigned(&package, expect, developer_mode)?,
    };
    if let Expect::Exactly(installed) = expect {
        check_recorded_version(&list, installed)?;
    }
    Ok(PackageFiles {
        list,
        publisher,
        signature,
    })
}

/// A signed package: a new plugin's must come from the key the owner chose, an installed one's
/// from the key its record names.
fn check_signed(
    package: &[u8],
    signature: &[u8],
    expect: Expect<'_>,
) -> Result<(PluginPackageList, Publisher), PluginInstallError> {
    let Some(installed) = expect.installed() else {
        return check_signed_new(package, signature, expect);
    };
    // An unsigned development plugin has no key a signed package could match.
    let Publisher::Key(recorded) = installed.publisher else {
        return Err(PluginInstallError::DifferentPublisher);
    };
    let keys = KeySet::new(&[recorded])?;
    let older = expect.older_than();
    let verified =
        verify_plugin_package(package, signature, &keys, installed.id, older).map_err(|error| {
            match error {
                UpdateError::UnknownKey => PluginInstallError::DifferentPublisher,
                other => PluginInstallError::Package(other),
            }
        })?;
    Ok((verified.list().clone(), Publisher::Key(recorded)))
}

/// A new plugin's signed package, which must come from the key the owner chose to trust.
fn check_signed_new(
    package: &[u8],
    signature: &[u8],
    expect: Expect<'_>,
) -> Result<(PluginPackageList, Publisher), PluginInstallError> {
    let Expect::New(Some(key)) = expect else {
        return Err(PluginInstallError::NoPublisherKey);
    };
    let verified = verify_new_plugin_package(package, signature, &KeySet::new(&[key])?)?;
    Ok((verified.list().clone(), Publisher::Key(key)))
}

/// A folder without `package.sig`: only in developer mode, and never for a signed plugin.
fn check_unsigned(
    package: &[u8],
    expect: Expect<'_>,
    developer_mode: bool,
) -> Result<(PluginPackageList, Publisher), PluginInstallError> {
    if !developer_mode {
        return Err(PluginInstallError::Unsigned);
    }
    let signed_before = expect
        .installed()
        .is_some_and(|installed| installed.publisher != Publisher::Unsigned);
    if signed_before {
        return Err(PluginInstallError::DifferentPublisher);
    }
    let rules = expect
        .installed()
        .zip(expect.older_than())
        .map(|(installed, older)| (installed.id, older));
    let list = read_unsigned_plugin_package(package, rules)?;
    Ok((list, Publisher::Unsigned))
}

/// The installed version's folder must hold that plugin at exactly that version.
fn check_recorded_version(
    list: &PluginPackageList,
    installed: Installed<'_>,
) -> Result<(), PluginInstallError> {
    if list.id() != installed.id {
        return Err(UpdateError::WrongPlugin {
            expected: installed.id.to_owned(),
            signed: list.id().to_owned(),
        }
        .into());
    }
    if list.version() != installed.version {
        return Err(PluginInstallError::NotRecordedVersion {
            recorded: installed.version,
            found: list.version(),
        });
    }
    Ok(())
}

/// Read `package` or `package.sig` whole and bounded through a held handle, or `None`.
fn read_package_file<P: Payload>(
    payload: &mut P,
    name: &'static str,
    limit: usize,
) -> Result<Option<Vec<u8>>, PluginInstallError> {
    let path = PackagePath::parse(name).map_err(|_| InstallError::NotAPackage)?;
    let Some(file) = payload.open(&path)? else {
        return Ok(None);
    };
    Ok(Some(read_bounded(file, limit, name)?))
}

/// Copy each listed file from the handle that was hashed, then the signed files themselves.
fn copy_verified(
    source: DirectoryPayload,
    staged: &StagedPlugin,
    signature: Option<&[u8]>,
) -> Result<(), PluginInstallError> {
    const STEP: &str = "copying a plugin into staging";
    let mut payload = staged.list.verify_payload(source)?;
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

/// A listed file read back through a held handle and checked: its size and SHA-256 must be
/// the list's.
fn read_listed(
    folder: &Path,
    list: &PluginPackageList,
    path: &str,
    limit: usize,
) -> Result<ListedFile, PluginInstallError> {
    let entry = listed(list, path)?;
    let mut payload = DirectoryPayload::open(folder)?;
    let file = payload
        .open(entry.path())?
        .ok_or_else(|| UpdateError::MissingFile {
            path: path.to_owned(),
        })?;
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

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn only_refused_contents_remove_a_waiting_folder() {
        let kept = [
            UpdateError::Read {
                what: "synthetic".into(),
                kind: io::ErrorKind::PermissionDenied,
            },
            UpdateError::Crypto {
                operation: "synthetic",
                status: -1,
            },
            UpdateError::InvalidKey { status: -1 },
        ];
        for error in kept {
            let refusal = PluginInstallError::Package(error);
            assert!(!is_content_refusal(&refusal), "{refusal:?}");
        }
        let removed = [
            UpdateError::HashMismatch {
                path: "plugin.wasm".into(),
            },
            UpdateError::BadSignature,
        ];
        for error in removed {
            let refusal = PluginInstallError::Package(error);
            assert!(is_content_refusal(&refusal), "{refusal:?}");
        }
        assert!(is_content_refusal(&PluginInstallError::DifferentPublisher));
        assert!(!is_content_refusal(&PluginInstallError::Unsigned));
    }
}

//! Installing and uninstalling a plugin from a package the application verified (#140): what an
//! offer is, which ids a new plugin may take, what Syrinx version it needs, and whether an
//! update waits for the owner because it asks for more (decisions 9 to 12). Applying an update
//! is `update.rs`'s.

use std::fmt;
use std::sync::Arc;

use syrinx_core::CommandRegistry;
use syrinx_store::{PluginInstall, PluginState, StoreError};

use super::cache::ABC_SHA256;
use super::{OwnerError, OwnerRuntime, Sha256};
use crate::HostError;
use crate::identity::{IdentityMismatch, SignedIdentity, SignedManifest, check_signed};
use crate::manifest::{
    CapabilityChanges, CapabilityKind, DiscoveryOptions, ManifestError, PluginManifest, compare,
};
use crate::registry::{PluginRegistry, RegistrationError, is_reserved_id};
use crate::worker::PluginEvent;

/// The built-in plugins the spec names. Syrinx's release will bundle their packages (research
/// note 10, "Loading"); until then their ids are still kept from plugins installed from a folder.
pub const BUILT_IN_PLUGINS: [&str; 3] = ["notes", "forge", "fledge"];

/// The publisher an install record names for an unsigned development plugin. It must equal
/// `syrinx_update::Publisher::Unsigned.to_record()`; a test in `syrinx-app` keeps them equal.
pub const UNSIGNED_PUBLISHER: &str = "unsigned-development";

/// The SHA-256 function offers are hashed with, checked once against FIPS 180's answer for
/// `abc`. The check catches a wrong function passed by mistake, not a hostile one: the
/// application that passes it is trusted either way.
#[derive(Clone, Copy)]
pub struct Hasher(Sha256);

impl Hasher {
    pub fn new(sha256: Sha256) -> Result<Self, InstallRefusal> {
        if sha256(b"abc").map_err(InstallRefusal::Hash)? != ABC_SHA256 {
            return Err(InstallRefusal::Hash(
                "the function given is not SHA-256".into(),
            ));
        }
        Ok(Self(sha256))
    }

    fn digest(self, bytes: &[u8]) -> Result<[u8; 32], InstallRefusal> {
        (self.0)(bytes).map_err(InstallRefusal::Hash)
    }
}

/// A package `syrinx-update` staged and verified, as the host needs it: the verified lines, the
/// manifest and component bytes, and the publisher as the install record keeps it. The host
/// hashes the bytes itself, so the record names exactly the bytes that load.
pub struct Offer {
    pub(super) signed: SignedIdentity,
    pub(super) manifest: Vec<u8>,
    pub(super) manifest_sha256: [u8; 32],
    pub(super) component: Arc<[u8]>,
    pub(super) module_sha256: [u8; 32],
    pub(super) publisher: String,
}

impl Offer {
    pub fn new(
        signed: SignedIdentity,
        manifest: Vec<u8>,
        component: Arc<[u8]>,
        publisher: String,
        hasher: Hasher,
    ) -> Result<Self, InstallRefusal> {
        Ok(Self {
            manifest_sha256: hasher.digest(&manifest)?,
            module_sha256: hasher.digest(&component)?,
            signed,
            manifest,
            component,
            publisher,
        })
    }

    /// An unsigned development plugin, read only in the owner's developer mode.
    fn unsigned(&self) -> bool {
        self.publisher == UNSIGNED_PUBLISHER
    }

    /// The manifest, parsed and checked against the signed lines.
    fn vouched(&self) -> Result<SignedManifest, InstallRefusal> {
        let options = DiscoveryOptions {
            developer_mode: self.unsigned(),
        };
        let manifest =
            PluginManifest::parse(&self.manifest, options).map_err(InstallRefusal::Manifest)?;
        check_signed(&self.signed, manifest).map_err(InstallRefusal::Identity)
    }
}

/// What installing an offer would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// A plugin new to Syrinx; the owner confirms the id it claims.
    Install,
    /// An update that asks for nothing new: it replaces the installed version, and its grants
    /// are rescoped to what it asks for.
    Update(CapabilityChanges),
    /// An update that asks for more: held, while the installed version keeps running, until
    /// the owner accepts the difference.
    Held(CapabilityChanges),
}

/// The installed version an update was reviewed against: its record and its component, read
/// again from its own folder.
pub(super) struct Base {
    pub(super) record: PluginInstall,
    pub(super) component: Arc<[u8]>,
}

/// An offer whose manifest matches its signed lines and passed every install rule, with the
/// installed version an update was compared against.
pub struct Reviewed {
    pub(super) signed: SignedManifest,
    pub(super) offer: Offer,
    pub(super) plan: Plan,
    pub(super) base: Option<Base>,
}

impl Reviewed {
    /// What the review sheet shows: the manifest the signed lines vouch for.
    pub fn manifest(&self) -> &PluginManifest {
        self.signed.manifest()
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// The owner accepted a held update's difference, so it may now replace the installed one.
    pub fn accept(mut self) -> Self {
        if let Plan::Held(changes) = self.plan {
            self.plan = Plan::Update(changes);
        }
        self
    }
}

/// Why an offer was not installed. Nothing of it was kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallRefusal {
    Manifest(ManifestError),
    Identity(IdentityMismatch),
    /// The function given to hash the offer is not SHA-256, or failed.
    Hash(String),
    /// The id belongs to Syrinx, to a built-in plugin or to an installed plugin.
    IdTaken(String),
    /// Third-party plugins get no full-trust capability in v1 (decision 10).
    FullTrust(CapabilityKind),
    /// The plugin's `min-syrinx` is newer than this Syrinx.
    NeedsNewerSyrinx(String),
    /// The offer names another publisher than the install record, so it was not checked
    /// against that record.
    NotCheckedAgainstRecord,
    /// The installed files are not the ones the install record names.
    NotRecorded,
    /// The update is for another plugin than the one being updated.
    WrongPlugin {
        expected: String,
        offered: String,
    },
    /// The install record changed since the update was reviewed.
    Stale,
    /// The update is not newer than the installed version.
    NotNewer,
    /// A required capability was not granted, so the plugin cannot be installed.
    RequiredDeclined(CapabilityKind),
    /// The update asks for more and the owner has not accepted the difference.
    NeedsConsent,
    /// The offer was reviewed as an update but installed as new, or the reverse.
    WrongPlan,
    Registration(RegistrationError),
    Owner(OwnerError),
}

impl fmt::Display for InstallRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(error) => write!(formatter, "{error}"),
            Self::Identity(error) => write!(formatter, "{error}"),
            Self::Hash(error) => write!(formatter, "the package could not be hashed: {error}"),
            Self::IdTaken(id) => write!(formatter, "the plugin id {id} is already taken"),
            Self::FullTrust(kind) => write!(
                formatter,
                "the plugin asks for {kind}, which can run programs on your computer, and \
                 plugins installed from a folder cannot have that yet"
            ),
            Self::NeedsNewerSyrinx(version) => {
                write!(formatter, "This plugin needs Syrinx {version} or newer")
            }
            Self::NotCheckedAgainstRecord => {
                formatter.write_str("the package was not checked against this plugin's record")
            }
            Self::NotRecorded => {
                formatter.write_str("the installed files are not the ones Syrinx recorded")
            }
            Self::WrongPlugin { expected, offered } => {
                write!(formatter, "this is {offered}, not an update to {expected}")
            }
            Self::Stale => {
                formatter.write_str("the installed plugin changed since this update was reviewed")
            }
            Self::NotNewer => formatter.write_str("the update is not newer than the installed one"),
            Self::RequiredDeclined(kind) => {
                write!(formatter, "the plugin cannot run without {kind}")
            }
            Self::NeedsConsent => formatter.write_str("the update waits for you to accept it"),
            Self::WrongPlan => formatter.write_str("the offer was reviewed for another step"),
            Self::Registration(error) => write!(formatter, "{error}"),
            Self::Owner(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for InstallRefusal {}

impl From<OwnerError> for InstallRefusal {
    fn from(error: OwnerError) -> Self {
        Self::Owner(error)
    }
}

impl From<StoreError> for InstallRefusal {
    fn from(error: StoreError) -> Self {
        Self::Owner(OwnerError::Store(error))
    }
}

impl From<HostError> for InstallRefusal {
    fn from(error: HostError) -> Self {
        Self::Owner(OwnerError::Host(error))
    }
}

impl From<RegistrationError> for InstallRefusal {
    fn from(error: RegistrationError) -> Self {
        Self::Registration(error)
    }
}

impl OwnerRuntime {
    /// Check an offer without running anything: its manifest against its signed lines, the
    /// Syrinx version it needs and its capabilities. For a new plugin (`base` is `None`) its id
    /// must be free. For an update, `base` is the installed version read again from its own
    /// folder: it must be exactly what the install record names, and the offer is compared
    /// with it, whether or not that version is running.
    pub fn review(&self, offer: Offer, base: Option<Offer>) -> Result<Reviewed, InstallRefusal> {
        let signed = offer.vouched()?;
        self.check_installable(signed.manifest())?;
        let Some(base) = base else {
            self.check_new_id(signed.manifest().id())?;
            return Ok(Reviewed {
                signed,
                offer,
                plan: Plan::Install,
                base: None,
            });
        };
        let (plan, base) = self.plan_update(&offer, signed.manifest(), base)?;
        Ok(Reviewed {
            signed,
            offer,
            plan,
            base: Some(base),
        })
    }

    /// Register and load an installed plugin at start-up from its version folder, once the
    /// offer is exactly what its install record names (#140, comment of 2026-11-13).
    pub fn load_installed(
        &mut self,
        offer: Offer,
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
    ) -> Result<(), InstallRefusal> {
        let signed = offer.vouched()?;
        let id = signed.manifest().id().to_owned();
        self.recorded(&offer, signed.manifest())?;
        let registered = registry.register(signed, commands)?;
        if let Err(error) = self.load(registered, offer.component) {
            registry.withdraw(&id, commands);
            return Err(error.into());
        }
        Ok(())
    }

    /// Install a reviewed new plugin with the capabilities the owner granted on the sheet,
    /// every required one among them. A failure leaves no record, grant or registration.
    pub fn install_reviewed(
        &mut self,
        reviewed: Reviewed,
        granted: &[CapabilityKind],
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
        now: i64,
    ) -> Result<(), InstallRefusal> {
        if reviewed.plan != Plan::Install {
            return Err(InstallRefusal::WrongPlan);
        }
        check_required(reviewed.manifest(), granted, |_| true)?;
        let Reviewed { signed, offer, .. } = reviewed;
        let id = signed.manifest().id().to_owned();
        let registered = registry.register(signed, commands)?;
        let record = install_record(&id, &offer, registered.manifest(), now, now);
        let installed = self
            .install(registered, Arc::clone(&offer.component), record)
            .and_then(|()| self.grant_all(&id, granted, now));
        if let Err(error) = installed {
            self.roll_back_install(&id, registry, commands);
            return Err(error.into());
        }
        Ok(())
    }

    /// Uninstall a plugin: withdraw its commands, tab types and feeds, stop it, and delete its
    /// record, grants, folders, settings and compiled code. Its package folders are the
    /// application's to delete.
    pub fn uninstall(
        &mut self,
        plugin: &str,
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
    ) -> Result<(), OwnerError> {
        // A plugin not registered this session has nothing to withdraw.
        registry.withdraw(plugin, commands);
        self.remove(plugin)
    }

    /// A background compile refused a plugin's component (#134's check at load): stop it and
    /// withdraw its commands, tab types and feeds in one step, so nothing of it stays
    /// runnable. Its install record stays, so the owner can update or uninstall it. Returns
    /// the plugin withdrawn; any other event withdraws nothing.
    pub fn withdraw_refused(
        &mut self,
        event: &PluginEvent,
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
    ) -> Option<String> {
        let PluginEvent::PrepareFailed { plugin, error } = event else {
            return None;
        };
        if !is_refusal(error) {
            return None;
        }
        registry.withdraw(plugin, commands)?;
        if let Err(error) = self.stop(plugin) {
            eprintln!("a refused plugin could not be stopped: {error}");
        }
        Some(plugin.clone())
    }

    /// The rules every offer meets: the Syrinx version it needs, and no full-trust kind.
    fn check_installable(&self, manifest: &PluginManifest) -> Result<(), InstallRefusal> {
        let needed = &manifest.plugin().min_syrinx;
        if !meets(&self.host_version, needed) {
            return Err(InstallRefusal::NeedsNewerSyrinx(needed.clone()));
        }
        let full_trust = manifest
            .capabilities()
            .iter()
            .find(|request| request.kind.is_full_trust());
        if let Some(request) = full_trust {
            return Err(InstallRefusal::FullTrust(request.kind));
        }
        Ok(())
    }

    /// A new plugin may not take Syrinx's ids, a built-in's, or an installed plugin's, loaded
    /// this session or not.
    fn check_new_id(&self, id: &str) -> Result<(), InstallRefusal> {
        let taken =
            is_reserved_id(id) || BUILT_IN_PLUGINS.contains(&id) || self.record(id)?.is_some();
        if taken {
            return Err(InstallRefusal::IdTaken(id.to_owned()));
        }
        Ok(())
    }

    /// An update: the installed version as its record names it, the same plugin, staged
    /// against that record, newer, and compared with it.
    fn plan_update(
        &self,
        offer: &Offer,
        manifest: &PluginManifest,
        base: Offer,
    ) -> Result<(Plan, Base), InstallRefusal> {
        let installed = base.vouched()?;
        let record = self.recorded(&base, installed.manifest())?;
        if manifest.id() != record.plugin_id {
            return Err(InstallRefusal::WrongPlugin {
                expected: record.plugin_id,
                offered: manifest.id().to_owned(),
            });
        }
        // `syrinx-update` decided the publisher; this only checks it used this record's.
        if record.signer_key != offer.publisher {
            return Err(InstallRefusal::NotCheckedAgainstRecord);
        }
        if !is_newer(manifest.version(), &record.version) {
            return Err(InstallRefusal::NotNewer);
        }
        let changes = compare(installed.manifest(), manifest);
        let plan = if changes.needs_consent() {
            Plan::Held(changes)
        } else {
            Plan::Update(changes)
        };
        let base = Base {
            record,
            component: base.component,
        };
        Ok((plan, base))
    }

    /// The install record of the offer's plugin, which must name exactly this offer.
    fn recorded(
        &self,
        offer: &Offer,
        manifest: &PluginManifest,
    ) -> Result<PluginInstall, InstallRefusal> {
        let id = manifest.id();
        let record = self
            .record(id)?
            .ok_or_else(|| OwnerError::NotInstalled(id.to_owned()))?;
        let offered = install_record(id, offer, manifest, record.installed_at, record.updated_at);
        if !same_install(&record, &offered) {
            return Err(InstallRefusal::NotRecorded);
        }
        Ok(record)
    }

    /// Grant each kind in turn, as the owner chose on the sheet.
    fn grant_all(
        &mut self,
        plugin: &str,
        kinds: &[CapabilityKind],
        now: i64,
    ) -> Result<(), OwnerError> {
        for kind in kinds {
            self.grant(plugin, *kind, now)?;
        }
        Ok(())
    }

    /// Undo a new install that failed: no registration, record or grant stays.
    fn roll_back_install(
        &mut self,
        plugin: &str,
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
    ) {
        // A record that was never saved has nothing to remove.
        match self.uninstall(plugin, registry, commands) {
            Ok(()) | Err(OwnerError::NotInstalled(_)) => {}
            Err(error) => eprintln!("a failed plugin install could not be rolled back: {error}"),
        }
    }
}

/// Whether two records name the same installed files: version, publisher and both hashes.
pub(super) fn same_install(left: &PluginInstall, right: &PluginInstall) -> bool {
    left.version == right.version
        && left.signer_key == right.signer_key
        && left.module_sha256 == right.module_sha256
        && left.manifest_sha256 == right.manifest_sha256
}

/// Whether version `offered` is strictly newer than `installed`.
pub(super) fn is_newer(offered: &str, installed: &str) -> bool {
    matches!(
        (release(offered), release(installed)),
        (Some(offered), Some(installed)) if offered > installed
    )
}

/// A background compile that failed because the component itself was refused, not because
/// Syrinx was busy: its commands must not stay runnable.
fn is_refusal(error: &HostError) -> bool {
    matches!(
        error,
        HostError::Malformed(_)
            | HostError::Precompiled
            | HostError::Identity(_)
            | HostError::Incompatible(_)
    )
}

/// The install record for an offer: the hashes of exactly the bytes that load, and its publisher.
pub(super) fn install_record(
    plugin: &str,
    offer: &Offer,
    manifest: &PluginManifest,
    installed_at: i64,
    updated_at: i64,
) -> PluginInstall {
    PluginInstall {
        plugin_id: plugin.to_owned(),
        version: manifest.version().to_owned(),
        module_sha256: offer.module_sha256,
        manifest_sha256: offer.manifest_sha256,
        signer_key: offer.publisher.clone(),
        state: PluginState::Enabled,
        installed_at,
        updated_at,
    }
}

/// Every required capability that `considered` selects must be among `granted`.
pub(super) fn check_required(
    manifest: &PluginManifest,
    granted: &[CapabilityKind],
    considered: impl Fn(CapabilityKind) -> bool,
) -> Result<(), InstallRefusal> {
    let declined = manifest
        .capabilities()
        .iter()
        .filter(|request| request.required && considered(request.kind))
        .find(|request| !granted.contains(&request.kind));
    match declined {
        Some(request) => Err(InstallRefusal::RequiredDeclined(request.kind)),
        None => Ok(()),
    }
}

/// Whether Syrinx's version, such as `0.9.0` or `0.9.0-preview`, is at least `needed`.
fn meets(host_version: &str, needed: &str) -> bool {
    match (release(host_version), release(needed)) {
        (Some(host), Some(needed)) => host >= needed,
        _ => false,
    }
}

/// `major.minor.patch` as numbers, ignoring a `-` or `+` suffix; `None` for anything else.
fn release(version: &str) -> Option<(u32, u32, u32)> {
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u32>().ok());
    let triple = (parts.next()??, parts.next()??, parts.next()??);
    if parts.next().is_some() {
        return None;
    }
    Some(triple)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_needing_a_newer_syrinx_is_refused() {
        assert!(meets("0.9.0", "0.9.0"));
        assert!(meets("0.10.0-preview", "0.9.3"));
        assert!(meets("1.0.0+build", "0.99.99"));
        assert!(!meets("0.9.0", "0.9.1"));
        assert!(!meets("0.9.0", "1.0.0"));
        // An unreadable version proves nothing, so it never meets a requirement.
        assert!(!meets("preview", "0.1.0"));
        assert!(!meets("0.9.0.1", "0.1.0"));
    }

    #[test]
    fn only_a_strictly_newer_version_is_an_update() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("next", "0.1.0"));
    }

    #[test]
    fn only_a_refused_component_is_withdrawn() {
        assert!(is_refusal(&HostError::Precompiled));
        assert!(is_refusal(&HostError::Malformed("synthetic".into())));
        assert!(!is_refusal(&HostError::Unavailable("busy".into())));
    }
}

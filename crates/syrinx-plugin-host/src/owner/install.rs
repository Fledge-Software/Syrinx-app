//! Installing, updating and uninstalling a plugin from a package the application verified
//! (#140): which ids a new plugin may take, what Syrinx version it needs, and whether an update
//! waits for the owner because it asks for more (decisions 9 to 12).

use std::fmt;
use std::sync::Arc;

use syrinx_core::CommandRegistry;
use syrinx_store::{PluginGrant, PluginInstall, PluginState, StoreError};

use super::{OwnerError, OwnerRuntime, scope_json};
use crate::identity::{IdentityMismatch, SignedIdentity, SignedManifest, check_signed};
use crate::manifest::{
    CapabilityChanges, CapabilityKind, DiscoveryOptions, ManifestError, PluginManifest, compare,
};
use crate::registry::{PluginRegistry, RegistrationError, is_reserved_id};

/// The built-in plugins Syrinx bundles in its release (runtime slice 4); a plugin installed
/// from a folder may not take their ids.
pub const BUILT_IN_PLUGINS: [&str; 3] = ["notes", "forge", "fledge"];

/// A package `syrinx-update` staged and verified, as the host needs it. The application builds
/// it from the staged copy's listed files, so each hash names exactly the bytes beside it.
pub struct Offer {
    /// The verified list's `id`, `version` and `world` lines.
    pub signed: SignedIdentity,
    pub manifest: Vec<u8>,
    pub manifest_sha256: [u8; 32],
    pub component: Arc<[u8]>,
    pub module_sha256: [u8; 32],
    /// The publisher as the install record keeps it.
    pub publisher: String,
    /// An unsigned development plugin, allowed only in the owner's developer mode.
    pub unsigned: bool,
}

/// What installing an offer would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// A plugin new to Syrinx; the owner confirms the id it claims.
    Install,
    /// An update that asks for nothing new: it replaces the running version, and its grants
    /// narrow to what it asks for.
    Update(CapabilityChanges),
    /// An update that asks for more: held, while the installed version keeps running, until
    /// the owner accepts the difference.
    Held(CapabilityChanges),
}

/// An offer whose manifest matches its signed lines and passed every install rule.
pub struct Reviewed {
    signed: SignedManifest,
    offer: Offer,
    plan: Plan,
}

impl Reviewed {
    /// What the review sheet shows: the manifest the signed lines vouch for.
    pub fn manifest(&self) -> &PluginManifest {
        self.signed.manifest()
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// The owner accepted a held update's difference, so it may now replace the running one.
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
    /// The id belongs to Syrinx, to a built-in plugin or to an installed plugin.
    IdTaken(String),
    /// Third-party plugins get no full-trust capability in v1 (decision 10).
    FullTrust(CapabilityKind),
    /// The plugin's `min-syrinx` is newer than this Syrinx.
    NeedsNewerSyrinx(String),
    /// The offer is not signed by the publisher the install record names.
    DifferentPublisher,
    /// The update is for another plugin than the one being updated.
    WrongPlugin {
        expected: String,
        offered: String,
    },
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
            Self::IdTaken(id) => write!(formatter, "the plugin id {id} is already taken"),
            Self::FullTrust(kind) => write!(
                formatter,
                "the plugin asks for {kind}, which can run programs on your computer, and \
                 plugins installed from a folder cannot have that yet"
            ),
            Self::NeedsNewerSyrinx(version) => {
                write!(formatter, "This plugin needs Syrinx {version} or newer")
            }
            Self::DifferentPublisher => {
                formatter.write_str("This update is signed by a different publisher")
            }
            Self::WrongPlugin { expected, offered } => {
                write!(formatter, "this is {offered}, not an update to {expected}")
            }
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

impl From<RegistrationError> for InstallRefusal {
    fn from(error: RegistrationError) -> Self {
        Self::Registration(error)
    }
}

impl OwnerRuntime {
    /// Check an offer without compiling or running anything: its manifest against its signed
    /// lines, the Syrinx version it needs and its capabilities; then, for a new plugin
    /// (`updating` is `None`), that its id is free, and for an update of `updating`, its
    /// publisher and what it asks for beyond the installed version.
    pub fn review(&self, offer: Offer, updating: Option<&str>) -> Result<Reviewed, InstallRefusal> {
        let options = DiscoveryOptions {
            developer_mode: offer.unsigned,
        };
        let manifest =
            PluginManifest::parse(&offer.manifest, options).map_err(InstallRefusal::Manifest)?;
        let signed = check_signed(&offer.signed, manifest).map_err(InstallRefusal::Identity)?;
        self.check_installable(signed.manifest())?;
        let plan = match updating {
            None => self.check_new_id(signed.manifest().id())?,
            Some(plugin) => self.plan_update(plugin, &offer, signed.manifest())?,
        };
        Ok(Reviewed {
            signed,
            offer,
            plan,
        })
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

    /// Replace the installed version with a reviewed update: an update that asks for nothing
    /// new at once, a held one only once accepted. Its grants narrow to what it asks for, the
    /// grants of requests it dropped are deleted, and any new capability is granted only if the
    /// owner granted it. The old version's compiled code is discarded, so only the new one is
    /// ever warm. A failure before the new version loads leaves the old record and
    /// registration in place.
    pub fn update_reviewed(
        &mut self,
        reviewed: Reviewed,
        granted: &[CapabilityKind],
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
        now: i64,
    ) -> Result<(), InstallRefusal> {
        let added = match &reviewed.plan {
            Plan::Update(changes) => changes.added.clone(),
            Plan::Held(_) => return Err(InstallRefusal::NeedsConsent),
            Plan::Install => return Err(InstallRefusal::WrongPlan),
        };
        check_required(reviewed.manifest(), granted, |kind| added.contains(&kind))?;
        let Reviewed { signed, offer, .. } = reviewed;
        let id = signed.manifest().id().to_owned();
        let old_record = self
            .record(&id)?
            .ok_or_else(|| OwnerError::NotInstalled(id.clone()))?;
        let record = install_record(
            &id,
            &offer,
            signed.manifest(),
            old_record.installed_at,
            now.max(old_record.installed_at),
        );
        self.store.save_plugin_install(record)?;
        if let Err(error) = self.swap_version(&id, signed, offer.component, registry, commands) {
            self.restore_record(old_record);
            return Err(error);
        }
        // Loading activates nothing, so no plugin code runs before its grants are narrowed.
        let narrowed = self.narrow_grants(&id, offer.manifest_sha256);
        let new_grants = self.grant_all(
            &id,
            &added
                .into_iter()
                .filter(|kind| granted.contains(kind))
                .collect::<Vec<_>>(),
            now,
        );
        narrowed.and(new_grants).map_err(InstallRefusal::Owner)
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
    fn check_new_id(&self, id: &str) -> Result<Plan, InstallRefusal> {
        let taken =
            is_reserved_id(id) || BUILT_IN_PLUGINS.contains(&id) || self.record(id)?.is_some();
        if taken {
            return Err(InstallRefusal::IdTaken(id.to_owned()));
        }
        Ok(Plan::Install)
    }

    /// An update of `plugin`: the same id and publisher as its record, compared with the
    /// version running this session.
    fn plan_update(
        &self,
        plugin: &str,
        offer: &Offer,
        manifest: &PluginManifest,
    ) -> Result<Plan, InstallRefusal> {
        if manifest.id() != plugin {
            return Err(InstallRefusal::WrongPlugin {
                expected: plugin.to_owned(),
                offered: manifest.id().to_owned(),
            });
        }
        let record = self
            .record(plugin)?
            .ok_or_else(|| OwnerError::NotInstalled(plugin.to_owned()))?;
        if record.signer_key != offer.publisher {
            return Err(InstallRefusal::DifferentPublisher);
        }
        let running = self.added(plugin)?;
        let changes = compare(&running.manifest, manifest);
        if changes.needs_consent() {
            return Ok(Plan::Held(changes));
        }
        Ok(Plan::Update(changes))
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
            Err(_) => eprintln!("a failed plugin install could not be rolled back completely"),
        }
    }

    /// Withdraw the old version's registration, register the new one and load it in place of
    /// the running one, discarding the old compiled code. The old registration returns if the
    /// new one is refused.
    fn swap_version(
        &mut self,
        plugin: &str,
        signed: SignedManifest,
        component: Arc<[u8]>,
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
    ) -> Result<(), InstallRefusal> {
        let old = registry
            .withdraw(plugin, commands)
            .ok_or_else(|| OwnerError::NotLoaded(plugin.to_owned()))?;
        if let Err(error) = registry.register(signed, commands) {
            if registry.reinstate(old, commands).is_err() {
                eprintln!("a refused plugin update could not restore the running version");
            }
            return Err(error.into());
        }
        self.unload(plugin)?;
        if let Some(cache) = self.cache.as_ref() {
            cache.delete_entry_or_report(plugin);
        }
        let registered = registry
            .get(plugin)
            .ok_or_else(|| RegistrationError::UnknownPlugin(plugin.to_owned()))?;
        Ok(self.load(registered, component)?)
    }

    /// Stop the running version and forget it, so the new one can be added.
    fn unload(&mut self, plugin: &str) -> Result<(), OwnerError> {
        let removed = self.added.remove(plugin);
        match (removed, self.running.as_mut()) {
            (Some(_), Some(running)) => running.plugins.uninstall(plugin).map_err(OwnerError::Host),
            _ => Ok(()),
        }
    }

    /// Put the replaced version's record back after a failed update.
    fn restore_record(&self, record: PluginInstall) {
        if self.store.save_plugin_install(record).is_err() {
            eprintln!("a failed plugin update could not restore its install record");
        }
    }

    /// Keep each grant the new manifest still asks for, rescoped to what it asks for now, and
    /// delete the rest. The new manifest is loaded, so `requested_scope` reads it.
    fn narrow_grants(&mut self, plugin: &str, manifest_sha256: [u8; 32]) -> Result<(), OwnerError> {
        let held: Vec<PluginGrant> = self
            .store
            .plugin_grants()?
            .into_iter()
            .filter(|stored| stored.plugin_id == plugin)
            .map(|stored| stored.grant)
            .collect();
        let mut first_failure = Ok(());
        for grant in held {
            let Some(kind) = CapabilityKind::from_name(&grant.capability) else {
                continue;
            };
            let kept = self.rescope(plugin, kind, grant, manifest_sha256);
            first_failure = first_failure.and(kept);
        }
        first_failure
    }

    /// One held grant: rescoped if still asked for, otherwise revoked.
    fn rescope(
        &mut self,
        plugin: &str,
        kind: CapabilityKind,
        grant: PluginGrant,
        manifest_sha256: [u8; 32],
    ) -> Result<(), OwnerError> {
        let manifest = &self.added(plugin)?.manifest;
        let Ok(scope) = super::requested_scope(manifest, kind) else {
            return self.revoke(plugin, kind);
        };
        let rescoped = PluginGrant {
            scope_json: scope_json(scope),
            granted_for_manifest: manifest_sha256,
            ..grant
        };
        self.store.grant_plugin(plugin.to_owned(), rescoped)?;
        Ok(())
    }
}

/// The install record for an offer: the hashes of exactly the bytes that load, and its publisher.
fn install_record(
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
fn check_required(
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
}

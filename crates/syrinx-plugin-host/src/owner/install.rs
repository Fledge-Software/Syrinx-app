//! Installing, updating and uninstalling a plugin from a package the application verified
//! (#140): which ids a new plugin may take, what Syrinx version it needs, whether an update
//! waits for the owner because it asks for more, and putting the running version back in full
//! when an update fails (decisions 9 to 12).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use syrinx_core::CommandRegistry;
use syrinx_store::{
    FolderNarrowing, PluginFolder, PluginGrant, PluginInstall, PluginState, StoreError, StoredGrant,
};

use super::cache::ABC_SHA256;
use super::{OwnerError, OwnerRuntime, Sha256, requested_scope, scope_json};
use crate::HostError;
use crate::identity::{IdentityMismatch, SignedIdentity, SignedManifest, check_signed};
use crate::manifest::{
    CapabilityChanges, CapabilityKind, DiscoveryOptions, FolderCount, ManifestError,
    PluginManifest, Scope, compare,
};
use crate::registry::{PluginRegistry, RegisteredPlugin, RegistrationError, is_reserved_id};
use crate::worker::PluginEvent;

/// The built-in plugins the spec names. Syrinx's release will bundle their packages (research
/// note 10, "Loading"); until then their ids are still kept from plugins installed from a folder.
pub const BUILT_IN_PLUGINS: [&str; 3] = ["notes", "forge", "fledge"];

/// The publisher an install record names for an unsigned development plugin. It must equal
/// `syrinx_update::Publisher::Unsigned.to_record()`; a test in `syrinx-app` keeps them equal.
pub const UNSIGNED_PUBLISHER: &str = "unsigned-development";

/// A package `syrinx-update` staged and verified, as the host needs it: the verified lines, the
/// manifest and component bytes, and the publisher as the install record keeps it. The host
/// hashes the bytes itself, so the record names exactly the bytes that load.
pub struct Offer {
    signed: SignedIdentity,
    manifest: Vec<u8>,
    manifest_sha256: [u8; 32],
    component: Arc<[u8]>,
    module_sha256: [u8; 32],
    publisher: String,
}

impl Offer {
    /// `sha256` must compute SHA-256; the application passes the one its updates use.
    pub fn new(
        signed: SignedIdentity,
        manifest: Vec<u8>,
        component: Arc<[u8]>,
        publisher: String,
        sha256: Sha256,
    ) -> Result<Self, InstallRefusal> {
        let digest = |bytes: &[u8]| sha256(bytes).map_err(InstallRefusal::Hash);
        if digest(b"abc")? != ABC_SHA256 {
            return Err(InstallRefusal::Hash(
                "the function given is not SHA-256".into(),
            ));
        }
        Ok(Self {
            manifest_sha256: digest(&manifest)?,
            module_sha256: digest(&component)?,
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

/// An offer whose manifest matches its signed lines and passed every install rule, with the
/// install record an update was compared against.
pub struct Reviewed {
    signed: SignedManifest,
    offer: Offer,
    plan: Plan,
    base: Option<PluginInstall>,
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
    /// The installed version changed since the update was reviewed, or the update is not newer.
    Stale,
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
            Self::Stale => formatter.write_str("the installed plugin changed; review it again"),
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

/// The version an update replaces, as the update found it, so a failure puts it back in full.
struct Previous {
    record: PluginInstall,
    grants: Vec<StoredGrant>,
    folders: Vec<PluginFolder>,
    component: Arc<[u8]>,
    registered: Option<RegisteredPlugin>,
}

impl OwnerRuntime {
    /// Check an offer without running anything: its manifest against its signed lines, the
    /// Syrinx version it needs and its capabilities; then, for a new plugin (`updating` is
    /// `None`), that its id is free, and for an update of `updating`, that it was checked
    /// against the record and what it asks for beyond the running version.
    pub fn review(&self, offer: Offer, updating: Option<&str>) -> Result<Reviewed, InstallRefusal> {
        let options = DiscoveryOptions {
            developer_mode: offer.unsigned(),
        };
        let manifest =
            PluginManifest::parse(&offer.manifest, options).map_err(InstallRefusal::Manifest)?;
        let signed = check_signed(&offer.signed, manifest).map_err(InstallRefusal::Identity)?;
        self.check_installable(signed.manifest())?;
        let (plan, base) = match updating {
            None => (self.check_new_id(signed.manifest().id())?, None),
            Some(plugin) => {
                let (plan, record) = self.plan_update(plugin, &offer, signed.manifest())?;
                (plan, Some(record))
            }
        };
        Ok(Reviewed {
            signed,
            offer,
            plan,
            base,
        })
    }

    /// Register and load an installed plugin at start-up from its version folder, once the
    /// offer is exactly what its install record names: the same version, publisher and hashes.
    pub fn load_installed(
        &mut self,
        offer: Offer,
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
    ) -> Result<(), InstallRefusal> {
        let options = DiscoveryOptions {
            developer_mode: offer.unsigned(),
        };
        let manifest =
            PluginManifest::parse(&offer.manifest, options).map_err(InstallRefusal::Manifest)?;
        let signed = check_signed(&offer.signed, manifest).map_err(InstallRefusal::Identity)?;
        let id = signed.manifest().id().to_owned();
        let record = self
            .record(&id)?
            .ok_or_else(|| OwnerError::NotInstalled(id.clone()))?;
        let recorded = record.version == signed.manifest().version()
            && record.module_sha256 == offer.module_sha256
            && record.manifest_sha256 == offer.manifest_sha256
            && record.signer_key == offer.publisher;
        if !recorded {
            return Err(InstallRefusal::NotRecorded);
        }
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

    /// Replace the running version with a reviewed update, as one step: a held update only
    /// once accepted, and only while the installed version is still the one it was reviewed
    /// against. The new version must compile against its manifest before anything changes.
    /// Its record replaces the old one, its grants are rescoped to what it asks for (and
    /// folders granted under them narrowed), grants of dropped requests are deleted, and new
    /// capabilities are granted only as the owner chose. The old compiled code is discarded,
    /// so only one version per plugin is ever warm. If any step fails, the old version's
    /// registration, record, grants, folders and loaded component are all put back.
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
        let id = reviewed.manifest().id().to_owned();
        let current = self.check_base(&reviewed)?;
        let mut previous = self.remember(&id, current)?;
        let Reviewed { signed, offer, .. } = reviewed;

        // The registration first: a refused one puts the old one straight back.
        previous.registered = registry.withdraw(&id, commands);
        if let Err(error) = registry.register(signed, commands) {
            self.restore(&id, previous, registry, commands);
            return Err(error.into());
        }
        let new_grants: Vec<CapabilityKind> = added
            .into_iter()
            .filter(|kind| granted.contains(kind))
            .collect();
        let swapped = self.swap(&id, &offer, &previous, &new_grants, registry, now);
        if let Err(error) = swapped {
            self.restore(&id, previous, registry, commands);
            return Err(error);
        }
        self.commit_grants(&id, &new_grants);
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
    /// runnable. Its install record stays, so the owner can see the refusal and update or
    /// uninstall it. Returns the plugin withdrawn; any other event withdraws nothing.
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
    fn check_new_id(&self, id: &str) -> Result<Plan, InstallRefusal> {
        let taken =
            is_reserved_id(id) || BUILT_IN_PLUGINS.contains(&id) || self.record(id)?.is_some();
        if taken {
            return Err(InstallRefusal::IdTaken(id.to_owned()));
        }
        Ok(Plan::Install)
    }

    /// An update of `plugin`: staged against its record, compared with the running version.
    fn plan_update(
        &self,
        plugin: &str,
        offer: &Offer,
        manifest: &PluginManifest,
    ) -> Result<(Plan, PluginInstall), InstallRefusal> {
        if manifest.id() != plugin {
            return Err(InstallRefusal::WrongPlugin {
                expected: plugin.to_owned(),
                offered: manifest.id().to_owned(),
            });
        }
        let record = self
            .record(plugin)?
            .ok_or_else(|| OwnerError::NotInstalled(plugin.to_owned()))?;
        // `syrinx-update` decided the publisher; this only checks it used this record's.
        if record.signer_key != offer.publisher {
            return Err(InstallRefusal::NotCheckedAgainstRecord);
        }
        let changes = compare(&self.added(plugin)?.manifest, manifest);
        let plan = match changes.needs_consent() {
            true => Plan::Held(changes),
            false => Plan::Update(changes),
        };
        Ok((plan, record))
    }

    /// The install record must still be the one the update was reviewed against, and the
    /// update newer than it.
    fn check_base(&self, reviewed: &Reviewed) -> Result<PluginInstall, InstallRefusal> {
        let id = reviewed.manifest().id();
        let current = self
            .record(id)?
            .ok_or_else(|| OwnerError::NotInstalled(id.to_owned()))?;
        let unchanged = reviewed.base.as_ref().is_some_and(|base| {
            base.version == current.version
                && base.module_sha256 == current.module_sha256
                && base.manifest_sha256 == current.manifest_sha256
                && base.signer_key == current.signer_key
        });
        let newer = matches!(
            (release(reviewed.manifest().version()), release(&current.version)),
            (Some(offered), Some(installed)) if offered > installed
        );
        if !unchanged || !newer {
            return Err(InstallRefusal::Stale);
        }
        Ok(current)
    }

    /// The running version's record, grants, folders and component, before an update.
    fn remember(&self, plugin: &str, record: PluginInstall) -> Result<Previous, InstallRefusal> {
        let component = self
            .running
            .as_ref()
            .and_then(|running| running.plugins.component(plugin))
            .ok_or_else(|| OwnerError::NotLoaded(plugin.to_owned()))?;
        let grants = self
            .store
            .plugin_grants()?
            .into_iter()
            .filter(|stored| stored.plugin_id == plugin)
            .collect();
        Ok(Previous {
            record,
            grants,
            folders: self.store.plugin_folders(plugin)?,
            component,
            registered: None,
        })
    }

    /// Check the new component, write the new record and grants, then load it in place of the
    /// running version. The caller puts everything back if this fails.
    fn swap(
        &mut self,
        plugin: &str,
        offer: &Offer,
        previous: &Previous,
        new_grants: &[CapabilityKind],
        registry: &PluginRegistry,
        now: i64,
    ) -> Result<(), InstallRefusal> {
        let registered = registry
            .get(plugin)
            .ok_or_else(|| RegistrationError::UnknownPlugin(plugin.to_owned()))?;
        self.check_component(registered, &offer.component)?;
        let record = install_record(
            plugin,
            offer,
            registered.manifest(),
            previous.record.installed_at,
            now.max(previous.record.installed_at),
        );
        self.store.save_plugin_install(record)?;
        self.rescope_grants(
            plugin,
            registered.manifest(),
            offer.manifest_sha256,
            previous,
        )?;
        for kind in new_grants {
            let scope = requested_scope(registered.manifest(), *kind)?;
            let grant = PluginGrant {
                capability: kind.name().to_owned(),
                scope_json: scope_json(scope),
                granted_at: now,
                granted_for_manifest: offer.manifest_sha256,
            };
            self.store.grant_plugin(plugin.to_owned(), grant)?;
        }
        self.stop(plugin)?;
        if let Some(cache) = self.cache.as_ref() {
            cache.delete_entry_or_report(plugin);
        }
        Ok(self.load(registered, Arc::clone(&offer.component))?)
    }

    /// Compile a component against its registered manifest on the running workers' runtime.
    fn check_component(
        &mut self,
        registered: &RegisteredPlugin,
        component: &[u8],
    ) -> Result<(), InstallRefusal> {
        let plugins = &self.running()?.plugins;
        Ok(plugins.check_component(registered, component)?)
    }

    /// Keep each stored grant the new manifest still asks for, rescoped to what it asks for
    /// now, its folders narrowed to match; delete the rest. Widening happens only here and
    /// only after the owner accepted it.
    fn rescope_grants(
        &self,
        plugin: &str,
        manifest: &PluginManifest,
        manifest_sha256: [u8; 32],
        previous: &Previous,
    ) -> Result<(), InstallRefusal> {
        for stored in &previous.grants {
            let Some(kind) = CapabilityKind::from_name(&stored.grant.capability) else {
                continue;
            };
            let Ok(scope) = requested_scope(manifest, kind) else {
                self.store
                    .revoke_plugin_grant(plugin.to_owned(), stored.grant.capability.clone())?;
                continue;
            };
            let rescoped = PluginGrant {
                scope_json: scope_json(scope),
                granted_for_manifest: manifest_sha256,
                ..stored.grant.clone()
            };
            self.store.grant_plugin(plugin.to_owned(), rescoped)?;
            if let Some(narrowing) = folder_narrowing(stored.grant_id, scope) {
                self.store
                    .narrow_plugin_folders(plugin.to_owned(), narrowing)?;
            }
        }
        Ok(())
    }

    /// The new version is in place: the grants the workers check follow the store.
    fn commit_grants(&mut self, plugin: &str, new_grants: &[CapabilityKind]) {
        let Ok(added) = self.added(plugin) else {
            return;
        };
        let requested: Vec<CapabilityKind> = added
            .manifest
            .capabilities()
            .iter()
            .map(|request| request.kind)
            .collect();
        for kind in self.grants.kinds(plugin) {
            if !requested.contains(&kind) {
                self.grants.revoke(plugin, kind);
            }
        }
        for kind in new_grants {
            self.grants.grant(plugin, *kind);
        }
    }

    /// Put the running version back after a failed update: its registration, record, grants
    /// and folders, and its loaded component. Each failure to do so is reported.
    fn restore(
        &mut self,
        plugin: &str,
        previous: Previous,
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
    ) {
        // The update stops the old version only right before loading the new one, and a failed
        // load adds nothing, so a plugin still added here is the old version, still running.
        let old_running = self.added.contains_key(plugin);
        if !old_running {
            self.drop_stray_slot(plugin);
        }
        registry.withdraw(plugin, commands);
        if let Err(error) = self.store.save_plugin_install(previous.record) {
            eprintln!("a failed plugin update could not restore its record: {error}");
        }
        if let Err(error) = self.restore_grants(plugin, &previous.grants, &previous.folders) {
            eprintln!("a failed plugin update could not restore its grants: {error}");
        }
        let Some(registered) = previous.registered else {
            return;
        };
        let registered = match registry.reinstate(registered, commands) {
            Ok(registered) => registered,
            Err(error) => {
                eprintln!("a failed plugin update could not register the old version: {error}");
                return;
            }
        };
        if old_running {
            return;
        }
        if let Err(error) = self.load(registered, previous.component) {
            eprintln!("a failed plugin update could not load the old version: {error}");
        }
    }

    /// Forget a slot a failed load left behind, so the old version can be added again.
    fn drop_stray_slot(&mut self, plugin: &str) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        if running.plugins.status(plugin).is_none() {
            return;
        }
        if let Err(error) = running.plugins.uninstall(plugin) {
            eprintln!("a failed plugin update could not stop the new version: {error}");
        }
    }

    /// Make the stored grants and folders exactly `grants` and `folders` again.
    fn restore_grants(
        &self,
        plugin: &str,
        grants: &[StoredGrant],
        folders: &[PluginFolder],
    ) -> Result<(), StoreError> {
        let stored = self.store.plugin_grants()?;
        for extra in stored
            .iter()
            .filter(|stored| stored.plugin_id == plugin)
            .filter(|stored| {
                !grants
                    .iter()
                    .any(|kept| kept.grant.capability == stored.grant.capability)
            })
        {
            self.store
                .revoke_plugin_grant(plugin.to_owned(), extra.grant.capability.clone())?;
        }
        let mut grant_ids = BTreeMap::new();
        for kept in grants {
            let grant_id = self
                .store
                .grant_plugin(plugin.to_owned(), kept.grant.clone())?;
            grant_ids.insert(kept.grant_id, grant_id);
        }
        for folder in folders {
            let Some(grant_id) = grant_ids.get(&folder.grant_id) else {
                continue;
            };
            let restored = PluginFolder {
                grant_id: *grant_id,
                ..folder.clone()
            };
            self.store.add_plugin_folder(plugin.to_owned(), restored)?;
        }
        Ok(())
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

/// How the folders granted under `grant_id` narrow to a folder scope; `None` for any other
/// scope.
fn folder_narrowing(grant_id: i64, scope: &Scope) -> Option<FolderNarrowing> {
    let Scope::Folder {
        writable, count, ..
    } = scope
    else {
        return None;
    };
    let keep = match count {
        FolderCount::Limited(limit) => Some(*limit),
        FolderCount::Many => None,
    };
    Some(FolderNarrowing {
        grant_id,
        read_only: !writable,
        keep,
    })
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

    #[test]
    fn only_a_refused_component_is_withdrawn() {
        assert!(is_refusal(&HostError::Precompiled));
        assert!(is_refusal(&HostError::Malformed("synthetic".into())));
        assert!(!is_refusal(&HostError::Unavailable("busy".into())));
    }

    #[test]
    fn a_folder_scope_narrows_its_folders_and_nothing_else_does() {
        let read_two = Scope::Folder {
            writable: false,
            count: FolderCount::Limited(2),
            watch: false,
        };
        assert_eq!(
            folder_narrowing(7, &read_two),
            Some(FolderNarrowing {
                grant_id: 7,
                read_only: true,
                keep: Some(2)
            })
        );
        assert_eq!(folder_narrowing(7, &Scope::None), None);
    }
}

//! The plugin runtime as the application owns it (#136, P2-04): installs and the owner's
//! grants kept in Syrinx's store, each plugin's settings in its own namespace there, and
//! the per-plugin workers behind them. Built only with the `store` feature, for `syrinx-app`.

mod grants;
mod records;
mod services;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use syrinx_store::{
    FolderAccess, PluginFolder, PluginGrant, PluginInstall, PluginState, Store, StoreError,
};

pub use grants::OwnerGrants;
pub use services::StoreServices;

use crate::grants::{DenyList, FolderRefusal};
use crate::manifest::{CapabilityKind, FolderCount, PluginManifest, Scope};
use crate::registry::RegisteredPlugin;
use crate::session::{CallResult, Session};
use crate::worker::{Pending, PluginEvent, Plugins};
use crate::{HostError, Runtime};
use records::PluginRecords;

/// Why the runtime refused or could not finish an owner's action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerError {
    Store(StoreError),
    Host(HostError),
    /// The plugin is not installed, or its install record does not match it.
    NotInstalled(String),
    /// The plugin's manifest does not ask for this capability.
    NotRequested(CapabilityKind),
    /// No plugin may hold a full-trust capability until Forge's slices.
    FullTrust(CapabilityKind),
    /// A folder may be added only under a granted `folder` capability.
    FolderNotGranted,
    Folder(FolderRefusal),
    /// A thread the runtime needs could not start.
    Thread(String),
}

impl fmt::Display for OwnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Host(error) => write!(formatter, "{error}"),
            Self::NotInstalled(plugin) => write!(formatter, "{plugin} is not installed"),
            Self::NotRequested(kind) => write!(formatter, "the plugin does not ask for {kind}"),
            Self::FullTrust(kind) => write!(formatter, "{kind} is full trust and not offered yet"),
            Self::FolderNotGranted => write!(formatter, "the folder capability is not granted"),
            Self::Folder(refusal) => write!(formatter, "the folder was refused: {refusal}"),
            Self::Thread(detail) => write!(formatter, "a plugin thread did not start: {detail}"),
        }
    }
}

impl std::error::Error for OwnerError {}

impl From<StoreError> for OwnerError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<HostError> for OwnerError {
    fn from(error: HostError) -> Self {
        Self::Host(error)
    }
}

/// An installed plugin the runtime knows the manifest of.
struct Installed {
    manifest: PluginManifest,
    manifest_sha256: [u8; 32],
}

/// The workers and the records thread, started with the first plugin added.
struct Running {
    plugins: Plugins<StoreServices>,
    events: Receiver<PluginEvent>,
    records: PluginRecords,
}

/// Every installed plugin and the owner's grants, as one Syrinx session holds them.
pub struct OwnerRuntime {
    store: Arc<Store>,
    host_version: String,
    grants: Arc<OwnerGrants>,
    deny: DenyList,
    installed: BTreeMap<String, Installed>,
    running: Option<Running>,
}

impl OwnerRuntime {
    /// Read the owner's grants from the store. No plugin code, compiler or thread starts
    /// until a plugin is added.
    pub fn start(
        store: Arc<Store>,
        host_version: impl Into<String>,
        deny: DenyList,
    ) -> Result<Self, OwnerError> {
        let grants = OwnerGrants::from_stored(&store.plugin_grants()?);
        Ok(Self {
            store,
            host_version: host_version.into(),
            grants: Arc::new(grants),
            deny,
            installed: BTreeMap::new(),
            running: None,
        })
    }

    /// Record a newly installed or updated plugin, then add it. Its grants are kept.
    pub fn install(
        &mut self,
        plugin: &RegisteredPlugin,
        component: Arc<[u8]>,
        install: PluginInstall,
    ) -> Result<(), OwnerError> {
        if install.plugin_id != plugin.manifest().id() {
            return Err(OwnerError::NotInstalled(install.plugin_id));
        }
        self.store.save_plugin_install(install)?;
        self.load(plugin, component)
    }

    /// Add a plugin the store records as installed and enabled, as at each start-up.
    pub fn load(
        &mut self,
        plugin: &RegisteredPlugin,
        component: Arc<[u8]>,
    ) -> Result<(), OwnerError> {
        let id = plugin.manifest().id().to_owned();
        let record = self
            .store
            .plugin_installs()?
            .into_iter()
            .find(|install| install.plugin_id == id && install.state == PluginState::Enabled)
            .ok_or_else(|| OwnerError::NotInstalled(id.clone()))?;
        self.running()?.plugins.add(plugin, component)?;
        let installed = Installed {
            manifest: plugin.manifest().clone(),
            manifest_sha256: record.manifest_sha256,
        };
        self.installed.insert(id, installed);
        Ok(())
    }

    /// Grant a capability the plugin's manifest asks for, with the scope it asks for. The
    /// store keeps it first; only then may the plugin use it.
    pub fn grant(
        &mut self,
        plugin: &str,
        kind: CapabilityKind,
        granted_at: i64,
    ) -> Result<(), OwnerError> {
        let installed = self.installed(plugin)?;
        let request = installed
            .manifest
            .capabilities()
            .iter()
            .find(|request| request.kind == kind)
            .ok_or(OwnerError::NotRequested(kind))?;
        if kind.is_full_trust() {
            return Err(OwnerError::FullTrust(kind));
        }
        let grant = PluginGrant {
            capability: kind.name().to_owned(),
            scope_json: scope_json(&request.scope),
            granted_at,
            granted_for_manifest: installed.manifest_sha256,
        };
        self.store.grant_plugin(plugin.to_owned(), grant)?;
        self.grants.insert(plugin, kind);
        Ok(())
    }

    /// Grant a folder under the plugin's `folder` capability, once the deny list allows its
    /// canonical form. The folder picker and the folder service are #36's.
    pub fn grant_folder(
        &mut self,
        plugin: &str,
        folder: &Path,
        access: FolderAccess,
    ) -> Result<(), OwnerError> {
        self.installed(plugin)?;
        let grant_id = self
            .store
            .plugin_grants()?
            .into_iter()
            .find(|stored| {
                stored.plugin_id == plugin
                    && stored.grant.capability == CapabilityKind::Folder.name()
            })
            .map(|stored| stored.grant_id)
            .ok_or(OwnerError::FolderNotGranted)?;
        let path = self.deny.check(folder).map_err(OwnerError::Folder)?;
        let folder = PluginFolder {
            grant_id,
            path,
            access,
        };
        self.store.add_plugin_folder(plugin.to_owned(), folder)?;
        Ok(())
    }

    /// Withdraw a capability. It stops working at once, before the store is written: a
    /// failed write is reported, and the capability stays withdrawn for this session.
    pub fn revoke(&mut self, plugin: &str, kind: CapabilityKind) -> Result<(), OwnerError> {
        self.installed(plugin)?;
        self.grants.remove(plugin, kind);
        if let Some(running) = self.running.as_mut() {
            running.plugins.revoke(plugin, kind)?;
        }
        self.store
            .revoke_plugin_grant(plugin.to_owned(), kind.name().to_owned())?;
        Ok(())
    }

    /// Remove a plugin and everything keyed to it: it stops, then its install record, grants,
    /// folders and settings are deleted together. The search index (2e) and recovery drafts
    /// (#37) keep nothing per plugin yet; their owners join this when they do.
    pub fn remove(&mut self, plugin: &str) -> Result<(), OwnerError> {
        self.installed(plugin)?;
        if let Some(running) = self.running.as_mut() {
            running.plugins.uninstall(plugin)?;
        }
        self.installed.remove(plugin);
        self.grants.forget(plugin);
        self.store.remove_plugin(plugin.to_owned())?;
        Ok(())
    }

    /// Queue a call for an added plugin; see `Plugins::call`.
    pub fn call<R: Send + 'static>(
        &mut self,
        plugin: &str,
        call: impl FnOnce(&mut Session<StoreServices>) -> CallResult<R> + Send + 'static,
    ) -> CallResult<Pending<R>> {
        let Some(running) = self.running.as_mut() else {
            return Err(HostError::Refused(format!("{plugin} is not installed")));
        };
        running.plugins.call(plugin, call)
    }

    /// What the workers reported since the last look.
    pub fn take_events(&self) -> Vec<PluginEvent> {
        self.running
            .as_ref()
            .map(|running| running.events.try_iter().collect())
            .unwrap_or_default()
    }

    /// Stop every plugin, then the records thread, so nothing holds the store any more.
    pub fn shutdown(mut self) {
        let Some(mut running) = self.running.take() else {
            return;
        };
        running.plugins.shutdown();
        running.records.stop();
    }

    fn installed(&self, plugin: &str) -> Result<&Installed, OwnerError> {
        self.installed
            .get(plugin)
            .ok_or_else(|| OwnerError::NotInstalled(plugin.to_owned()))
    }

    /// The workers and records thread, started on first use.
    fn running(&mut self) -> Result<&mut Running, OwnerError> {
        let running = match self.running.take() {
            Some(running) => running,
            None => self.start_running()?,
        };
        Ok(self.running.insert(running))
    }

    fn start_running(&self) -> Result<Running, OwnerError> {
        let records = PluginRecords::start(Arc::clone(&self.store))
            .map_err(|error| OwnerError::Thread(error.to_string()))?;
        let handle = records
            .handle()
            .ok_or_else(|| OwnerError::Thread("the records thread stopped".into()))?;
        let factory =
            move |manifest: &PluginManifest| StoreServices::new(manifest.id(), handle.clone());
        let grants: Arc<dyn crate::grants::Grants> = self.grants.clone();
        let (plugins, events) =
            Plugins::new(Runtime::new()?, self.host_version.clone(), grants, factory);
        Ok(Running {
            plugins,
            events,
            records,
        })
    }
}

/// The scope the manifest asks for, as the grant row keeps it. Never a path.
fn scope_json(scope: &Scope) -> String {
    let value = match scope {
        Scope::None => serde_json::json!({}),
        Scope::Folder {
            writable,
            count,
            watch,
        } => {
            let count = match count {
                FolderCount::Limited(limit) => serde_json::json!(limit),
                FolderCount::Many => serde_json::json!("many"),
            };
            serde_json::json!({ "writable": writable, "count": count, "watch": watch })
        }
        Scope::Index { feeds } => serde_json::json!({ "feeds": feeds }),
    };
    value.to_string()
}

#[cfg(test)]
mod tests;

//! The plugin runtime as the application owns it (#136, P2-04): installs and the owner's
//! grants kept in Syrinx's store, each plugin's settings in its own namespace there, and
//! the per-plugin workers behind them. Built only with the `store` feature, for `syrinx-app`.

mod services;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use syrinx_store::{
    FolderAccess, PluginFolder, PluginGrant, PluginInstall, PluginState, Store, StoreError,
    StoredGrant,
};

pub use services::StoreServices;

use crate::grants::{DenyList, FolderRefusal, GrantSet};
use crate::manifest::{CapabilityKind, FolderCount, PluginManifest, Scope};
use crate::registry::RegisteredPlugin;
use crate::session::{CallResult, Session};
use crate::worker::{Pending, PluginEvent, PluginStatus, Plugins};
use crate::{HostError, Runtime, limits};

// The plugin contract's settings bounds (`limits`) are the source; the store keeps the same.
const _: () = assert!(limits::MAX_SETTINGS_KEY_BYTES == syrinx_store::MAX_PLUGIN_SETTING_KEY_BYTES);
const _: () =
    assert!(limits::MAX_SETTINGS_VALUE_BYTES == syrinx_store::MAX_PLUGIN_SETTING_VALUE_BYTES);
const _: () = assert!(limits::MAX_SETTINGS_KEYS == syrinx_store::MAX_PLUGIN_SETTINGS);

/// Why the runtime refused or could not finish an owner's action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerError {
    Store(StoreError),
    Host(HostError),
    /// The store has no install record for the plugin, or none enabled where one must be.
    NotInstalled(String),
    /// The install record names a different plugin from the package it came with.
    RecordMismatch {
        record: String,
        plugin: String,
    },
    /// The plugin is already loaded this session; an update is the install flow's (2d).
    AlreadyAdded(String),
    /// The plugin is installed but was not loaded this session, so its manifest is unknown.
    NotLoaded(String),
    /// The plugin's manifest does not ask for this capability.
    NotRequested(CapabilityKind),
    /// No plugin may hold a full-trust capability until Forge's slices.
    FullTrust(CapabilityKind),
    /// A folder may be added only while the `folder` capability is granted.
    FolderNotGranted,
    /// The manifest asks to read folders, not to write them.
    ReadWriteNotRequested,
    /// The manifest asks for at most this many folders, and that many are granted.
    TooManyFolders(u32),
    Folder(FolderRefusal),
    /// A revoke or remove whose steps did not all succeed: the first failure, and whether
    /// the store kept the change. In memory the change has taken effect either way.
    Incomplete {
        first: Box<OwnerError>,
        persisted: bool,
    },
}

impl fmt::Display for OwnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Host(error) => write!(formatter, "{error}"),
            Self::NotInstalled(plugin) => write!(formatter, "{plugin} is not installed"),
            Self::RecordMismatch { record, plugin } => write!(
                formatter,
                "the install record is for {record}, but the package is {plugin}"
            ),
            Self::AlreadyAdded(plugin) => {
                write!(formatter, "{plugin} is already loaded this session")
            }
            Self::NotLoaded(plugin) => {
                write!(
                    formatter,
                    "{plugin} is installed but not loaded this session"
                )
            }
            Self::NotRequested(kind) => write!(formatter, "the plugin does not ask for {kind}"),
            Self::FullTrust(kind) => write!(formatter, "{kind} is full trust and not offered yet"),
            Self::FolderNotGranted => write!(formatter, "the folder capability is not granted"),
            Self::ReadWriteNotRequested => {
                write!(formatter, "the plugin asks only to read its folders")
            }
            Self::TooManyFolders(limit) => {
                write!(formatter, "the plugin asks for at most {limit} folders")
            }
            Self::Folder(refusal) => write!(formatter, "the folder was refused: {refusal}"),
            Self::Incomplete { first, persisted } => {
                let saved = if *persisted {
                    "the change was saved"
                } else {
                    "the change was not saved and returns at the next start"
                };
                write!(formatter, "{first}; {saved}")
            }
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

/// A plugin added this session, with the manifest that says what it may ask for.
struct Added {
    manifest: PluginManifest,
    manifest_sha256: [u8; 32],
}

/// The workers, started with the first plugin added.
struct Running {
    plugins: Plugins<StoreServices>,
    events: Receiver<PluginEvent>,
}

/// Every installed plugin and the owner's grants, as one Syrinx session holds them.
pub struct OwnerRuntime {
    store: Arc<Store>,
    host_version: String,
    grants: GrantSet,
    deny: DenyList,
    added: BTreeMap<String, Added>,
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
        let grants = from_stored(&store.plugin_grants()?);
        Ok(Self {
            store,
            host_version: host_version.into(),
            grants,
            deny,
            added: BTreeMap::new(),
            running: None,
        })
    }

    /// Record a newly installed plugin, then add it. A plugin already added is refused before
    /// anything is written: updating one is the install flow's (2d).
    pub fn install(
        &mut self,
        plugin: &RegisteredPlugin,
        component: Arc<[u8]>,
        install: PluginInstall,
    ) -> Result<(), OwnerError> {
        let id = plugin.manifest().id();
        if install.plugin_id != id {
            return Err(OwnerError::RecordMismatch {
                record: install.plugin_id,
                plugin: id.to_owned(),
            });
        }
        if self.added.contains_key(id) {
            return Err(OwnerError::AlreadyAdded(id.to_owned()));
        }
        self.store.save_plugin_install(install)?;
        self.load(plugin, component)
    }

    /// Add a plugin the store records as installed and enabled, as at each start-up. The
    /// record's hashes and signer are not checked against the package here: the package is
    /// verified when it is installed (2d, #140).
    pub fn load(
        &mut self,
        plugin: &RegisteredPlugin,
        component: Arc<[u8]>,
    ) -> Result<(), OwnerError> {
        let id = plugin.manifest().id().to_owned();
        if self.added.contains_key(&id) {
            return Err(OwnerError::AlreadyAdded(id));
        }
        let record = self
            .record(&id)?
            .filter(|install| install.state == PluginState::Enabled)
            .ok_or_else(|| OwnerError::NotInstalled(id.clone()))?;
        self.running()?.plugins.add(plugin, component)?;
        let added = Added {
            manifest: plugin.manifest().clone(),
            manifest_sha256: record.manifest_sha256,
        };
        self.added.insert(id, added);
        Ok(())
    }

    /// Grant a capability the plugin's manifest asks for, with the scope it asks for. The
    /// store keeps it first; only then may the plugin use it. The plugin must be added, since
    /// its manifest says what it may ask for.
    pub fn grant(
        &mut self,
        plugin: &str,
        kind: CapabilityKind,
        granted_at: i64,
    ) -> Result<(), OwnerError> {
        let added = self.added(plugin)?;
        let scope = requested_scope(&added.manifest, kind)?;
        if kind.is_full_trust() {
            return Err(OwnerError::FullTrust(kind));
        }
        let grant = PluginGrant {
            capability: kind.name().to_owned(),
            scope_json: scope_json(scope),
            granted_at,
            granted_for_manifest: added.manifest_sha256,
        };
        self.store.grant_plugin(plugin.to_owned(), grant)?;
        self.grants.grant(plugin, kind);
        Ok(())
    }

    /// Grant a folder under the plugin's `folder` capability, no wider than its manifest
    /// asks, once the deny list allows its canonical form. The picker is #36's.
    pub fn grant_folder(
        &mut self,
        plugin: &str,
        folder: &Path,
        access: FolderAccess,
    ) -> Result<(), OwnerError> {
        let added = self.added(plugin)?;
        let Scope::Folder {
            writable, count, ..
        } = requested_scope(&added.manifest, CapabilityKind::Folder)?
        else {
            return Err(OwnerError::NotRequested(CapabilityKind::Folder));
        };
        let (writable, count) = (*writable, *count);
        if !self.grants.is_granted(plugin, CapabilityKind::Folder) {
            return Err(OwnerError::FolderNotGranted);
        }
        if access == FolderAccess::ReadWrite && !writable {
            return Err(OwnerError::ReadWriteNotRequested);
        }
        let grant_id = self.folder_grant_id(plugin)?;
        let path = self.deny.check(folder).map_err(OwnerError::Folder)?;
        self.check_folder_count(plugin, grant_id, &path, count)?;
        let folder = PluginFolder {
            grant_id,
            path,
            access,
        };
        self.store.add_plugin_folder(plugin.to_owned(), folder)?;
        Ok(())
    }

    /// Withdraw a capability from any installed plugin, added this session or not. It stops
    /// working at once; the worker and the store are then each told, and a failure of either
    /// is reported with whether the store kept the change.
    pub fn revoke(&mut self, plugin: &str, kind: CapabilityKind) -> Result<(), OwnerError> {
        self.require_record(plugin)?;
        self.grants.revoke(plugin, kind);
        let workers = match (self.added.contains_key(plugin), self.running.as_mut()) {
            (true, Some(running)) => running.plugins.revoke(plugin, kind),
            _ => Ok(()),
        };
        let stored = self
            .store
            .revoke_plugin_grant(plugin.to_owned(), kind.name().to_owned());
        both_steps(workers, stored)
    }

    /// Remove any installed plugin and everything keyed to it: it stops, then its install
    /// record, grants, folders and settings are deleted together. The search index (#141) and
    /// recovery drafts (#37) keep nothing per plugin yet; their owners join this when they do.
    pub fn remove(&mut self, plugin: &str) -> Result<(), OwnerError> {
        self.require_record(plugin)?;
        let workers = match (self.added.remove(plugin), self.running.as_mut()) {
            (Some(_), Some(running)) => running.plugins.uninstall(plugin),
            _ => Ok(()),
        };
        self.grants.forget(plugin);
        let stored = self.store.remove_plugin(plugin.to_owned());
        both_steps(workers, stored)
    }

    /// Queue a call for an added plugin; see `Plugins::call`.
    pub fn call<R: Send + 'static>(
        &mut self,
        plugin: &str,
        call: impl FnOnce(&mut Session<StoreServices>) -> CallResult<R> + Send + 'static,
    ) -> CallResult<Pending<R>> {
        let loaded = self.added.contains_key(plugin);
        let Some(running) = self.running.as_mut().filter(|_| loaded) else {
            return Err(HostError::Refused(format!(
                "{plugin} is not loaded this session"
            )));
        };
        running.plugins.call(plugin, call)
    }

    /// Where an added plugin is in its life; `None` for one not added this session.
    pub fn status(&self, plugin: &str) -> Option<PluginStatus> {
        self.running.as_ref()?.plugins.status(plugin)
    }

    /// What the workers reported since the last look.
    pub fn take_events(&self) -> Vec<PluginEvent> {
        self.running
            .as_ref()
            .map(|running| running.events.try_iter().collect())
            .unwrap_or_default()
    }

    /// Stop every plugin, so nothing but the caller holds the store any more.
    pub fn shutdown(mut self) {
        if let Some(running) = self.running.take() {
            running.plugins.shutdown();
        }
    }

    fn added(&self, plugin: &str) -> Result<&Added, OwnerError> {
        self.added
            .get(plugin)
            .ok_or_else(|| OwnerError::NotLoaded(plugin.to_owned()))
    }

    /// The store's install record for `plugin`, in any state.
    fn record(&self, plugin: &str) -> Result<Option<PluginInstall>, OwnerError> {
        let installs = self.store.plugin_installs()?;
        Ok(installs
            .into_iter()
            .find(|install| install.plugin_id == plugin))
    }

    fn require_record(&self, plugin: &str) -> Result<(), OwnerError> {
        match self.record(plugin)? {
            Some(_) => Ok(()),
            None => Err(OwnerError::NotInstalled(plugin.to_owned())),
        }
    }

    fn folder_grant_id(&self, plugin: &str) -> Result<i64, OwnerError> {
        let folder = CapabilityKind::Folder.name();
        self.store
            .plugin_grants()?
            .into_iter()
            .find(|stored| stored.plugin_id == plugin && stored.grant.capability == folder)
            .map(|stored| stored.grant_id)
            .ok_or(OwnerError::FolderNotGranted)
    }

    /// A folder past the manifest's count is refused; granting one already held again is not.
    fn check_folder_count(
        &self,
        plugin: &str,
        grant_id: i64,
        path: &Path,
        count: FolderCount,
    ) -> Result<(), OwnerError> {
        let FolderCount::Limited(limit) = count else {
            return Ok(());
        };
        let held: Vec<PluginFolder> = self
            .store
            .plugin_folders(plugin)?
            .into_iter()
            .filter(|folder| folder.grant_id == grant_id)
            .collect();
        let again = held.iter().any(|folder| folder.path == path);
        if again || held.len() < limit as usize {
            return Ok(());
        }
        Err(OwnerError::TooManyFolders(limit))
    }

    /// The workers, started on first use.
    fn running(&mut self) -> Result<&mut Running, OwnerError> {
        let running = match self.running.take() {
            Some(running) => running,
            None => self.start_running()?,
        };
        Ok(self.running.insert(running))
    }

    fn start_running(&self) -> Result<Running, OwnerError> {
        let store = Arc::clone(&self.store);
        let factory =
            move |manifest: &PluginManifest| StoreServices::new(manifest.id(), Arc::clone(&store));
        let (plugins, events) = Plugins::new(
            Runtime::new()?,
            self.host_version.clone(),
            self.grants.clone(),
            factory,
        );
        Ok(Running { plugins, events })
    }
}

/// The grants the store holds. A row naming a kind this Syrinx does not know grants nothing.
fn from_stored(rows: &[StoredGrant]) -> GrantSet {
    let grants = GrantSet::new();
    for row in rows {
        if let Some(kind) = CapabilityKind::from_name(&row.grant.capability) {
            grants.grant(&row.plugin_id, kind);
        }
    }
    grants
}

/// The scope `manifest` asks `kind` with, if it asks for it at all.
fn requested_scope(manifest: &PluginManifest, kind: CapabilityKind) -> Result<&Scope, OwnerError> {
    manifest
        .capabilities()
        .iter()
        .find(|request| request.kind == kind)
        .map(|request| &request.scope)
        .ok_or(OwnerError::NotRequested(kind))
}

/// A revoke's or remove's result: the worker step's failure first, then the store's, and
/// whether the store kept the change.
fn both_steps(workers: CallResult<()>, stored: Result<bool, StoreError>) -> Result<(), OwnerError> {
    match (workers, stored) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), stored) => Err(OwnerError::Incomplete {
            first: Box::new(OwnerError::Host(error)),
            persisted: stored.is_ok(),
        }),
        (Ok(()), Err(error)) => Err(OwnerError::Incomplete {
            first: Box::new(OwnerError::Store(error)),
            persisted: false,
        }),
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
mod tests {
    use super::*;

    fn stored(plugin: &str, capability: &str) -> StoredGrant {
        StoredGrant {
            grant_id: 1,
            plugin_id: plugin.to_owned(),
            grant: PluginGrant {
                capability: capability.to_owned(),
                scope_json: "{}".to_owned(),
                granted_at: 1,
                granted_for_manifest: [2; 32],
            },
        }
    }

    #[test]
    fn stored_grants_are_read_by_kind_and_unknown_kinds_grant_nothing() {
        let grants = from_stored(&[stored("test", "storage"), stored("test", "teleport")]);
        assert_eq!(grants.kinds("test"), [CapabilityKind::Storage]);
        assert!(grants.kinds("other").is_empty());
    }

    #[test]
    fn a_failed_step_is_reported_with_whether_the_store_kept_the_change() {
        let host = || HostError::Unavailable("synthetic".into());
        assert_eq!(both_steps(Ok(()), Ok(true)), Ok(()));
        assert_eq!(
            both_steps(Err(host()), Ok(true)),
            Err(OwnerError::Incomplete {
                first: Box::new(OwnerError::Host(host())),
                persisted: true
            })
        );
        assert_eq!(
            both_steps(Err(host()), Err(StoreError::Busy)),
            Err(OwnerError::Incomplete {
                first: Box::new(OwnerError::Host(host())),
                persisted: false
            })
        );
        assert_eq!(
            both_steps(Ok(()), Err(StoreError::Busy)),
            Err(OwnerError::Incomplete {
                first: Box::new(OwnerError::Store(StoreError::Busy)),
                persisted: false
            })
        );
    }
}

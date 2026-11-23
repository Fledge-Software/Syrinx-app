//! The plugin runtime as the application owns it (#136, P2-04): installs and the owner's
//! grants kept in Syrinx's store, each plugin's settings in its own namespace there, and
//! the per-plugin workers behind them, and the compiled-code cache (#137). Built only with
//! the `store` feature, for `syrinx-app`.

mod cache;
mod services;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use syrinx_store::{
    FolderAccess, PluginFolder, PluginGrant, PluginInstall, PluginState, Store, StoreError,
    StoredGrant,
};

use wasmtime::Engine;

pub use cache::{CacheSetup, Sha256};
pub use services::StoreServices;

use crate::bindings::exports::syrinx::plugin::search_feed::Batch;
use crate::bindings::syrinx::plugin::host::{Host as _, LogLevel};
use crate::bindings::syrinx::plugin::types::PluginError;
use crate::grants::{DenyList, FolderRefusal, GrantSet};
use crate::manifest::{CapabilityKind, FolderCount, PluginManifest, Scope};
use crate::registry::RegisteredPlugin;
use crate::session::{CallResult, Session};
use crate::validate::{self, FeedTiers};
use crate::worker::{Pending, PluginEvent, PluginStatus, Plugins, send_event};
use crate::{CompileCounts, HostError, Runtime, limits};
use cache::StoreCache;

// The plugin contract's settings bounds (`limits`) are the source; the store keeps the same.
const _: () = assert!(limits::MAX_SETTINGS_KEY_BYTES == syrinx_store::MAX_PLUGIN_SETTING_KEY_BYTES);
const _: () =
    assert!(limits::MAX_SETTINGS_VALUE_BYTES == syrinx_store::MAX_PLUGIN_SETTING_VALUE_BYTES);
const _: () = assert!(limits::MAX_SETTINGS_KEYS == syrinx_store::MAX_PLUGIN_SETTINGS);
// The store's feed tables share the contract's bounds; `validate::batch` also refuses every
// other value the store or the full-text index would refuse, so a plugin's bad batch is the
// plugin's error and never wedges its feed (#141).
const _: () = assert!(limits::MAX_CURSOR_BYTES == syrinx_store::MAX_PLUGIN_FEED_CURSOR_BYTES);
const _: () = assert!(limits::MAX_ENTRY_ID_BYTES == syrinx_store::MAX_PLUGIN_FEED_ENTRY_ID_BYTES);
const _: () = assert!(limits::MAX_KEYWORDS == syrinx_store::MAX_PLUGIN_FEED_KEYWORDS);
const _: () =
    assert!(limits::KEYWORD_SEPARATOR as u32 == syrinx_store::PLUGIN_FEED_KEYWORD_END as u32);
// A scope lists only what its manifest names, so any accepted manifest's scope fits the store.
const _: () = assert!(crate::manifest::MAX_MANIFEST_BYTES <= syrinx_store::MAX_PLUGIN_SCOPE_BYTES);

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
    /// The plugin is installed, but disabled or held until its update is reviewed, so it is
    /// not loaded.
    NotEnabled {
        plugin: String,
        held_for_update: bool,
    },
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
            Self::NotEnabled {
                plugin,
                held_for_update: false,
            } => write!(formatter, "{plugin} is installed but disabled"),
            Self::NotEnabled {
                plugin,
                held_for_update: true,
            } => write!(
                formatter,
                "{plugin} is installed but held until you review its update"
            ),
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
}

/// Every installed plugin and the owner's grants, as one Syrinx session holds them.
pub struct OwnerRuntime {
    store: Arc<Store>,
    host_version: String,
    grants: GrantSet,
    deny: DenyList,
    added: BTreeMap<String, Added>,
    running: Option<Running>,
    /// The engine every plugin compiles on, built at start so the cache's key names it.
    engine: Engine,
    cache: Option<Arc<StoreCache>>,
    /// The one channel every `PluginEvent` arrives on: the workers', the background
    /// compiles' and the cache's; `report` is its sending end.
    events: Receiver<PluginEvent>,
    report: Sender<PluginEvent>,
}

impl OwnerRuntime {
    /// Read the owner's grants from the store and clear stale entries out of the compiled-code
    /// cache, if there is one. The engine is configured, but no plugin code runs, nothing is
    /// compiled and no thread starts until a plugin is added. A cache that cannot be used is
    /// reported through `take_events`, and plugins then compile at each activation.
    pub fn start(
        store: Arc<Store>,
        host_version: impl Into<String>,
        deny: DenyList,
        cache: Option<CacheSetup>,
    ) -> Result<Self, OwnerError> {
        let grants = from_stored(&store.plugin_grants()?);
        let host_version = host_version.into();
        let engine = crate::engine::configured_engine()?;
        let (report, events) = mpsc::channel();
        let cache = cache
            .and_then(|setup| open_cache(&store, setup, &engine, &host_version, report.clone()));
        Ok(Self {
            store,
            host_version,
            grants,
            deny,
            added: BTreeMap::new(),
            running: None,
            engine,
            cache,
            events,
            report,
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
    /// verified when it is installed (2d, #140). When the compiled-code cache has no current
    /// entry for it, as for a new install or a bundled plugin first seen, it is compiled on a
    /// background thread; this call does not wait for that.
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
            .ok_or_else(|| OwnerError::NotInstalled(id.clone()))?;
        if record.state != PluginState::Enabled {
            return Err(OwnerError::NotEnabled {
                plugin: id,
                held_for_update: record.state == PluginState::UpdateHeld,
            });
        }
        let plugins = &mut self.running()?.plugins;
        plugins.add(plugin, component)?;
        plugins.prepare_in_background(&id)?;
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

    /// Remove any installed plugin and everything keyed to it, its compiled code included.
    /// Its search feed rows and cursors go with the install record by cascade (#141); once
    /// the store kept the removal, `PluginEvent::Removed` tells the application to take the
    /// plugin's rows out of the palette and its text out of the full-text index. Recovery
    /// drafts (#37) join this when they keep anything.
    pub fn remove(&mut self, plugin: &str) -> Result<(), OwnerError> {
        self.require_record(plugin)?;
        let workers = match (self.added.remove(plugin), self.running.as_mut()) {
            (Some(_), Some(running)) => running.plugins.uninstall(plugin),
            _ => Ok(()),
        };
        self.grants.forget(plugin);
        let stored = self.store.remove_plugin(plugin.to_owned());
        if let Some(cache) = self.cache.as_ref() {
            cache.delete_entry_or_report(plugin);
        }
        if stored.is_ok() {
            let removed = PluginEvent::Removed {
                plugin: plugin.to_owned(),
            };
            send_event(&self.report, removed);
        }
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

    /// Queue `search-feed.changes(feed, after, max)` (#141). Refused unless the plugin is
    /// loaded and holds `index`; the session refuses an undeclared feed, and bounds the batch.
    pub fn pull_feed(
        &mut self,
        plugin: &str,
        feed: &str,
        after: Option<String>,
        max: u32,
    ) -> CallResult<Pending<Result<Batch, PluginError>>> {
        self.require_index(plugin)?;
        let tiers = self.declared_tiers(plugin, feed);
        let feed = feed.to_owned();
        self.call(plugin, move |session| {
            let mut answer = session.changes(&feed, after.as_deref(), max)?;
            if let Ok(batch) = &mut answer {
                // A change for a tier the manifest does not declare is dropped and logged.
                for dropped in validate::keep_declared_tiers(batch, tiers) {
                    session
                        .services_mut()
                        .log(LogLevel::Warn, format!("dropped {dropped}"));
                }
            }
            Ok(answer)
        })
    }

    /// The tiers `feed` declares in the plugin's manifest; none for a feed it does not
    /// declare, which the session refuses anyway.
    fn declared_tiers(&self, plugin: &str, feed: &str) -> FeedTiers {
        let declared = self.added.get(plugin).and_then(|added| {
            let feeds = added.manifest.search_feeds();
            feeds.iter().find(|declared| declared.id == feed)
        });
        let Some(declared) = declared else {
            return FeedTiers {
                catalogue: false,
                full_text: false,
            };
        };
        let has = |tier: &str| declared.tiers.iter().any(|declared| declared == tier);
        FeedTiers {
            catalogue: has("catalogue"),
            full_text: has("full-text"),
        }
    }

    /// Queue `search-feed.resolve(feed, id)`: the address a chosen palette row opens (#141).
    pub fn resolve_feed_entry(
        &mut self,
        plugin: &str,
        feed: &str,
        id: &str,
    ) -> CallResult<Pending<Result<String, PluginError>>> {
        self.require_index(plugin)?;
        let (feed, id) = (feed.to_owned(), id.to_owned());
        self.call(plugin, move |session| session.resolve(&feed, &id))
    }

    /// Where an added plugin is in its life; `None` for one not added this session.
    pub fn status(&self, plugin: &str) -> Option<PluginStatus> {
        self.running.as_ref()?.plugins.status(plugin)
    }

    /// What the runtime, its workers, its background compiles and its compiled-code cache
    /// reported since the last look.
    pub fn take_events(&self) -> Vec<PluginEvent> {
        self.events.try_iter().collect()
    }

    /// How many plugins were compiled and how many loaded from the compiled-code cache: a
    /// diagnostic for tests and measurements, not part of the application's contract.
    pub fn compile_counts(&self) -> CompileCounts {
        self.running
            .as_ref()
            .map_or_else(CompileCounts::default, |running| {
                running.plugins.compile_counts()
            })
    }

    /// Stop every plugin, so nothing but the caller holds the store any more.
    pub fn shutdown(mut self) {
        if let Some(running) = self.running.take() {
            running.plugins.shutdown();
        }
    }

    /// The plugin as loaded this session; otherwise whether the store knows it at all.
    fn added(&self, plugin: &str) -> Result<&Added, OwnerError> {
        if let Some(added) = self.added.get(plugin) {
            return Ok(added);
        }
        self.require_record(plugin)?;
        Err(OwnerError::NotLoaded(plugin.to_owned()))
    }

    /// The store's install record for `plugin`, in any state.
    fn record(&self, plugin: &str) -> Result<Option<PluginInstall>, OwnerError> {
        let installs = self.store.plugin_installs()?;
        Ok(installs
            .into_iter()
            .find(|install| install.plugin_id == plugin))
    }

    /// A feed is read only while the owner grants `index`, as `feed-changed` is heard.
    fn require_index(&self, plugin: &str) -> CallResult<()> {
        if self.grants.is_granted(plugin, CapabilityKind::Index) {
            return Ok(());
        }
        Err(HostError::Refused(format!(
            "{plugin} does not hold the index capability"
        )))
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
        let report = self.report.clone();
        let factory = move |manifest: &PluginManifest| {
            StoreServices::new(manifest.id(), Arc::clone(&store)).with_events(report.clone())
        };
        let mut runtime = Runtime::on(self.engine.clone())?;
        if let Some(cache) = self.cache.as_ref() {
            runtime = runtime.with_cache(Arc::clone(cache) as Arc<dyn crate::cache::CompiledCache>);
        }
        let plugins = Plugins::with_events(
            runtime,
            self.host_version.clone(),
            self.grants.clone(),
            factory,
            self.report.clone(),
        );
        Ok(Running { plugins })
    }
}

/// The compiled-code cache for `engine`, with its stale entries cleared out; `None` when it
/// cannot be used. Every problem is sent on `report`.
fn open_cache(
    store: &Arc<Store>,
    setup: CacheSetup,
    engine: &Engine,
    host_version: &str,
    report: Sender<PluginEvent>,
) -> Option<Arc<StoreCache>> {
    let opened = StoreCache::new(
        Arc::clone(store),
        setup,
        engine,
        host_version.to_owned(),
        report.clone(),
    );
    let cache = match opened {
        Ok(cache) => cache,
        Err(error) => {
            let plugin = None;
            send_event(&report, PluginEvent::Cache { plugin, error });
            return None;
        }
    };
    cache.clean();
    Some(Arc::new(cache))
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

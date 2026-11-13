//! The application runtime's own rules, on a temporary store: the records thread's budget,
//! each plugin's settings namespace, typed store errors and the grants read at start-up.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use syrinx_store::{
    DirectoryLease, LocationError, PluginGrant, PluginInstall, PluginState, Store, StoreError,
};

use super::records::PluginRecords;
use super::{DenyList, OwnerError, OwnerGrants, OwnerRuntime, StoreServices, scope_json};
use crate::bindings::syrinx::plugin::host::Host;
use crate::bindings::syrinx::plugin::types::PluginError;
use crate::deadline::{CallDeadline, Export};
use crate::grants::Grants;
use crate::manifest::{CapabilityKind, FolderCount, Scope};
use crate::{PluginHost, limits};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A database in a fresh folder of its own, removed afterwards. Twin of the lease in
/// `tests/owner.rs`; integration tests cannot share this module.
struct TempStore {
    folder: PathBuf,
    database: PathBuf,
}

impl TempStore {
    fn new() -> Self {
        let name = format!(
            "syrinx-owner-unit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let folder = std::env::temp_dir().join(name);
        fs::create_dir(&folder).expect("a fresh synthetic folder");
        let database = folder.join("syrinx.db");
        Self { folder, database }
    }

    fn open(&self) -> Arc<Store> {
        let lease = Arc::new(Lease(self.database.clone()));
        Arc::new(Store::open(lease).expect("a synthetic store"))
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        // Only the folder this test created is removed.
        if fs::remove_dir_all(&self.folder).is_err() {
            eprintln!("a synthetic store folder was left behind");
        }
    }
}

struct Lease(PathBuf);

impl DirectoryLease for Lease {
    fn database_path(&self) -> &Path {
        &self.0
    }

    fn revalidate(&self) -> Result<(), LocationError> {
        Ok(())
    }
}

fn install(plugin: &str) -> PluginInstall {
    PluginInstall {
        plugin_id: plugin.to_owned(),
        version: "0.1.0".into(),
        module_sha256: [1; 32],
        manifest_sha256: [2; 32],
        signer_key: "synthetic-test-key".into(),
        state: PluginState::Enabled,
        installed_at: 1,
        updated_at: 1,
    }
}

fn services(plugin: &str, records: &PluginRecords) -> StoreServices {
    let mut services = StoreServices::new(plugin, records.handle().expect("running"));
    services.begin_export(CallDeadline::starting_now(Export::Run));
    services
}

#[test]
fn each_plugins_services_reach_only_its_own_namespace() {
    let temp = TempStore::new();
    let store = temp.open();
    for plugin in ["first", "second"] {
        store
            .save_plugin_install(install(plugin))
            .expect("installed");
    }
    let records = PluginRecords::start(Arc::clone(&store)).expect("records thread");
    let mut first = services("first", &records);
    let mut second = services("second", &records);
    assert_eq!(
        first.settings_set("shared.key".into(), b"one".to_vec()),
        Ok(())
    );
    assert_eq!(second.settings_get("shared.key".into()), Ok(None));
    assert_eq!(
        store.plugin_setting("first", "shared.key"),
        Ok(Some(b"one".to_vec()))
    );
}

#[test]
fn store_failures_reach_the_plugin_as_typed_errors() {
    let temp = TempStore::new();
    let store = temp.open();
    let records = PluginRecords::start(Arc::clone(&store)).expect("records thread");
    let mut absent = services("absent", &records);
    assert!(
        matches!(
            absent.settings_set("key".into(), Vec::new()),
            Err(PluginError::Unavailable(_))
        ),
        "a plugin with no install row has no namespace"
    );

    store
        .save_plugin_install(install("full"))
        .expect("installed");
    let mut full = services("full", &records);
    for number in 0..limits::MAX_SETTINGS_KEYS {
        full.settings_set(format!("key.n{number}"), Vec::new())
            .expect("within the limit");
    }
    assert_eq!(
        full.settings_set("one.more".into(), Vec::new()),
        Err(PluginError::InvalidInput("too many settings keys".into()))
    );
}

#[test]
fn a_busy_store_answers_unavailable_within_the_call_budget() {
    let temp = TempStore::new();
    let store = temp.open();
    store
        .save_plugin_install(install("test"))
        .expect("installed");
    let records = PluginRecords::start(Arc::clone(&store)).expect("records thread");
    let handle = records.handle().expect("running");
    let (release, held) = mpsc::channel::<()>();
    let blocker = std::thread::spawn(move || {
        handle.run(Duration::from_secs(60), move |_store| {
            held.recv().map_err(|_| StoreError::ReplyAbandoned)
        })
    });
    // Wait until the blocker holds the records thread: a probe then times out.
    let probe = records.handle().expect("running");
    while probe.run(Duration::from_millis(1), |_store| Ok(())).is_ok() {
        std::thread::yield_now();
    }

    let mut services = services("test", &records);
    let started = Instant::now();
    assert!(matches!(
        services.settings_get("test.key".into()),
        Err(PluginError::Unavailable(_))
    ));
    assert!(started.elapsed() <= Export::Run.deadline());
    release.send(()).expect("release the blocker");
    assert!(blocker.join().expect("blocker").is_ok());
}

#[test]
fn start_reads_the_owners_grants_and_ignores_kinds_it_does_not_know() {
    let temp = TempStore::new();
    let store = temp.open();
    store
        .save_plugin_install(install("test"))
        .expect("installed");
    for capability in ["storage", "teleport"] {
        let grant = PluginGrant {
            capability: capability.into(),
            scope_json: "{}".into(),
            granted_at: 1,
            granted_for_manifest: [2; 32],
        };
        store.grant_plugin("test".into(), grant).expect("granted");
    }
    let grants = OwnerGrants::from_stored(&store.plugin_grants().expect("grants"));
    assert!(grants.is_granted("test", CapabilityKind::Storage));
    assert!(!grants.is_granted("other", CapabilityKind::Storage));
    assert_eq!(
        crate::grants::granted_kinds(&grants, "test"),
        [CapabilityKind::Storage]
    );
    let runtime = OwnerRuntime::start(store, "0.1.0-owner", DenyList::new()).expect("starts");
    assert!(runtime.grants.is_granted("test", CapabilityKind::Storage));
    assert!(runtime.take_events().is_empty(), "nothing started yet");
}

#[test]
fn owner_actions_on_a_plugin_that_is_not_installed_are_refused_by_name() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime = OwnerRuntime::start(store, "0.1.0-owner", DenyList::new()).expect("starts");
    let absent = Err(OwnerError::NotInstalled("absent".into()));
    assert_eq!(runtime.grant("absent", CapabilityKind::Storage, 1), absent);
    assert_eq!(runtime.revoke("absent", CapabilityKind::Storage), absent);
    assert_eq!(runtime.remove("absent"), absent);
}

#[test]
fn a_grant_keeps_the_scope_the_manifest_asked_for_and_never_a_path() {
    assert_eq!(scope_json(&Scope::None), "{}");
    let folder = Scope::Folder {
        writable: true,
        count: FolderCount::Limited(1),
        watch: false,
    };
    // Compared as values: the workspace build may keep JSON keys in insertion order.
    let stored: serde_json::Value = serde_json::from_str(&scope_json(&folder)).expect("JSON");
    assert_eq!(
        stored,
        serde_json::json!({ "writable": true, "count": 1, "watch": false })
    );
}

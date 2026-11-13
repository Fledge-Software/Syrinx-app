//! The test plugin under the runtime the application owns (#136, P2-04): installed, granted
//! and revoked through Syrinx's store, driven through activation, commands, views and a
//! restart, and removed with everything keyed to it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use syrinx_core::CommandRegistry;
use syrinx_plugin_host::PluginHost;
use syrinx_plugin_host::bindings::syrinx::plugin::host::Host;
use syrinx_plugin_host::bindings::syrinx::plugin::types::PluginError;
use syrinx_plugin_host::deadline::{CallDeadline, Export};
use syrinx_plugin_host::grants::{DenyList, FolderRefusal, Protected};
use syrinx_plugin_host::manifest::{CapabilityKind, DiscoveryOptions, PluginManifest};
use syrinx_plugin_host::owner::{OwnerError, OwnerRuntime, StoreServices};
use syrinx_plugin_host::session::{CallResult, Session};
use syrinx_plugin_host::worker::{PluginStatus, StopReason};
use syrinx_plugin_host::{HostError, PluginRegistry};
use syrinx_store::{
    DirectoryLease, EncodingField, FolderAccess, LocationError, PluginInstall, PluginState, Store,
    StoreError,
};

use crate::lifecycle::answer;
use crate::package::{manifest_text, register};
use crate::support::{self, FULL};

static NEXT: AtomicU64 = AtomicU64::new(0);

const STORAGE: &str = "[[capabilities]]\nkind = \"storage\"\nreason = \"Keeps synthetic settings.\"\nrequired = false\n";
const FOLDER: &str = "[[capabilities]]\nkind = \"folder\"\naccess = \"read-write\"\nreason = \"Reads a synthetic vault.\"\nrequired = false\n";
const READ_ONE_FOLDER: &str = "[[capabilities]]\nkind = \"folder\"\naccess = \"read\"\ncount = 1\nreason = \"Reads one synthetic vault.\"\nrequired = false\n";
const REQUIRED_STORAGE: &str = "[[capabilities]]\nkind = \"storage\"\nreason = \"Keeps synthetic settings.\"\nrequired = true\n";

/// A database in a fresh folder of its own, removed afterwards.
struct TempStore {
    folder: PathBuf,
    database: PathBuf,
}

impl TempStore {
    fn new() -> Self {
        let name = format!(
            "syrinx-owner-{}-{}",
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

/// The full fixture registered as `plugin`, asking for `capabilities`.
fn registry(plugin: &str, capabilities: &str) -> PluginRegistry {
    let text = manifest_text(plugin) + capabilities;
    let manifest =
        PluginManifest::parse(text.as_bytes(), DiscoveryOptions::default()).expect("manifest");
    let mut registry = PluginRegistry::new();
    register(&mut registry, manifest, &mut CommandRegistry::browser()).expect("registers");
    registry
}

fn install_record(plugin: &str) -> PluginInstall {
    PluginInstall {
        plugin_id: plugin.to_owned(),
        version: "0.1.0".into(),
        module_sha256: [1; 32],
        manifest_sha256: [2; 32],
        signer_key: "synthetic-test-key".into(),
        state: PluginState::Enabled,
        installed_at: 10,
        updated_at: 10,
    }
}

fn install(runtime: &mut OwnerRuntime, plugin: &str, capabilities: &str) {
    let registry = registry(plugin, capabilities);
    let registered = registry.get(plugin).expect("registered");
    runtime
        .install(
            registered,
            Arc::from(support::bytes(FULL)),
            install_record(plugin),
        )
        .expect("installed");
}

fn call<R: Send + 'static>(
    runtime: &mut OwnerRuntime,
    plugin: &str,
    call: impl FnOnce(&mut Session<StoreServices>) -> CallResult<R> + Send + 'static,
) -> CallResult<R> {
    answer(runtime.call(plugin, call))
}

fn echo(runtime: &mut OwnerRuntime, plugin: &str) -> CallResult<Result<(), PluginError>> {
    let command = format!("{plugin}.echo");
    call(runtime, plugin, move |session| {
        session.run_command(&command)
    })
}

fn logged(runtime: &mut OwnerRuntime, plugin: &str) -> Vec<String> {
    let logs = call(runtime, plugin, |session| {
        Ok(session.services().logs().to_vec())
    });
    logs.expect("read")
        .into_iter()
        .map(|(_, message)| message)
        .collect()
}

/// Close the runtime, then the store, as Syrinx does when it exits.
fn close(runtime: OwnerRuntime, store: Arc<Store>) {
    runtime.shutdown();
    let store = Arc::try_unwrap(store).expect("nothing else holds the store");
    store.shutdown().expect("the store closes");
}

#[test]
fn the_test_plugin_runs_through_activation_commands_views_and_a_restart() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime =
        OwnerRuntime::start(Arc::clone(&store), "0.1.0-owner", DenyList::new()).expect("starts");
    install(&mut runtime, "test", STORAGE);
    runtime
        .grant("test", CapabilityKind::Storage, 20)
        .expect("granted");

    // Activation is told the grant; the command's setting lands in the plugin's namespace.
    assert_eq!(echo(&mut runtime, "test"), Ok(Ok(())));
    assert_eq!(
        logged(&mut runtime, "test").first().map(String::as_str),
        Some("activate 0.1.0-owner granted=1")
    );
    assert_eq!(
        store.plugin_setting("test", "test.greeting"),
        Ok(Some(b"hello".to_vec()))
    );

    // A view opens, answers a message, closes, and a reopened one gets fresh request ids.
    let first = call(&mut runtime, "test", |session| {
        session.run_command("test.open")?.expect("opened");
        let request = session.queue_view_message(1, "ping", "{\"a\":1}")?;
        session.run_deferred()?;
        let reply = session.reply(request).cloned();
        session.close_view(1)?;
        Ok((request, reply, session.open_views()))
    })
    .expect("the view ran");
    assert_eq!(first.1, Some(Ok("{\"echo\":{\"a\":1}}".into())));
    assert!(first.2.is_empty(), "closed");
    let reopened = call(&mut runtime, "test", |session| {
        session.run_command("test.open")?.expect("opened");
        let view = *session.open_views().first().expect("a reopened view");
        session.queue_view_message(view, "ping", "{\"a\":2}")
    })
    .expect("the reopened view ran");
    assert!(reopened > first.0, "request ids are never reused");
    close(runtime, store);

    // After a restart the grant and the setting are still there, with no new grant.
    let store = temp.open();
    let mut runtime =
        OwnerRuntime::start(Arc::clone(&store), "0.1.0-owner", DenyList::new()).expect("restarts");
    let registry = registry("test", STORAGE);
    runtime
        .load(
            registry.get("test").expect("registered"),
            Arc::from(support::bytes(FULL)),
        )
        .expect("loaded");
    assert_eq!(
        store.plugin_setting("test", "test.greeting"),
        Ok(Some(b"hello".to_vec()))
    );
    assert_eq!(echo(&mut runtime, "test"), Ok(Ok(())));
    assert_eq!(
        logged(&mut runtime, "test").first().map(String::as_str),
        Some("activate 0.1.0-owner granted=1")
    );
    close(runtime, store);
}

#[test]
fn one_plugins_grant_is_not_anothers_and_a_revocation_stops_the_next_call() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime =
        OwnerRuntime::start(Arc::clone(&store), "0.1.0-owner", DenyList::new()).expect("starts");
    install(&mut runtime, "test", STORAGE);
    install(&mut runtime, "other", STORAGE);
    runtime
        .grant("test", CapabilityKind::Storage, 20)
        .expect("granted");
    assert_eq!(echo(&mut runtime, "test"), Ok(Ok(())));
    assert!(matches!(
        echo(&mut runtime, "other"),
        Ok(Err(PluginError::Denied(_)))
    ));
    assert_eq!(store.plugin_setting("other", "test.greeting"), Ok(None));

    runtime
        .revoke("test", CapabilityKind::Storage)
        .expect("revoked");
    assert!(matches!(
        echo(&mut runtime, "test"),
        Ok(Err(PluginError::Revoked(_)))
    ));
    assert_eq!(
        store.plugin_grants(),
        Ok(Vec::new()),
        "revoked in the store too"
    );
    close(runtime, store);
}

#[test]
fn removing_a_plugin_stops_it_and_deletes_everything_keyed_to_it() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime =
        OwnerRuntime::start(Arc::clone(&store), "0.1.0-owner", DenyList::new()).expect("starts");
    install(&mut runtime, "test", STORAGE);
    runtime
        .grant("test", CapabilityKind::Storage, 20)
        .expect("granted");
    assert_eq!(echo(&mut runtime, "test"), Ok(Ok(())));
    runtime.remove("test").expect("removed");

    assert!(matches!(
        runtime.call("test", |session| session.run_command("test.echo")),
        Err(HostError::Refused(_))
    ));
    assert_eq!(store.plugin_installs(), Ok(Vec::new()));
    assert_eq!(store.plugin_grants(), Ok(Vec::new()));
    assert_eq!(store.plugin_setting("test", "test.greeting"), Ok(None));
    close(runtime, store);
}

#[test]
fn only_requested_sandboxed_capabilities_are_granted_and_store_refusals_are_typed() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime =
        OwnerRuntime::start(Arc::clone(&store), "0.1.0-owner", DenyList::new()).expect("starts");
    let terminal = "[[capabilities]]\nkind = \"terminal\"\ntrust = \"full\"\nreason = \"Runs synthetic builds.\"\nrequired = false\n";
    install(&mut runtime, "test", &format!("{STORAGE}{terminal}"));
    assert_eq!(
        runtime.grant("test", CapabilityKind::OpenExternal, 20),
        Err(OwnerError::NotRequested(CapabilityKind::OpenExternal))
    );
    assert_eq!(
        runtime.grant("test", CapabilityKind::Terminal, 20),
        Err(OwnerError::FullTrust(CapabilityKind::Terminal))
    );
    assert_eq!(store.plugin_grants(), Ok(Vec::new()), "nothing was granted");

    let registry = registry("bad", "");
    let mut record = install_record("bad");
    record.version = String::new();
    assert_eq!(
        runtime.install(
            registry.get("bad").expect("registered"),
            Arc::from(support::bytes(FULL)),
            record
        ),
        Err(OwnerError::Store(StoreError::InvalidEncoding(
            EncodingField::Plugin
        )))
    );
    assert!(matches!(
        runtime.call("bad", |session| session.run_command("bad.echo")),
        Err(HostError::Refused(_))
    ));
    close(runtime, store);
}

#[test]
fn a_folder_is_granted_only_under_the_folder_capability_and_outside_the_deny_list() {
    let temp = TempStore::new();
    let protected = temp.folder.join("syrinx-data");
    let vault = temp.folder.join("vault");
    for folder in [&protected, &vault] {
        fs::create_dir(folder).expect("a synthetic folder");
    }
    let mut deny = DenyList::new();
    deny.protect(Protected::SyrinxData, &protected);
    let store = temp.open();
    let mut runtime = OwnerRuntime::start(Arc::clone(&store), "0.1.0-owner", deny).expect("starts");
    install(&mut runtime, "test", FOLDER);

    assert_eq!(
        runtime.grant_folder("test", &vault, FolderAccess::ReadWrite),
        Err(OwnerError::FolderNotGranted)
    );
    runtime
        .grant("test", CapabilityKind::Folder, 20)
        .expect("granted");
    assert_eq!(
        runtime.grant_folder("test", &protected, FolderAccess::ReadWrite),
        Err(OwnerError::Folder(FolderRefusal::Protected(
            Protected::SyrinxData
        )))
    );
    runtime
        .grant_folder("test", &vault, FolderAccess::ReadWrite)
        .expect("an ordinary folder is granted");
    let folders = store.plugin_folders("test").expect("folders");
    assert_eq!(folders.len(), 1);
    assert_eq!(
        folders[0].path,
        fs::canonicalize(&vault).expect("canonical")
    );
    let grants = store.plugin_grants().expect("grants");
    assert!(
        grants[0].grant.scope_json.contains("\"writable\":true"),
        "the grant keeps the manifest's scope"
    );
    close(runtime, store);
}

fn start(store: &Arc<Store>) -> OwnerRuntime {
    OwnerRuntime::start(Arc::clone(store), "0.1.0-owner", DenyList::new()).expect("starts")
}

/// What `recall` read back, from `plugin`'s log.
fn recalled(runtime: &mut OwnerRuntime, plugin: &str) -> String {
    let command = format!("{plugin}.recall");
    let ran = call(runtime, plugin, move |session| {
        session.run_command(&command)
    });
    assert_eq!(ran, Ok(Ok(())));
    logged(runtime, plugin)
        .into_iter()
        .rev()
        .find(|line| line.starts_with("recall "))
        .expect("recall reported")
}

#[test]
fn two_granted_plugins_each_keep_and_read_only_their_own_settings() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime = start(&store);
    for plugin in ["test", "other"] {
        install(&mut runtime, plugin, STORAGE);
        runtime
            .grant(plugin, CapabilityKind::Storage, 20)
            .expect("granted");
    }

    assert_eq!(echo(&mut runtime, "test"), Ok(Ok(())));
    assert_eq!(recalled(&mut runtime, "other"), "recall stored=None");
    assert_eq!(store.plugin_setting("other", "test.greeting"), Ok(None));

    store
        .save_plugin_setting("other".into(), "test.greeting".into(), b"theirs".to_vec())
        .expect("a synthetic value for the other plugin");
    assert_eq!(
        recalled(&mut runtime, "test"),
        format!("recall stored={:?}", Some(b"hello".to_vec()))
    );
    assert_eq!(
        recalled(&mut runtime, "other"),
        format!("recall stored={:?}", Some(b"theirs".to_vec()))
    );
    close(runtime, store);
}

#[test]
fn revoking_a_required_capability_stops_the_plugin_and_keeps_it_from_starting() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime = start(&store);
    install(&mut runtime, "test", REQUIRED_STORAGE);
    runtime
        .grant("test", CapabilityKind::Storage, 20)
        .expect("granted");
    assert_eq!(echo(&mut runtime, "test"), Ok(Ok(())));

    runtime
        .revoke("test", CapabilityKind::Storage)
        .expect("revoked");
    let reason = StopReason::CapabilityRevoked(CapabilityKind::Storage);
    let started = std::time::Instant::now();
    while runtime.status("test") != Some(PluginStatus::Deactivated(reason.clone())) {
        assert!(started.elapsed() < crate::lifecycle::WAIT, "never stopped");
        std::thread::yield_now();
    }
    assert!(matches!(
        runtime.call("test", |session| session.run_command("test.echo")),
        Err(HostError::NotGranted(CapabilityKind::Storage))
    ));
    close(runtime, store);
}

#[test]
fn a_plugin_that_is_installed_but_not_running_can_be_revoked_and_removed() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut record = install_record("held");
    record.state = PluginState::Disabled;
    store
        .save_plugin_install(record)
        .expect("a disabled install");
    let grant = syrinx_store::PluginGrant {
        capability: "storage".into(),
        scope_json: "{}".into(),
        granted_at: 1,
        granted_for_manifest: [2; 32],
    };
    store.grant_plugin("held".into(), grant).expect("granted");
    store
        .save_plugin_setting("held".into(), "held.key".into(), vec![1])
        .expect("a setting");
    let mut runtime = start(&store);

    let registry = registry("held", STORAGE);
    assert_eq!(
        runtime.load(
            registry.get("held").expect("registered"),
            Arc::from(support::bytes(FULL))
        ),
        Err(OwnerError::NotInstalled("held".into())),
        "a disabled plugin is not loaded"
    );
    assert_eq!(
        runtime.grant("held", CapabilityKind::Storage, 2),
        Err(OwnerError::NotLoaded("held".into()))
    );
    assert!(matches!(
        runtime.call("held", |session| session.run_command("held.echo")),
        Err(HostError::Refused(ref reason)) if reason == "held is not loaded this session"
    ));
    runtime
        .revoke("held", CapabilityKind::Storage)
        .expect("revoked without a worker");
    assert_eq!(store.plugin_grants(), Ok(Vec::new()));
    runtime.remove("held").expect("removed without a worker");
    assert_eq!(store.plugin_installs(), Ok(Vec::new()));
    assert_eq!(store.plugin_setting("held", "held.key"), Ok(None));
    assert_eq!(
        runtime.remove("held"),
        Err(OwnerError::NotInstalled("held".into()))
    );
    close(runtime, store);
}

#[test]
fn installing_again_or_with_another_plugins_record_is_refused_before_writing() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime = start(&store);
    install(&mut runtime, "test", STORAGE);
    let registry = registry("test", STORAGE);
    let registered = registry.get("test").expect("registered");

    let mut update = install_record("test");
    update.version = "0.2.0".into();
    assert_eq!(
        runtime.install(registered, Arc::from(support::bytes(FULL)), update),
        Err(OwnerError::AlreadyAdded("test".into()))
    );
    assert_eq!(store.plugin_installs(), Ok(vec![install_record("test")]));
    assert_eq!(
        runtime.install(
            registered,
            Arc::from(support::bytes(FULL)),
            install_record("other")
        ),
        Err(OwnerError::RecordMismatch {
            record: "other".into(),
            plugin: "test".into()
        })
    );
    close(runtime, store);
}

#[test]
fn a_folder_grant_never_widens_past_what_the_manifest_asks() {
    let temp = TempStore::new();
    let first = temp.folder.join("first");
    let second = temp.folder.join("second");
    for folder in [&first, &second] {
        fs::create_dir(folder).expect("a synthetic folder");
    }
    let store = temp.open();
    let mut runtime = start(&store);
    install(&mut runtime, "test", READ_ONE_FOLDER);
    runtime
        .grant("test", CapabilityKind::Folder, 20)
        .expect("granted");

    assert_eq!(
        runtime.grant_folder("test", &first, FolderAccess::ReadWrite),
        Err(OwnerError::ReadWriteNotRequested)
    );
    runtime
        .grant_folder("test", &first, FolderAccess::Read)
        .expect("one folder to read");
    runtime
        .grant_folder("test", &first, FolderAccess::Read)
        .expect("the same folder again");
    assert_eq!(
        runtime.grant_folder("test", &second, FolderAccess::Read),
        Err(OwnerError::TooManyFolders(1))
    );
    runtime
        .revoke("test", CapabilityKind::Folder)
        .expect("revoked");
    assert_eq!(
        runtime.grant_folder("test", &second, FolderAccess::Read),
        Err(OwnerError::FolderNotGranted)
    );
    assert_eq!(store.plugin_folders("test"), Ok(Vec::new()));
    close(runtime, store);
}

#[test]
fn store_failures_reach_the_plugin_as_typed_errors() {
    let temp = TempStore::new();
    let store = temp.open();
    let deadline = CallDeadline::starting_now(Export::Run);
    let mut absent = StoreServices::new("absent", Arc::clone(&store));
    absent.begin_export(deadline);
    assert!(
        matches!(
            absent.settings_set("key".into(), Vec::new()),
            Err(PluginError::Unavailable(_))
        ),
        "a plugin with no install record has no namespace"
    );

    store
        .save_plugin_install(install_record("full"))
        .expect("installed");
    let mut full = StoreServices::new("full", Arc::clone(&store));
    for number in 0..syrinx_plugin_host::limits::MAX_SETTINGS_KEYS {
        full.begin_export(CallDeadline::starting_now(Export::Run));
        full.settings_set(format!("key.n{number}"), Vec::new())
            .expect("within the limit");
    }
    full.begin_export(CallDeadline::starting_now(Export::Run));
    assert_eq!(
        full.settings_set("one.more".into(), Vec::new()),
        Err(PluginError::InvalidInput("too many settings keys".into()))
    );
    drop((absent, full));
    let store = Arc::try_unwrap(store).expect("nothing else holds the store");
    store.shutdown().expect("the store closes");
}

#[test]
fn a_store_call_past_its_budget_reaches_the_plugin_as_unavailable() {
    let temp = TempStore::new();
    let store = temp.open();
    store
        .save_plugin_install(install_record("test"))
        .expect("installed");
    let mut services = StoreServices::new("test", Arc::clone(&store));
    // The shortest export deadline, already over: the service's budget is nothing.
    let spent = CallDeadline::starting_now(Export::Validate);
    std::thread::sleep(Export::Validate.deadline());
    services.begin_export(spent);
    let started = std::time::Instant::now();
    assert!(matches!(
        services.settings_get("test.key".into()),
        Err(PluginError::Unavailable(_))
    ));
    assert!(matches!(
        services.settings_set("test.key".into(), vec![1]),
        Err(PluginError::Unavailable(_))
    ));
    assert!(
        started.elapsed() < Export::Validate.deadline(),
        "neither waited for the store"
    );
    assert_eq!(store.plugin_setting("test", "test.key"), Ok(None));
    drop(services);
    let store = Arc::try_unwrap(store).expect("nothing else holds the store");
    store.shutdown().expect("the store closes");
}

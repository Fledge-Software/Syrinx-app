//! Proves the session starts the plugin runtime from its own store at start-up (#136), that a
//! grant it cannot read never stops browsing, that the test plugin runs through the
//! session's own route: activation, a command, a view opened and closed, and shutdown, that
//! its compiled code is kept under the real SHA-256 and loaded after a restart (#137), and
//! that the runtime's events, cache problems included, are reported once, without plugin
//! text, as the loop wakes and at close (#268).

use syrinx_core::CommandRegistry;
use syrinx_plugin_host::bindings::syrinx::plugin::types::Outcome;
use syrinx_plugin_host::bindings::syrinx::plugin::types::PluginError;
use syrinx_plugin_host::deadline::Export;
use syrinx_plugin_host::manifest::{
    CapabilityKind, DiscoveryOptions, MANIFEST_FILE, PluginManifest,
};
use syrinx_plugin_host::owner::{OwnerError, OwnerRuntime};
use syrinx_plugin_host::session::CallResult;
use syrinx_plugin_host::worker::{PluginEvent, PluginStatus, StopReason};
use syrinx_plugin_host::{
    CacheError, CompileCounts, Failure, HostError, PluginRegistry, Refusal, SignedIdentity,
    check_signed,
};
use syrinx_store::{PluginInstall, PluginState};

use super::*;

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";

#[path = "../../tests/support/plugin_fixture.rs"]
mod plugin_fixture;

use plugin_fixture::fixture_workspace;

/// The full fixture's component, built into the folder the integration tests use
/// (`<target>/tmp/plugin-fixtures`); this test binary is `<target>/<profile>/deps/<name>`.
pub(super) fn full_component() -> &'static [u8] {
    let executable = std::env::current_exe().expect("the test binary's path");
    let target = executable.ancestors().nth(3).expect("the target folder");
    plugin_fixture::full_component(&target.join("tmp").join("plugin-fixtures"))
}

/// The full fixture's real manifest, asking for `storage`, registered with matching lines.
pub(super) fn registered_fixture(
    registry: &mut PluginRegistry,
) -> &syrinx_plugin_host::RegisteredPlugin {
    registered_fixture_requiring_storage(registry, false)
}

/// `registered_fixture`, with `storage` marked `required` or not.
pub(super) fn registered_fixture_requiring_storage(
    registry: &mut PluginRegistry,
    required: bool,
) -> &syrinx_plugin_host::RegisteredPlugin {
    registered_fixture_declaring(registry, required, "")
}

/// `registered_fixture_requiring_storage`, with `extra` manifest text appended.
pub(super) fn registered_fixture_declaring<'registry>(
    registry: &'registry mut PluginRegistry,
    required: bool,
    extra: &str,
) -> &'registry syrinx_plugin_host::RegisteredPlugin {
    let mut text = fs::read_to_string(fixture_workspace().join("full").join(MANIFEST_FILE))
        .expect("the fixture's manifest");
    text.push_str(&format!(
        "\n[[capabilities]]\nkind = \"storage\"\nreason = \"Keeps synthetic settings.\"\nrequired = {required}\n",
    ));
    text.push_str(extra);
    let manifest =
        PluginManifest::parse(text.as_bytes(), DiscoveryOptions::default()).expect("manifest");
    let lines = SignedIdentity::from_signed_lines(manifest.id(), manifest.version(), WORLD);
    let signed = check_signed(&lines, manifest).expect("the lines match the manifest");
    registry
        .register(signed, &mut CommandRegistry::browser())
        .expect("registers")
}

/// Install the full fixture's manifest with `component` under `record`.
fn install_fixture(runtime: &mut OwnerRuntime, component: Arc<[u8]>, record: PluginInstall) {
    install_fixture_declaring(runtime, component, record, "");
}

/// `install_fixture`, with `extra` manifest text appended.
fn install_fixture_declaring(
    runtime: &mut OwnerRuntime,
    component: Arc<[u8]>,
    record: PluginInstall,
    extra: &str,
) {
    let mut registry = PluginRegistry::new();
    let registered = registered_fixture_declaring(&mut registry, false, extra);
    runtime
        .install(registered, component, record)
        .expect("installed");
}

pub(super) fn install_record() -> PluginInstall {
    PluginInstall {
        plugin_id: "test".to_owned(),
        version: "0.1.0".to_owned(),
        module_sha256: [1; 32],
        manifest_sha256: [2; 32],
        signer_key: "synthetic-test-key".to_owned(),
        state: PluginState::Enabled,
        installed_at: 10,
        updated_at: 10,
    }
}

/// The session's store and plugin runtime over the fixture's folder, with the cache folder
/// beside the database, as `serve_accepted` starts them.
pub(super) fn started(fixture: &Fixture) -> (Arc<Store>, plugins::SessionPlugins) {
    let (_lease, store, session_plugins) = started_with_lease(fixture);
    (store, session_plugins)
}

/// As `started`, keeping the lease for a test that also opens the session's other parts.
fn started_with_lease(
    fixture: &Fixture,
) -> (
    Arc<WindowsDirectoryLease>,
    Arc<Store>,
    plugins::SessionPlugins,
) {
    let lease = Arc::new(WindowsDirectoryLease::prepare(&fixture.0).expect("the synthetic folder"));
    let store = Arc::new(Store::open(lease.clone()).expect("the synthetic store"));
    let cache_folder = lease.cache_folder(plugins::CACHE_FOLDER);
    let (session_plugins, problems) = plugins::start(&store, &fixture.0, cache_folder);
    assert!(problems.is_empty(), "{problems:?}");
    (lease, store, session_plugins.expect("the runtime starts"))
}

/// Activate the test plugin with an empty call; it answers once activation is done.
fn activate(runtime: &mut OwnerRuntime) {
    let answered = runtime
        .call("test", |_session| Ok(()))
        .expect("queued")
        .wait(Duration::from_secs(60))
        .expect("answered");
    assert_eq!(answered, Ok(()));
}

#[test]
fn the_test_plugin_runs_through_the_sessions_own_route() {
    let fixture = Fixture::new();
    create_database(&fixture);
    let (store, mut session_plugins) = started(&fixture);
    let runtime = &mut session_plugins.runtime;
    install_fixture(runtime, Arc::from(full_component()), install_record());
    runtime
        .grant("test", CapabilityKind::Storage, 20)
        .expect("granted");

    let wait = Duration::from_secs(60);
    let ran = runtime
        .call("test", |session| {
            let echoed = session.run_command("test.echo")?;
            session.run_command("test.open")?.expect("opened");
            let opened = session.open_views();
            let view = *opened.first().expect("a view opened");
            session.close_view(view)?;
            Ok((echoed, opened.len(), session.open_views()))
        })
        .expect("queued")
        .wait(wait)
        .expect("answered");
    let (echoed, opened, still_open): (Result<(), PluginError>, usize, Vec<u64>) =
        ran.expect("ran");
    assert_eq!(echoed, Ok(()));
    assert_eq!(opened, 1);
    assert!(still_open.is_empty(), "closed");
    assert_eq!(
        store.plugin_setting("test", "test.greeting"),
        Ok(Some(b"hello".to_vec()))
    );
    assert_eq!(plugins::close(Some(session_plugins), store), Ok(()));
    fixture.cleanup();
}

/// A fresh synthetic session with the full fixture installed and granted storage, and its lease.
fn installed(
    fixture: &Fixture,
) -> (
    Arc<WindowsDirectoryLease>,
    Arc<Store>,
    plugins::SessionPlugins,
) {
    installed_declaring(fixture, "")
}

/// `installed`, with `extra` manifest text appended to the fixture's manifest.
pub(super) fn installed_declaring(
    fixture: &Fixture,
    extra: &str,
) -> (
    Arc<WindowsDirectoryLease>,
    Arc<Store>,
    plugins::SessionPlugins,
) {
    create_database(fixture);
    let (lease, store, mut session_plugins) = started_with_lease(fixture);
    let runtime = &mut session_plugins.runtime;
    let component = Arc::from(full_component());
    install_fixture_declaring(runtime, component, install_record(), extra);
    // Installing also starts a background compile, which reports nothing when it succeeds.
    // `test.echo` keeps a synthetic setting, so it needs storage.
    runtime
        .grant("test", CapabilityKind::Storage, 20)
        .expect("granted");
    (lease, store, session_plugins)
}

/// Run one of the fixture's commands and wait for its answer.
fn run(runtime: &mut OwnerRuntime, command: &'static str) -> CallResult<Result<(), PluginError>> {
    runtime
        .call("test", move |session| session.run_command(command))
        .expect("queued")
        .wait(Duration::from_secs(60))
        .expect("answered")
}

/// Make the fixture trap, wait until the host shows it failed, then use it again. The new
/// worker joins the trapped one before it activates (the host's `wait_for_predecessor`), so
/// once this returns the trap's event is queued and nothing has taken it.
fn trap_and_restart(runtime: &mut OwnerRuntime) {
    assert!(run(runtime, "test.panic").is_err(), "the panic traps");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !matches!(runtime.status("test"), Some(PluginStatus::Failed(_))) {
        assert!(Instant::now() < deadline, "the trap is never shown");
        std::thread::sleep(Duration::from_millis(10));
    }
    let again = run(runtime, "test.echo");
    assert!(
        matches!(again, Ok(Ok(()))),
        "used again after the trap: {again:?}"
    );
}

const TRAPPED: &str = "Plugin test stopped: commands.run trapped";

#[test]
fn a_trap_is_reported_once_and_the_next_drain_is_empty() {
    let fixture = Fixture::new();
    let (lease, store, mut session_plugins) = installed(&fixture);
    trap_and_restart(&mut session_plugins.runtime);
    assert_eq!(
        plugins::drain(&session_plugins.runtime),
        vec![TRAPPED.to_owned()]
    );
    assert!(
        plugins::drain(&session_plugins.runtime).is_empty(),
        "nothing is reported twice"
    );
    assert_eq!(plugins::close(Some(session_plugins), store), Ok(()));
    drop(lease);
    fixture.cleanup();
}

#[test]
fn the_session_loop_empties_the_plugin_queue_as_it_wakes() {
    let fixture = Fixture::new();
    let (lease, store, mut session_plugins) = installed(&fixture);
    trap_and_restart(&mut session_plugins.runtime);
    assert_eq!(
        session_plugins.runtime.status("test"),
        Some(PluginStatus::Active),
        "restarted"
    );
    let journal = DeletionJournal::open(&lease).expect("open synthetic journal");
    let page_text = page_text::PageText::open(&lease);
    let mut worker = Worker::restore(&store, journal, page_text).expect("restore synthetic worker");
    worker.plugins = Some(session_plugins);
    // One ordinary command, then the window goes away: the real loop wakes for each.
    let (commands, receiver) = mpsc::channel();
    commands
        .send(SessionCommand::NewTab {
            address: "about:blank".to_owned(),
        })
        .expect("queue a synthetic command");
    drop(commands);
    let (sender, _events) = unbounded();
    let (mut usage, _) =
        UsageCollector::load(Arc::default(), &fixture.0, local_today(), Instant::now());
    assert_eq!(worker.serve(&store, receiver, &sender, &mut usage), Ok(()));
    let session_plugins = worker.plugins.take().expect("the worker keeps the runtime");
    assert!(
        plugins::drain(&session_plugins.runtime).is_empty(),
        "the loop took the queued trap"
    );
    drop(worker);
    assert_eq!(plugins::close(Some(session_plugins), store), Ok(()));
    drop(lease);
    fixture.cleanup();
}

/// Text only a plugin, a service or the system could have written, which no line may carry.
const SENTINEL: &str = "/home/owner.invalid/secret-7f3a";

#[test]
fn every_event_is_described_without_text_from_the_plugin_or_the_system() {
    let plugin = || "test".to_owned();
    let refused = || PluginError::Internal(SENTINEL.to_owned());
    let failed = |failure| PluginEvent::Failed {
        plugin: plugin(),
        failure,
    };
    let settled = |outcome| PluginEvent::Outcome {
        plugin: plugin(),
        request: 7,
        outcome,
    };
    let stopped = |reason| PluginEvent::Deactivated {
        plugin: plugin(),
        reason,
    };
    let cached = |error| PluginEvent::Cache {
        plugin: Some(plugin()),
        error,
    };
    let refused_entry = |refusal| cached(CacheError::Refused(refusal));
    let events = vec![
        settled(Outcome::Done(Some(SENTINEL.to_owned()))),
        settled(Outcome::Failed(refused())),
        settled(Outcome::Uncertain(SENTINEL.to_owned())),
        failed(Failure::DeadlineExpired(Export::Run)),
        failed(Failure::Trapped {
            export: Export::Activate,
            code: None,
        }),
        failed(Failure::Flooded),
        failed(Failure::OverBudget),
        failed(Failure::Load(HostError::Unavailable(SENTINEL.to_owned()))),
        failed(Failure::ActivationRefused(refused())),
        stopped(StopReason::Shutdown),
        stopped(StopReason::Uninstalled),
        stopped(StopReason::CapabilityRevoked(CapabilityKind::Storage)),
        refused_entry(Refusal::Missing),
        refused_entry(Refusal::Length),
        refused_entry(Refusal::Hash),
        refused_entry(Refusal::Deserialize(SENTINEL.to_owned())),
        refused_entry(Refusal::Checks(SENTINEL.to_owned())),
        refused_entry(Refusal::OtherBytes),
        cached(CacheError::File(std::io::ErrorKind::PermissionDenied)),
        cached(CacheError::NotAFile),
        PluginEvent::Cache {
            plugin: Some(format!("../{SENTINEL}\nPlugin forged")),
            error: CacheError::Unnamed,
        },
        cached(CacheError::Store(SENTINEL.to_owned())),
        cached(CacheError::Hash(SENTINEL.to_owned())),
        PluginEvent::Cache {
            plugin: None,
            error: CacheError::Serialize(SENTINEL.to_owned()),
        },
        PluginEvent::PrepareFailed {
            plugin: plugin(),
            error: HostError::Malformed(SENTINEL.to_owned()),
        },
    ];
    let lines = plugins::describe_all(&events);
    assert_eq!(
        lines,
        [
            "Plugin test: write 7 was settled by the host as done",
            "Plugin test: write 7 was settled by the host as failed",
            "Plugin test: write 7 was settled by the host as uncertain",
            "Plugin test stopped: commands.run ran past its deadline",
            "Plugin test stopped: lifecycle.activate trapped",
            "Plugin test stopped: it kept queueing deferred calls",
            "Plugin test stopped: one call and its deferred calls took too long",
            "Plugin test stopped: it could not be compiled, linked or started",
            "Plugin test stopped: it refused to activate",
            "Plugin test was deactivated: Syrinx is closing",
            "Plugin test was deactivated: the plugin was uninstalled",
            "Plugin test was deactivated: the required storage capability was revoked",
            "Plugin test: its compiled code was not kept or used: an entry was refused (its file is missing)",
            "Plugin test: its compiled code was not kept or used: an entry was refused (its length is wrong)",
            "Plugin test: its compiled code was not kept or used: an entry was refused (its SHA-256 is wrong)",
            "Plugin test: its compiled code was not kept or used: an entry was refused (wasmtime could not load it)",
            "Plugin test: its compiled code was not kept or used: an entry was refused (it failed the checks a compile passes)",
            "Plugin test: its compiled code was not kept or used: an entry was refused (it was compiled from other bytes)",
            "Plugin test: its compiled code was not kept or used: a cache file failed (permission denied)",
            "Plugin test: its compiled code was not kept or used: something other than a file is in its folder",
            "Plugin (an id that cannot be shown): its compiled code was not kept or used: the plugin id cannot name a cache file",
            "Plugin test: its compiled code was not kept or used: the store could not read or keep its record",
            "Plugin test: its compiled code was not kept or used: its SHA-256 could not be computed",
            "The compiled plugin code cache could not be used: the compiled code could not be written out",
            "Plugin test could not be compiled in the background; its first use tries again",
        ]
    );
    assert!(lines.iter().all(|line| !line.contains("secret-7f3a")));
    assert!(
        lines.iter().all(|line| !line.contains('\n')),
        "one line each"
    );
}

#[test]
fn shutdown_hands_back_what_stopping_the_plugins_raised() {
    let fixture = Fixture::new();
    let (lease, store, mut session_plugins) = installed(&fixture);
    assert!(
        matches!(run(&mut session_plugins.runtime, "test.echo"), Ok(Ok(()))),
        "the fixture is active"
    );
    assert_eq!(
        plugins::stop(session_plugins),
        vec!["Plugin test was deactivated: Syrinx is closing".to_owned()]
    );
    let store = Arc::try_unwrap(store).unwrap_or_else(|_| panic!("nothing else holds the store"));
    assert_eq!(store.shutdown(), Ok(()));
    drop(lease);
    fixture.cleanup();
}

#[test]
fn compiled_code_is_kept_under_its_sha256_and_loaded_after_a_restart() {
    let fixture = Fixture::new();
    create_database(&fixture);
    let component = full_component();
    let record = PluginInstall {
        module_sha256: syrinx_update::sha256(component).expect("SHA-256"),
        ..install_record()
    };
    let (store, mut session_plugins) = started(&fixture);
    install_fixture(&mut session_plugins.runtime, Arc::from(component), record);
    activate(&mut session_plugins.runtime);
    let compiled = CompileCounts {
        compiled: 1,
        loaded: 0,
    };
    assert_eq!(session_plugins.runtime.compile_counts(), compiled);
    assert!(session_plugins.runtime.take_events().is_empty());
    let kept = store
        .plugin_compiled("test")
        .expect("the record reads")
        .expect("a record");
    assert_eq!(kept.syrinx_version, env!("CARGO_PKG_VERSION"));
    let entry = fixture
        .0
        .join(plugins::CACHE_FOLDER)
        .join("plugin-test.cwasm");
    let bytes = fs::read(&entry).expect("the cached entry");
    assert_eq!(
        kept.artifact_sha256,
        syrinx_update::sha256(&bytes).expect("SHA-256")
    );
    assert_eq!(plugins::close(Some(session_plugins), store), Ok(()));

    let (store, mut session_plugins) = started(&fixture);
    let mut registry = PluginRegistry::new();
    let registered = registered_fixture(&mut registry);
    session_plugins
        .runtime
        .load(registered, Arc::from(component))
        .expect("loaded");
    activate(&mut session_plugins.runtime);
    let loaded = CompileCounts {
        compiled: 0,
        loaded: 1,
    };
    assert_eq!(session_plugins.runtime.compile_counts(), loaded);
    assert_eq!(plugins::close(Some(session_plugins), store), Ok(()));
    fixture.cleanup();
}

/// Install the fixture with its real SHA-256 and activate it, leaving a warm cache entry.
fn warm_cache(fixture: &Fixture, component: &[u8]) {
    let record = PluginInstall {
        module_sha256: syrinx_update::sha256(component).expect("SHA-256"),
        ..install_record()
    };
    let (store, mut session_plugins) = started(fixture);
    install_fixture(&mut session_plugins.runtime, Arc::from(component), record);
    activate(&mut session_plugins.runtime);
    assert!(plugins::drain(&session_plugins.runtime).is_empty());
    assert_eq!(plugins::close(Some(session_plugins), store), Ok(()));
}

#[test]
fn a_damaged_cache_entry_is_reported_while_the_plugin_still_loads() {
    let fixture = Fixture::new();
    create_database(&fixture);
    let component = full_component();
    warm_cache(&fixture, component);
    // One flipped byte in the middle of the kept entry: only its SHA-256 shows it.
    let entry = fixture
        .0
        .join(plugins::CACHE_FOLDER)
        .join("plugin-test.cwasm");
    let mut bytes = fs::read(&entry).expect("the cached entry");
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x01;
    fs::write(&entry, bytes).expect("rewrite the cached entry");

    let (store, mut session_plugins) = started(&fixture);
    let mut registry = PluginRegistry::new();
    let registered = registered_fixture(&mut registry);
    session_plugins
        .runtime
        .load(registered, Arc::from(component))
        .expect("loaded");
    activate(&mut session_plugins.runtime);
    let recompiled = CompileCounts {
        compiled: 1,
        loaded: 0,
    };
    assert_eq!(session_plugins.runtime.compile_counts(), recompiled);
    assert_eq!(
        plugins::drain(&session_plugins.runtime),
        vec![
            "Plugin test: its compiled code was not kept or used: an entry was refused (its SHA-256 is wrong)"
                .to_owned()
        ]
    );
    assert_eq!(
        session_plugins.runtime.status("test"),
        Some(PluginStatus::Active),
        "the plugin compiled and runs"
    );
    assert_eq!(plugins::close(Some(session_plugins), store), Ok(()));
    fixture.cleanup();
}

const PREPARE_FAILED: &str =
    "Plugin test could not be compiled in the background; its first use tries again";

#[test]
fn a_failed_background_compile_is_reported() {
    let fixture = Fixture::new();
    create_database(&fixture);
    let (store, mut session_plugins) = started(&fixture);
    // Bytes no compiler accepts, under the fixture's real manifest.
    install_fixture(
        &mut session_plugins.runtime,
        Arc::from(&b"not a component"[..]),
        install_record(),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut lines = Vec::new();
    while !lines.iter().any(|line| line == PREPARE_FAILED) {
        assert!(
            Instant::now() < deadline,
            "the failed compile is never reported"
        );
        std::thread::sleep(Duration::from_millis(10));
        lines.extend(plugins::drain(&session_plugins.runtime));
    }
    lines.extend(plugins::drain(&session_plugins.runtime));
    assert_eq!(lines, vec![PREPARE_FAILED.to_owned()], "nothing else");
    assert_eq!(plugins::close(Some(session_plugins), store), Ok(()));
    fixture.cleanup();
}

/// Starts and stops a fresh worker, so the database exists at the current schema.
pub(super) fn create_database(fixture: &Fixture) {
    let (handle, mut events) = fixture.start();
    state(&mut events);
    stop(handle, &mut events);
}

#[test]
fn an_unreadable_grant_is_reported_and_browsing_goes_on() {
    let fixture = Fixture::new();
    create_database(&fixture);
    let connection =
        rusqlite::Connection::open(fixture.0.join("syrinx.db")).expect("open synthetic database");
    connection
        .execute_batch(
            "INSERT INTO plugin_install VALUES
                 ('synthetic', '0.1.0', zeroblob(32), zeroblob(32), 'key', 1, 0, 0);
             INSERT INTO plugin_grant(plugin_id, capability, scope_json, granted_at,
                 granted_for_manifest)
                 VALUES ('synthetic', 'Damaged', '{}', 0, zeroblob(32));",
        )
        .expect("seed a damaged synthetic grant");
    assert!(connection.close().is_ok(), "close synthetic connection");

    let (handle, mut events) = fixture.start();
    let reported = next_any_event(&mut events);
    let expected = SessionError::Plugins(OwnerError::Store(StoreError::InvalidEncoding(
        EncodingField::Plugin,
    )));
    assert!(
        matches!(&reported, SessionEvent::Failure { error, fatal: false } if *error == expected),
        "{reported:?}"
    );
    state(&mut events);
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: "about:blank".to_owned(),
        },
    );
    assert_eq!(opened.model.tabs.len(), 1);
    stop(handle, &mut events);
    fixture.cleanup();
}

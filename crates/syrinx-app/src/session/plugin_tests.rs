//! Proves the session starts the plugin runtime from its own store at start-up (#136), that a
//! grant it cannot read never stops browsing, and that the test plugin runs through the
//! session's own route: activation, a command, a view opened and closed, and shutdown; and that
//! the workers' events are reported once, without plugin text, as the loop wakes and at close
//! (#268).

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
use syrinx_plugin_host::{Failure, HostError};
use syrinx_plugin_host::{PluginRegistry, SignedIdentity, check_signed};
use syrinx_store::{PluginInstall, PluginState};

use super::*;

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";

#[path = "../../tests/support/plugin_fixture.rs"]
mod plugin_fixture;

use plugin_fixture::fixture_workspace;

/// The full fixture's component, built into the folder the integration tests use
/// (`<target>/tmp/plugin-fixtures`); this test binary is `<target>/<profile>/deps/<name>`.
fn full_component() -> &'static [u8] {
    let executable = std::env::current_exe().expect("the test binary's path");
    let target = executable.ancestors().nth(3).expect("the target folder");
    plugin_fixture::full_component(&target.join("tmp").join("plugin-fixtures"))
}

/// The full fixture's real manifest, asking for `storage`, registered with matching lines.
fn registered_fixture(registry: &mut PluginRegistry) -> &syrinx_plugin_host::RegisteredPlugin {
    let mut text = fs::read_to_string(fixture_workspace().join("full").join(MANIFEST_FILE))
        .expect("the fixture's manifest");
    text.push_str(
        "\n[[capabilities]]\nkind = \"storage\"\nreason = \"Keeps synthetic settings.\"\nrequired = false\n",
    );
    let manifest =
        PluginManifest::parse(text.as_bytes(), DiscoveryOptions::default()).expect("manifest");
    let lines = SignedIdentity::from_signed_lines(manifest.id(), manifest.version(), WORLD);
    let signed = check_signed(&lines, manifest).expect("the lines match the manifest");
    registry
        .register(signed, &mut CommandRegistry::browser())
        .expect("registers")
}

fn install_record() -> PluginInstall {
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

#[test]
fn the_test_plugin_runs_through_the_sessions_own_route() {
    let fixture = Fixture::new();
    let (lease, store) = synthetic_store(&fixture);
    let mut runtime = installed_runtime(&store, &fixture);

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
    assert_eq!(plugins::close(Some(runtime), store), Ok(()));
    drop(lease);
    fixture.cleanup();
}

/// A runtime over `store` with the full fixture installed and granted storage.
fn installed_runtime(store: &Arc<Store>, fixture: &Fixture) -> OwnerRuntime {
    let mut runtime = plugins::start(store, &fixture.0).expect("the runtime starts");
    let mut registry = PluginRegistry::new();
    let registered = registered_fixture(&mut registry);
    runtime
        .install(registered, Arc::from(full_component()), install_record())
        .expect("installed");
    // `test.echo` keeps a synthetic setting, so it needs storage.
    runtime
        .grant("test", CapabilityKind::Storage, 20)
        .expect("granted");
    runtime
}

/// A fresh synthetic store at the current schema, and its lease.
fn synthetic_store(fixture: &Fixture) -> (Arc<WindowsDirectoryLease>, Arc<Store>) {
    create_database(fixture);
    let lease = Arc::new(WindowsDirectoryLease::prepare(&fixture.0).expect("the synthetic folder"));
    let store = Arc::new(Store::open(lease.clone()).expect("the synthetic store"));
    (lease, store)
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
    let (lease, store) = synthetic_store(&fixture);
    let mut runtime = installed_runtime(&store, &fixture);
    trap_and_restart(&mut runtime);
    assert_eq!(plugins::drain(&runtime), vec![TRAPPED.to_owned()]);
    assert!(
        plugins::drain(&runtime).is_empty(),
        "nothing is reported twice"
    );
    assert_eq!(plugins::close(Some(runtime), store), Ok(()));
    drop(lease);
    fixture.cleanup();
}

#[test]
fn the_session_loop_empties_the_plugin_queue_as_it_wakes() {
    let fixture = Fixture::new();
    let (lease, store) = synthetic_store(&fixture);
    let mut runtime = installed_runtime(&store, &fixture);
    trap_and_restart(&mut runtime);
    assert_eq!(
        runtime.status("test"),
        Some(PluginStatus::Active),
        "restarted"
    );
    let journal = DeletionJournal::open(&lease).expect("open synthetic journal");
    let page_text = page_text::PageText::open(&lease);
    let mut worker = Worker::restore(&store, journal, page_text).expect("restore synthetic worker");
    worker.plugins = Some(runtime);
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
    let runtime = worker.plugins.take().expect("the worker keeps the runtime");
    assert!(
        plugins::drain(&runtime).is_empty(),
        "the loop took the queued trap"
    );
    drop(worker);
    assert_eq!(plugins::close(Some(runtime), store), Ok(()));
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
        ]
    );
    assert!(lines.iter().all(|line| !line.contains("secret-7f3a")));
}

#[test]
fn shutdown_hands_back_what_stopping_the_plugins_raised() {
    let fixture = Fixture::new();
    let (lease, store) = synthetic_store(&fixture);
    let mut runtime = installed_runtime(&store, &fixture);
    assert!(
        matches!(run(&mut runtime, "test.echo"), Ok(Ok(()))),
        "the fixture is active"
    );
    assert_eq!(
        plugins::describe_all(&runtime.shutdown()),
        vec!["Plugin test was deactivated: Syrinx is closing".to_owned()]
    );
    let store = Arc::try_unwrap(store).unwrap_or_else(|_| panic!("nothing else holds the store"));
    assert_eq!(store.shutdown(), Ok(()));
    drop(lease);
    fixture.cleanup();
}

/// Starts and stops a fresh worker, so the database exists at the current schema.
fn create_database(fixture: &Fixture) {
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

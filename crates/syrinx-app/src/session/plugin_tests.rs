//! Proves the session starts the plugin runtime from its own store at start-up (#136), that a
//! grant it cannot read never stops browsing, and that the test plugin runs through the
//! session's own route: activation, a command, a view opened and closed, and shutdown.

use syrinx_core::CommandRegistry;
use syrinx_plugin_host::bindings::syrinx::plugin::types::PluginError;
use syrinx_plugin_host::manifest::{
    CapabilityKind, DiscoveryOptions, MANIFEST_FILE, PluginManifest,
};
use syrinx_plugin_host::owner::OwnerError;
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
    create_database(&fixture);
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("the synthetic folder");
    let store = Arc::new(Store::open(Arc::new(lease)).expect("the synthetic store"));
    let mut runtime = plugins::start(&store, &fixture.0).expect("the runtime starts");
    let mut registry = PluginRegistry::new();
    let registered = registered_fixture(&mut registry);
    runtime
        .install(registered, Arc::from(full_component()), install_record())
        .expect("installed");
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
    assert_eq!(plugins::close(Some(runtime), store), Ok(()));
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

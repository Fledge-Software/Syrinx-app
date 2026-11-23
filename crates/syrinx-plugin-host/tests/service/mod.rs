//! Contract tests for the `service` import (#280): a synthetic plugin reads from a synthetic
//! loopback server through the test host, and every refusal is named.

mod server;

use std::sync::Arc;

use syrinx_plugin_host::bindings::syrinx::plugin::types::PluginError;
use syrinx_plugin_host::limits;
use syrinx_plugin_host::manifest::CapabilityKind;
use syrinx_plugin_host::service::{Credential, OwnerTyping, ServiceAddress, ServiceDoor};
use syrinx_plugin_host::test_host::{
    Declarations, DeclaredCommand, LoopbackTransport, SyntheticGrants, TestHost,
};

use crate::support;
use server::SyntheticServer;

/// The commands the `service` variant answers; each reads `/<name>`.
const COMMANDS: [&str; 6] = [
    "variant.notes",
    "variant.other-host",
    "variant.redirect-away",
    "variant.redirect-home",
    "variant.big",
    "variant.loop",
];

fn declarations() -> Declarations {
    Declarations {
        plugin_id: "variant".into(),
        commands: COMMANDS
            .iter()
            .map(|id| DeclaredCommand {
                id: (*id).into(),
                form: false,
            })
            .collect(),
        ..Declarations::default()
    }
}

/// A credential made fresh for each run, so it can only be found where this run put it.
fn synthetic_credential() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!("synthetic-credential-{}-{nanos}", std::process::id())
}

struct Fixture {
    host: TestHost,
    server: SyntheticServer,
    typing: OwnerTyping,
    credential: String,
}

/// The `service` variant, granted `service`, its address the synthetic server's.
fn granted() -> Fixture {
    let grants = SyntheticGrants::new();
    grants.grant("variant", CapabilityKind::Service);
    let mut fixture = ungranted(grants);
    fixture
        .host
        .activate()
        .expect("activate runs")
        .expect("activate succeeds");
    fixture
}

fn ungranted(grants: SyntheticGrants) -> Fixture {
    let bytes = support::bytes(support::variant("service"));
    let mut host = TestHost::load_with_grants(&bytes, declarations(), grants)
        .expect("the service variant loads");
    let server = SyntheticServer::start();
    let typing = OwnerTyping::new();
    let credential = synthetic_credential();
    let address = ServiceAddress::parse(&server.address()).expect("a loopback address");
    let door = ServiceDoor::new(
        address,
        Some(Credential::new("x-api-key", credential.clone()).expect("valid")),
        Arc::new(LoopbackTransport),
        typing.clone(),
    );
    host.approve_service(door);
    Fixture {
        host,
        server,
        typing,
        credential,
    }
}

fn run(fixture: &mut Fixture, command: &str) -> Result<(), PluginError> {
    fixture.host.run_command(command).expect("the command runs")
}

/// The error's text, and that it is the expected kind.
fn refused(result: Result<(), PluginError>) -> String {
    format!("{:?}", result.expect_err("refused"))
}

fn logged(fixture: &Fixture) -> Vec<String> {
    fixture
        .host
        .logs()
        .iter()
        .map(|(_, message)| message.clone())
        .collect()
}

#[test]
fn the_plugin_reads_from_its_approved_address_with_the_host_s_credential() {
    let mut fixture = granted();
    assert_eq!(run(&mut fixture, "variant.notes"), Ok(()));
    assert!(
        logged(&fixture).contains(&format!("200 {}", server::NOTES)),
        "{:?}",
        logged(&fixture)
    );
    let requests = fixture.server.requests();
    let expected = format!("x-api-key: {}", fixture.credential);
    assert!(
        requests[0].starts_with("GET /notes HTTP/1.1\r\n"),
        "{requests:?}"
    );
    assert!(requests[0].contains(&expected), "{requests:?}");
}

#[test]
fn a_request_naming_another_host_is_refused_before_it_is_sent() {
    let mut fixture = granted();
    let error = refused(run(&mut fixture, "variant.other-host"));
    assert!(error.starts_with("Denied(\"other-host: "), "{error}");
    assert!(fixture.server.requests().is_empty());
}

#[test]
fn a_redirect_off_the_approved_host_is_refused_and_not_followed() {
    let mut fixture = granted();
    let error = refused(run(&mut fixture, "variant.redirect-away"));
    assert!(error.starts_with("Denied(\"redirect-off-host: "), "{error}");
    assert_eq!(fixture.server.requests().len(), 1);
}

#[test]
fn a_redirect_within_the_approved_host_is_followed() {
    let mut fixture = granted();
    assert_eq!(run(&mut fixture, "variant.redirect-home"), Ok(()));
    assert!(logged(&fixture).contains(&format!("200 {}", server::NOTES)));
    assert_eq!(fixture.server.requests().len(), 2);
}

#[test]
fn a_redirect_loop_is_ended() {
    let mut fixture = granted();
    let error = refused(run(&mut fixture, "variant.loop"));
    assert!(
        error.starts_with("Denied(\"too-many-redirects: "),
        "{error}"
    );
    assert_eq!(
        fixture.server.requests().len(),
        limits::MAX_SERVICE_REDIRECTS + 1
    );
}

#[test]
fn an_oversized_response_is_refused() {
    let mut fixture = granted();
    let error = refused(run(&mut fixture, "variant.big"));
    assert!(
        error.starts_with("Denied(\"response-too-large: "),
        "{error}"
    );
}

#[test]
fn no_request_starts_while_the_owner_types_in_the_palette() {
    let mut fixture = granted();
    fixture.typing.set(true);
    let error = refused(run(&mut fixture, "variant.notes"));
    assert!(error.starts_with("Unavailable(\"owner-typing: "), "{error}");
    assert!(fixture.server.requests().is_empty());
    fixture.typing.set(false);
    assert_eq!(run(&mut fixture, "variant.notes"), Ok(()));
}

#[test]
fn without_the_service_grant_nothing_is_sent() {
    let mut fixture = ungranted(SyntheticGrants::new());
    let error = refused(run(&mut fixture, "variant.notes"));
    assert!(
        error.contains("the service capability is not granted"),
        "{error}"
    );
    assert!(fixture.server.requests().is_empty());
}

#[test]
fn without_an_approved_address_the_grant_reaches_nothing() {
    let grants = SyntheticGrants::new();
    grants.grant("variant", CapabilityKind::Service);
    let bytes = support::bytes(support::variant("service"));
    let mut host =
        TestHost::load_with_grants(&bytes, declarations(), grants).expect("the variant loads");
    let error = format!(
        "{:?}",
        host.run_command("variant.notes")
            .expect("runs")
            .expect_err("refused")
    );
    assert!(error.starts_with("Denied(\"no-address: "), "{error}");
}

#[test]
fn the_credential_appears_in_no_log_and_no_error() {
    let mut fixture = granted();
    let mut texts = Vec::new();
    for command in COMMANDS {
        texts.push(format!("{:?}", run(&mut fixture, command)));
    }
    texts.extend(logged(&fixture));
    for text in &texts {
        assert!(!text.contains(&fixture.credential), "{text}");
    }
}

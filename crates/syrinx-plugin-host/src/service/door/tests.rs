//! The door against a scripted transport: what it sends, which redirects it follows, and
//! what it refuses before anything is sent.

use std::sync::Mutex;

use super::*;

/// A transport that answers from a script and records what it was asked.
#[derive(Default)]
struct Scripted {
    answers: Mutex<Vec<Result<IncomingResponse, TransportError>>>,
    sent: Mutex<Vec<(Method, String, Vec<Header>, Vec<u8>)>>,
}

impl Scripted {
    fn new(answers: Vec<Result<IncomingResponse, TransportError>>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().rev().collect()),
            sent: Mutex::default(),
        })
    }

    fn sent(&self) -> Vec<(Method, String, Vec<Header>, Vec<u8>)> {
        self.sent.lock().expect("unpoisoned").clone()
    }
}

impl Transport for Scripted {
    fn send(&self, request: &OutgoingRequest<'_>) -> Result<IncomingResponse, TransportError> {
        self.sent.lock().expect("unpoisoned").push((
            request.method,
            request.path.to_owned(),
            request.headers.to_vec(),
            request.body.to_vec(),
        ));
        self.answers
            .lock()
            .expect("unpoisoned")
            .pop()
            .expect("the script has an answer")
    }
}

const BUDGET: Duration = Duration::from_secs(1);

fn header(name: &str, value: &str) -> Header {
    Header {
        name: name.into(),
        value: value.into(),
    }
}

fn ok(status: u16, headers: Vec<Header>, body: &[u8]) -> Result<IncomingResponse, TransportError> {
    Ok(IncomingResponse {
        status,
        headers,
        body: body.to_vec(),
    })
}

fn redirect(status: u16, location: &str) -> Result<IncomingResponse, TransportError> {
    ok(status, vec![header("location", location)], b"")
}

fn door(transport: Arc<Scripted>) -> ServiceDoor {
    let address = ServiceAddress::parse("https://notes.example.test").expect("valid");
    let credential = Credential::new("x-api-key", "synthetic-credential").expect("valid");
    ServiceDoor::new(address, Some(credential), transport, OwnerTyping::new())
}

fn get(door: &ServiceDoor, path: &str) -> Result<Response, ServiceRefusal> {
    door.request(Method::Get, path, Vec::new(), b"", BUDGET)
}

#[test]
fn the_host_adds_the_credential_and_never_hands_it_back() {
    let transport = Scripted::new(vec![ok(
        200,
        vec![
            header("content-type", "application/json"),
            header("X-API-Key", "synthetic-credential"),
            header("set-cookie", "session=synthetic"),
        ],
        b"{}",
    )]);
    let door = door(transport.clone());
    let headers = vec![header("accept", "application/json")];
    let response = door
        .request(Method::Get, "/etapi/notes", headers, b"", BUDGET)
        .expect("answered");
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers,
        vec![header("content-type", "application/json")]
    );
    let sent = transport.sent();
    assert_eq!(
        sent[0].2,
        vec![
            header("accept", "application/json"),
            header("x-api-key", "synthetic-credential"),
        ]
    );
}

#[test]
fn nothing_is_sent_while_the_owner_types_or_for_another_host() {
    let transport = Scripted::new(Vec::new());
    let typing = OwnerTyping::new();
    let address = ServiceAddress::parse("https://notes.example.test").expect("valid");
    let door = ServiceDoor::new(address, None, transport.clone(), typing.clone());
    typing.set(true);
    assert_eq!(get(&door, "/x"), Err(ServiceRefusal::OwnerTyping));
    typing.set(false);
    assert_eq!(
        get(&door, "https://other.example.test/x"),
        Err(ServiceRefusal::OtherHost)
    );
    let large = vec![0; limits::MAX_SERVICE_REQUEST_BYTES + 1];
    assert_eq!(
        door.request(Method::Post, "/x", Vec::new(), &large, BUDGET),
        Err(ServiceRefusal::RequestTooLarge)
    );
    assert_eq!(
        door.request(Method::Get, "/x", Vec::new(), b"", Duration::ZERO),
        Err(ServiceRefusal::TimedOut)
    );
    assert!(transport.sent().is_empty());
}

#[test]
fn redirects_within_the_address_are_followed() {
    let transport = Scripted::new(vec![
        redirect(301, "/moved"),
        redirect(307, "https://NOTES.example.test:443/again?x=1"),
        ok(200, Vec::new(), b"done"),
    ]);
    let door = door(transport.clone());
    let response = door
        .request(Method::Put, "/start", Vec::new(), b"body", BUDGET)
        .expect("answered");
    assert_eq!(response.body, b"done");
    let paths: Vec<(Method, String, Vec<u8>)> = transport
        .sent()
        .into_iter()
        .map(|(method, path, _, body)| (method, path, body))
        .collect();
    assert_eq!(
        paths,
        vec![
            (Method::Put, "/start".to_owned(), b"body".to_vec()),
            (Method::Put, "/moved".to_owned(), b"body".to_vec()),
            (Method::Put, "/again?x=1".to_owned(), b"body".to_vec()),
        ]
    );
}

#[test]
fn a_303_becomes_a_bodiless_get() {
    let transport = Scripted::new(vec![redirect(303, "/result"), ok(200, Vec::new(), b"")]);
    let door = door(transport.clone());
    door.request(Method::Post, "/form", Vec::new(), b"data", BUDGET)
        .expect("answered");
    let second = &transport.sent()[1];
    assert_eq!((second.0, second.1.as_str()), (Method::Get, "/result"));
    assert!(second.3.is_empty());
}

#[test]
fn a_redirect_off_the_address_is_refused_before_it_is_followed() {
    for location in [
        "https://other.example.test/x",
        "//other.example.test/x",
        "http://notes.example.test/x",
        "https://notes.example.test:8443/x",
        "https://user@notes.example.test/x",
        "relative/x",
        "/a/../b",
    ] {
        let transport = Scripted::new(vec![redirect(302, location)]);
        let door = door(transport.clone());
        assert_eq!(
            get(&door, "/x"),
            Err(ServiceRefusal::RedirectOffHost),
            "{location}"
        );
        assert_eq!(transport.sent().len(), 1, "{location}");
    }
}

#[test]
fn a_redirect_loop_ends() {
    let answers = (0..=limits::MAX_SERVICE_REDIRECTS)
        .map(|_| redirect(302, "/loop"))
        .collect();
    let door = door(Scripted::new(answers));
    assert_eq!(get(&door, "/loop"), Err(ServiceRefusal::TooManyRedirects));
}

#[test]
fn an_oversized_response_is_refused_however_the_transport_reports_it() {
    let oversized = vec![0; limits::MAX_SERVICE_RESPONSE_BYTES + 1];
    let door = door(Scripted::new(vec![ok(200, Vec::new(), &oversized)]));
    assert_eq!(get(&door, "/big"), Err(ServiceRefusal::ResponseTooLarge));
    let door = self::door(Scripted::new(vec![Err(TransportError::ResponseTooLarge)]));
    assert_eq!(get(&door, "/big"), Err(ServiceRefusal::ResponseTooLarge));
}

#[test]
fn every_refusal_names_its_rule_and_never_the_credential() {
    let door = door(Scripted::new(vec![Err(TransportError::Unreachable)]));
    let refusal = get(&door, "/x").expect_err("unreachable");
    let error = crate::bindings::syrinx::plugin::types::PluginError::from(refusal);
    let text = format!("{error:?}");
    assert!(text.contains("unreachable: "), "{text}");
    assert!(!text.contains("synthetic-credential"), "{text}");
}

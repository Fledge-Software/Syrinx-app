//! The door against a scripted transport: what it sends, which redirects it follows, and
//! what it refuses before anything is sent.

use std::sync::Mutex;

use super::*;

/// One request as the scripted transport received it.
#[derive(Debug, Clone)]
struct Sent {
    method: Method,
    path: String,
    headers: Vec<Header>,
    body: Vec<u8>,
    timeout: Duration,
}

/// A transport that answers from a script and records what it was asked.
#[derive(Default)]
struct Scripted {
    answers: Mutex<Vec<Result<IncomingResponse, TransportError>>>,
    sent: Mutex<Vec<Sent>>,
    /// Set as the first request is sent, as if the owner started typing during it.
    typing_after_first: Option<OwnerTyping>,
    /// How long each send takes, so a test can see the time left shrink.
    delay: Duration,
}

impl Scripted {
    fn new(answers: Vec<Result<IncomingResponse, TransportError>>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().rev().collect()),
            sent: Mutex::default(),
            typing_after_first: None,
            delay: Duration::ZERO,
        })
    }

    fn slow(answers: Vec<Result<IncomingResponse, TransportError>>, delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().rev().collect()),
            sent: Mutex::default(),
            typing_after_first: None,
            delay,
        })
    }

    fn typing_after_first(
        answers: Vec<Result<IncomingResponse, TransportError>>,
        typing: OwnerTyping,
    ) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().rev().collect()),
            sent: Mutex::default(),
            typing_after_first: Some(typing),
            delay: Duration::ZERO,
        })
    }

    fn sent(&self) -> Vec<Sent> {
        self.sent.lock().expect("unpoisoned").clone()
    }
}

impl Transport for Scripted {
    fn send(&self, request: &OutgoingRequest<'_>) -> Result<IncomingResponse, TransportError> {
        self.sent.lock().expect("unpoisoned").push(Sent {
            method: request.method,
            path: request.path.to_owned(),
            headers: request.headers.to_vec(),
            body: request.body.to_vec(),
            timeout: request.timeout,
        });
        if let Some(typing) = &self.typing_after_first {
            typing.set(true);
        }
        std::thread::sleep(self.delay);
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
        sent[0].headers,
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
        Err(ServiceRefusal::NoTime)
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
        .map(|sent| (sent.method, sent.path, sent.body))
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
    assert_eq!(
        (second.method, second.path.as_str()),
        (Method::Get, "/result")
    );
    assert!(second.body.is_empty());
}

#[test]
fn a_redirect_off_the_address_is_refused_before_it_is_followed() {
    for location in [
        "https://other.example.test/x",
        "//other.example.test/x",
        "http://notes.example.test/x",
        "https://notes.example.test:8443/x",
        "https://user@notes.example.test/x",
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
fn a_redirect_within_the_address_to_a_refused_path_is_an_invalid_redirect() {
    for location in [
        "relative/x",
        "/a/../b",
        "/a#fragment",
        "https://notes.example.test/a b",
    ] {
        let transport = Scripted::new(vec![redirect(302, location)]);
        let door = door(transport.clone());
        assert_eq!(
            get(&door, "/x"),
            Err(ServiceRefusal::InvalidRedirect),
            "{location}"
        );
        assert_eq!(transport.sent().len(), 1, "{location}");
    }
}

#[test]
fn a_redirect_hop_is_not_sent_once_the_owner_types() {
    let typing = OwnerTyping::new();
    let transport = Scripted::typing_after_first(vec![redirect(302, "/next")], typing.clone());
    let address = ServiceAddress::parse("https://notes.example.test").expect("valid");
    let door = ServiceDoor::new(address, None, transport.clone(), typing);
    assert_eq!(get(&door, "/x"), Err(ServiceRefusal::OwnerTyping));
    assert_eq!(transport.sent().len(), 1);
}

#[test]
fn each_hop_gets_only_the_time_left_within_both_bounds() {
    let delay = Duration::from_millis(30);
    let transport = Scripted::slow(
        vec![redirect(302, "/next"), ok(200, Vec::new(), b"")],
        delay,
    );
    let door = door(transport.clone());
    let long_budget = limits::SERVICE_REQUEST_DEADLINE * 4;
    door.request(Method::Get, "/x", Vec::new(), b"", long_budget)
        .expect("answered");
    let sent = transport.sent();
    assert!(
        sent[0].timeout <= limits::SERVICE_REQUEST_DEADLINE,
        "{sent:?}"
    );
    assert!(sent[1].timeout + delay <= sent[0].timeout, "{sent:?}");
    let transport = Scripted::new(vec![ok(200, Vec::new(), b"")]);
    let door = self::door(transport.clone());
    door.request(Method::Get, "/x", Vec::new(), b"", BUDGET / 10)
        .expect("answered");
    assert!(transport.sent()[0].timeout <= BUDGET / 10);
}

#[test]
fn once_a_write_is_answered_every_later_failure_is_uncertain() {
    let post = |door: &ServiceDoor| door.request(Method::Post, "/x", Vec::new(), b"data", BUDGET);
    let cases = [
        (
            vec![redirect(303, "/result"), Err(TransportError::TimedOut)],
            ServiceRefusal::Uncertain("timed-out"),
        ),
        (
            vec![redirect(302, "https://other.example.test/")],
            ServiceRefusal::Uncertain("redirect-off-host"),
        ),
        (
            vec![redirect(307, "/a/../b")],
            ServiceRefusal::Uncertain("invalid-redirect"),
        ),
        (
            vec![redirect(307, "/next"), Err(TransportError::Unreachable)],
            ServiceRefusal::Uncertain("unreachable"),
        ),
        (
            vec![Err(TransportError::ResponseTooLarge)],
            ServiceRefusal::Uncertain("response-too-large"),
        ),
        (
            vec![ok(
                200,
                vec![header("x-a", "v"); limits::MAX_SERVICE_RESPONSE_HEADERS + 1],
                b"",
            )],
            ServiceRefusal::Uncertain("response-too-large"),
        ),
    ];
    for (answers, refusal) in cases {
        assert_eq!(
            post(&door(Scripted::new(answers))),
            Err(refusal.clone()),
            "{refusal}"
        );
    }
    let typing = OwnerTyping::new();
    let transport = Scripted::typing_after_first(vec![redirect(307, "/next")], typing.clone());
    let address = ServiceAddress::parse("https://notes.example.test").expect("valid");
    let door = ServiceDoor::new(address, None, transport, typing);
    assert_eq!(post(&door), Err(ServiceRefusal::Uncertain("owner-typing")));
}

#[test]
fn a_lost_answer_to_a_write_is_uncertain_and_to_a_read_may_be_asked_again() {
    let cases = [
        (
            Method::Get,
            TransportError::TimedOut,
            ServiceRefusal::TimedOut,
        ),
        (
            Method::Head,
            TransportError::BadResponse,
            ServiceRefusal::BadResponse,
        ),
        (
            Method::Post,
            TransportError::TimedOut,
            ServiceRefusal::Uncertain("timed-out"),
        ),
        (
            Method::Delete,
            TransportError::BadResponse,
            ServiceRefusal::Uncertain("bad-response"),
        ),
        (
            Method::Put,
            TransportError::Unreachable,
            ServiceRefusal::Unreachable,
        ),
    ];
    for (method, error, refusal) in cases {
        let door = door(Scripted::new(vec![Err(error)]));
        assert_eq!(
            door.request(method, "/x", Vec::new(), b"", BUDGET),
            Err(refusal.clone()),
            "{method:?} {error:?}"
        );
        let retry_safe = matches!(
            crate::bindings::syrinx::plugin::types::PluginError::from(refusal.clone()),
            crate::bindings::syrinx::plugin::types::PluginError::Unavailable(_)
        );
        assert_eq!(
            retry_safe,
            !matches!(refusal, ServiceRefusal::Uncertain(_)),
            "{refusal}"
        );
    }
}

#[test]
fn too_many_response_headers_are_refused() {
    let headers = vec![header("x-a", "v"); limits::MAX_SERVICE_RESPONSE_HEADERS + 1];
    let door = door(Scripted::new(vec![ok(200, headers, b"")]));
    assert_eq!(get(&door, "/x"), Err(ServiceRefusal::ResponseTooLarge));
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
    let refused = [
        (
            vec![Err(TransportError::Unreachable)],
            "/x",
            "unreachable: ",
        ),
        (
            vec![redirect(302, "https://other.example.test/")],
            "/x",
            "redirect-off-host: ",
        ),
        (vec![redirect(302, "/x"); 4], "/x", "too-many-redirects: "),
        (Vec::new(), "//other.example.test/", "other-host: "),
    ];
    for (answers, path, name) in refused {
        let door = door(Scripted::new(answers));
        let refusal = get(&door, path).expect_err(name);
        let text = format!(
            "{:?}",
            crate::bindings::syrinx::plugin::types::PluginError::from(refusal)
        );
        assert!(text.contains(name), "{text}");
        assert!(!text.contains("synthetic-credential"), "{text}");
    }
    let reserved = door(Scripted::new(Vec::new())).request(
        Method::Get,
        "/x",
        vec![header("x-api-key", "synthetic-credential")],
        b"",
        BUDGET,
    );
    let text = format!("{:?}", reserved.expect_err("reserved"));
    assert!(
        text.contains("x-api-key") && !text.contains("synthetic-credential"),
        "{text}"
    );
}

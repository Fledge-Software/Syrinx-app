//! The bridge's checks, their order, and what happens to pending requests.

use std::cell::Cell;

use serde_json::{Value, json};

use super::{EnvelopeError, Inbound, Refusal, RelayError, Relayed, ViewBridge, ViewOrigin};
use crate::bindings::syrinx::plugin::types::PluginError;
use crate::limits::{MAX_PENDING_VIEW_REQUESTS, MAX_VIEW_MESSAGE_BYTES};
use crate::manifest::TabType;

const SOURCE: &str = "https://syrinx-plugin.hello.invalid/index.html";
const GENERATION: u64 = 4;

fn bridge() -> ViewBridge {
    let tab_type = TabType {
        id: "hello.greeting".into(),
        title: "Greeting".into(),
        ui_methods: vec!["greet".into(), "wave".into()],
    };
    ViewBridge::new(ViewOrigin::plugin("hello"), &tab_type, GENERATION)
}

fn request(id: u64, method: &str) -> String {
    json!({ "v": 1, "kind": "request", "id": id, "method": method, "body": { "n": id } })
        .to_string()
}

fn cancel(id: u64) -> String {
    json!({ "v": 1, "kind": "cancel", "id": id }).to_string()
}

fn inbound(json: &str) -> Inbound<'_> {
    Inbound {
        source: SOURCE,
        generation: GENERATION,
        json,
    }
}

/// A `deliver` that counts its calls and hands out host request ids from 100.
struct Plugin {
    calls: Cell<u64>,
}

impl Plugin {
    fn new() -> Self {
        Self {
            calls: Cell::new(0),
        }
    }

    fn deliver(&self) -> impl FnOnce(&str, &str) -> Result<u64, String> + '_ {
        move |_method, _body| {
            self.calls.set(self.calls.get() + 1);
            Ok(100 + self.calls.get())
        }
    }
}

/// Relay `inbound` and return its refusal, checking `deliver` was never called.
fn refused(bridge: &mut ViewBridge, inbound: Inbound<'_>) -> Refusal {
    let plugin = Plugin::new();
    let result = bridge.relay(inbound, plugin.deliver());
    assert_eq!(plugin.calls.get(), 0, "nothing reached the plugin");
    match result {
        Err(RelayError::Refused(refusal)) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Relay a request that must be queued, returning its host request id.
fn queued(bridge: &mut ViewBridge, id: u64, plugin: &Plugin) -> u64 {
    match bridge.relay(inbound(&request(id, "greet")), plugin.deliver()) {
        Ok(Relayed::Queued { host_request }) => host_request,
        other => panic!("expected the request to be queued, got {other:?}"),
    }
}

#[test]
fn a_valid_request_reaches_the_plugin_once() {
    let mut bridge = bridge();
    let mut seen = Vec::new();
    let result = bridge.relay(inbound(&request(42, "greet")), |method, body| {
        seen.push((method.to_string(), body.to_string()));
        Ok::<u64, String>(9)
    });
    assert_eq!(result, Ok(Relayed::Queued { host_request: 9 }));
    assert_eq!(seen, vec![("greet".to_string(), r#"{"n":42}"#.to_string())]);
}

#[test]
fn a_message_from_another_origin_is_refused() {
    let mut bridge = bridge();
    let message = request(1, "greet");
    let from_elsewhere = Inbound {
        source: "https://syrinx-plugin.hello.invalid.evil.test/",
        ..inbound(&message)
    };
    assert_eq!(refused(&mut bridge, from_elsewhere), Refusal::WrongOrigin);
}

#[test]
fn a_message_from_an_old_generation_is_refused() {
    let mut bridge = bridge();
    let message = request(1, "greet");
    let old = Inbound {
        generation: GENERATION - 1,
        ..inbound(&message)
    };
    assert_eq!(refused(&mut bridge, old), Refusal::StaleGeneration);
}

#[test]
fn a_message_over_the_size_limit_is_refused() {
    let mut bridge = bridge();
    let padding = "x".repeat(MAX_VIEW_MESSAGE_BYTES);
    let message = json!({ "v": 1, "kind": "request", "id": 1, "method": "greet", "body": padding })
        .to_string();
    assert_eq!(refused(&mut bridge, inbound(&message)), Refusal::TooLarge);
}

#[test]
fn size_is_checked_before_the_text_is_parsed() {
    let mut bridge = bridge();
    let oversized_garbage = "x".repeat(MAX_VIEW_MESSAGE_BYTES + 1);
    assert_eq!(
        refused(&mut bridge, inbound(&oversized_garbage)),
        Refusal::TooLarge
    );
}

#[test]
fn a_stale_oversized_message_reports_the_generation() {
    let mut bridge = bridge();
    let oversized = "x".repeat(MAX_VIEW_MESSAGE_BYTES + 1);
    let stale = Inbound {
        generation: GENERATION - 1,
        ..inbound(&oversized)
    };
    assert_eq!(refused(&mut bridge, stale), Refusal::StaleGeneration);
}

#[test]
fn a_message_of_exactly_the_limit_is_queued() {
    let mut bridge = bridge();
    let envelope = |padding: &str| {
        json!({ "v": 1, "kind": "request", "id": 1, "method": "greet", "body": padding })
            .to_string()
    };
    let overhead = envelope("").len();
    let message = envelope(&"x".repeat(MAX_VIEW_MESSAGE_BYTES - overhead));
    assert_eq!(message.len(), MAX_VIEW_MESSAGE_BYTES);
    let plugin = Plugin::new();
    assert!(matches!(
        bridge.relay(inbound(&message), plugin.deliver()),
        Ok(Relayed::Queued { .. })
    ));
}

#[test]
fn a_malformed_envelope_is_refused() {
    let mut bridge = bridge();
    assert_eq!(
        refused(&mut bridge, inbound("not json")),
        Refusal::Malformed(EnvelopeError::NotJson)
    );
}

#[test]
fn an_undeclared_method_is_refused() {
    let mut bridge = bridge();
    assert_eq!(
        refused(&mut bridge, inbound(&request(1, "delete-everything"))),
        Refusal::UndeclaredMethod("delete-everything".into())
    );
}

#[test]
fn an_id_already_pending_is_refused() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    queued(&mut bridge, 1, &plugin);
    assert_eq!(
        refused(&mut bridge, inbound(&request(1, "wave"))),
        Refusal::DuplicateId(1)
    );
}

#[test]
fn a_full_queue_is_refused() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    for id in 0..MAX_PENDING_VIEW_REQUESTS as u64 {
        queued(&mut bridge, id, &plugin);
    }
    assert_eq!(
        refused(&mut bridge, inbound(&request(1_000, "greet"))),
        Refusal::QueueFull
    );
}

#[test]
fn the_earlier_check_is_reported() {
    let mut bridge = bridge();
    let undeclared = request(1, "delete-everything");
    let elsewhere_and_old = Inbound {
        source: "https://example.com/",
        generation: 0,
        json: "not json",
    };
    let old_and_malformed = Inbound {
        generation: 0,
        ..inbound("not json")
    };
    let large = format!("{}{}", " ".repeat(MAX_VIEW_MESSAGE_BYTES), undeclared);
    assert_eq!(
        refused(&mut bridge, elsewhere_and_old),
        Refusal::WrongOrigin
    );
    assert_eq!(
        refused(&mut bridge, old_and_malformed),
        Refusal::StaleGeneration
    );
    assert_eq!(refused(&mut bridge, inbound(&large)), Refusal::TooLarge);
    let old_version_and_undeclared =
        json!({ "v": 2, "kind": "request", "id": 1, "method": "delete-everything", "body": {} })
            .to_string();
    assert_eq!(
        refused(&mut bridge, inbound(&old_version_and_undeclared)),
        Refusal::Malformed(EnvelopeError::Version)
    );
}

#[test]
fn an_undeclared_duplicate_reports_the_method() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    queued(&mut bridge, 1, &plugin);
    assert_eq!(
        refused(&mut bridge, inbound(&request(1, "delete-everything"))),
        Refusal::UndeclaredMethod("delete-everything".into())
    );
}

#[test]
fn a_duplicate_in_a_full_queue_reports_the_duplicate() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    for id in 0..MAX_PENDING_VIEW_REQUESTS as u64 {
        queued(&mut bridge, id, &plugin);
    }
    assert_eq!(
        refused(&mut bridge, inbound(&request(0, "greet"))),
        Refusal::DuplicateId(0)
    );
}

#[test]
fn a_failed_delivery_leaves_nothing_pending() {
    let mut bridge = bridge();
    let result = bridge.relay(inbound(&request(1, "greet")), |_, _| {
        Err::<u64, _>("the worker is stopping")
    });
    assert_eq!(result, Err(RelayError::Delivery("the worker is stopping")));
    let plugin = Plugin::new();
    queued(&mut bridge, 1, &plugin);
}

#[test]
fn a_cancel_returns_the_pending_request_and_never_delivers() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    let host_request = queued(&mut bridge, 7, &plugin);
    let result = bridge.relay(inbound(&cancel(7)), plugin.deliver());
    assert_eq!(result, Ok(Relayed::Cancelled { host_request }));
    assert_eq!(plugin.calls.get(), 1, "only the request was delivered");
    assert_eq!(
        bridge.reply(GENERATION, host_request, Ok("{}".into())),
        None
    );
}

#[test]
fn a_cancel_for_an_unknown_id_is_ignored() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    let result = bridge.relay(inbound(&cancel(7)), plugin.deliver());
    assert_eq!(result, Ok(Relayed::CancelIgnored));
    assert_eq!(plugin.calls.get(), 0);
}

#[test]
fn a_stale_cancel_leaves_its_request_pending() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    let host_request = queued(&mut bridge, 7, &plugin);
    let message = cancel(7);
    let stale = Inbound {
        generation: GENERATION - 1,
        ..inbound(&message)
    };
    assert_eq!(refused(&mut bridge, stale), Refusal::StaleGeneration);
    assert!(
        bridge
            .reply(GENERATION, host_request, Ok("{}".into()))
            .is_some(),
        "the request is still pending"
    );
}

#[test]
fn a_cancel_passes_the_first_checks() {
    let mut bridge = bridge();
    let message = cancel(7);
    let elsewhere = Inbound {
        source: "https://example.com/",
        ..inbound(&message)
    };
    assert_eq!(refused(&mut bridge, elsewhere), Refusal::WrongOrigin);
}

#[test]
fn a_reply_answers_the_interface_id() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    let host_request = queued(&mut bridge, 42, &plugin);
    let written = bridge
        .reply(GENERATION, host_request, Ok(r#"{"greeting":"hi"}"#.into()))
        .expect("the reply is sent");
    let expected =
        json!({ "v": 1, "kind": "response", "id": 42, "ok": true, "body": { "greeting": "hi" } });
    assert_eq!(serde_json::from_str::<Value>(&written).unwrap(), expected);
}

#[test]
fn an_error_reply_is_sent_as_not_ok() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    let host_request = queued(&mut bridge, 42, &plugin);
    let error = PluginError::Denied("files".into());
    let written = bridge
        .reply(GENERATION, host_request, Err(error))
        .expect("the reply is sent");
    let value: Value = serde_json::from_str(&written).unwrap();
    assert_eq!(value["ok"], json!(false));
    assert_eq!(value["body"]["kind"], json!("denied"));
}

#[test]
fn a_reply_for_an_old_generation_or_unknown_request_is_dropped() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    let host_request = queued(&mut bridge, 42, &plugin);
    assert_eq!(
        bridge.reply(GENERATION - 1, host_request, Ok("{}".into())),
        None
    );
    assert_eq!(bridge.reply(GENERATION, 999, Ok("{}".into())), None);
    assert!(
        bridge
            .reply(GENERATION, host_request, Ok("{}".into()))
            .is_some()
    );
    assert_eq!(
        bridge.reply(GENERATION, host_request, Ok("{}".into())),
        None,
        "a request is answered once"
    );
}

#[test]
fn an_answered_id_may_be_used_again() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    let host_request = queued(&mut bridge, 42, &plugin);
    assert!(
        bridge
            .reply(GENERATION, host_request, Ok("{}".into()))
            .is_some()
    );
    queued(&mut bridge, 42, &plugin);
}

#[test]
fn renewal_cancels_pending_requests_and_refuses_the_old_generation() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    let first = queued(&mut bridge, 1, &plugin);
    let second = queued(&mut bridge, 2, &plugin);
    assert_eq!(bridge.renew(), vec![first, second]);
    assert_eq!(bridge.generation(), GENERATION + 1);
    assert_eq!(bridge.reply(GENERATION, first, Ok("{}".into())), None);
    assert_eq!(bridge.reply(GENERATION + 1, first, Ok("{}".into())), None);
    assert_eq!(
        refused(&mut bridge, inbound(&request(3, "greet"))),
        Refusal::StaleGeneration
    );
    let message = request(1, "greet");
    let current = Inbound {
        generation: GENERATION + 1,
        ..inbound(&message)
    };
    assert!(matches!(
        bridge.relay(current, plugin.deliver()),
        Ok(Relayed::Queued { .. })
    ));
}

#[test]
fn closing_returns_the_pending_requests() {
    let mut bridge = bridge();
    let plugin = Plugin::new();
    let host_request = queued(&mut bridge, 1, &plugin);
    assert_eq!(bridge.close(), vec![host_request]);
}

//! The envelope's reading rules and the responses written back.

use serde_json::{Value, json};

use super::{Envelope, EnvelopeError, parse, response};
use crate::bindings::syrinx::plugin::types::PluginError;

fn refusal(message: Value) -> EnvelopeError {
    parse(&message.to_string()).expect_err("the envelope is refused")
}

#[test]
fn a_request_is_read_with_its_body_compacted() {
    let message = r#"{ "v": 1, "kind": "request", "id": 42, "method": "greet",
                       "body": { "name" : "Ada" } }"#;
    let expected = Envelope::Request {
        id: 42,
        method: "greet".into(),
        body: r#"{"name":"Ada"}"#.into(),
    };
    assert_eq!(parse(message), Ok(expected));
}

#[test]
fn any_json_value_is_a_body() {
    let message = json!({ "v": 1, "kind": "request", "id": 0, "method": "greet", "body": null });
    let Ok(Envelope::Request { body, .. }) = parse(&message.to_string()) else {
        panic!("the request is read");
    };
    assert_eq!(body, "null");
}

#[test]
fn a_cancel_is_read() {
    let message = json!({ "v": 1, "kind": "cancel", "id": 7 });
    assert_eq!(parse(&message.to_string()), Ok(Envelope::Cancel { id: 7 }));
}

#[test]
fn text_that_is_not_a_json_object_is_refused() {
    assert_eq!(parse("{"), Err(EnvelopeError::NotJson));
    assert_eq!(refusal(json!([1, 2])), EnvelopeError::NotAnObject);
}

#[test]
fn an_unknown_kind_is_refused() {
    for kind in [json!("response"), json!("event"), json!(1)] {
        let message = json!({ "v": 1, "kind": kind, "id": 1 });
        assert_eq!(refusal(message), EnvelopeError::Kind);
    }
    assert_eq!(refusal(json!({ "v": 1, "id": 1 })), EnvelopeError::Kind);
}

#[test]
fn an_unknown_field_is_refused() {
    let message =
        json!({ "v": 1, "kind": "request", "id": 1, "method": "greet", "body": {}, "extra": 1 });
    assert_eq!(
        refusal(message),
        EnvelopeError::UnknownField("extra".into())
    );
}

#[test]
fn a_cancel_carries_no_method_or_body() {
    let with_method = json!({ "v": 1, "kind": "cancel", "id": 1, "method": "greet" });
    let with_body = json!({ "v": 1, "kind": "cancel", "id": 1, "body": {} });
    assert_eq!(
        refusal(with_method),
        EnvelopeError::UnknownField("method".into())
    );
    assert_eq!(
        refusal(with_body),
        EnvelopeError::UnknownField("body".into())
    );
}

#[test]
fn only_version_one_is_read() {
    for version in [json!(2), json!(1.0), json!("1"), json!(0)] {
        let message = json!({ "v": version, "kind": "cancel", "id": 1 });
        assert_eq!(refusal(message), EnvelopeError::Version);
    }
    assert_eq!(
        refusal(json!({ "kind": "cancel", "id": 1 })),
        EnvelopeError::Version
    );
}

#[test]
fn an_id_must_be_an_exact_javascript_integer() {
    let largest = (1_u64 << 53) - 1;
    let message = json!({ "v": 1, "kind": "cancel", "id": largest });
    assert_eq!(
        parse(&message.to_string()),
        Ok(Envelope::Cancel { id: largest })
    );
    for id in [
        json!(largest + 1),
        json!(-1),
        json!(1.5),
        json!("1"),
        json!(null),
    ] {
        let message = json!({ "v": 1, "kind": "cancel", "id": id });
        assert_eq!(refusal(message), EnvelopeError::Id);
    }
    assert_eq!(
        refusal(json!({ "v": 1, "kind": "cancel" })),
        EnvelopeError::Id
    );
}

#[test]
fn a_request_needs_a_string_method_and_a_body() {
    let no_method = json!({ "v": 1, "kind": "request", "id": 1, "body": {} });
    let number_method = json!({ "v": 1, "kind": "request", "id": 1, "method": 3, "body": {} });
    let no_body = json!({ "v": 1, "kind": "request", "id": 1, "method": "greet" });
    assert_eq!(refusal(no_method), EnvelopeError::Method);
    assert_eq!(refusal(number_method), EnvelopeError::Method);
    assert_eq!(refusal(no_body), EnvelopeError::Body);
}

#[test]
fn a_reply_becomes_a_response() {
    let written = response(42, Ok(r#"{"greeting":"hello"}"#.into()));
    let expected = json!({
        "v": 1, "kind": "response", "id": 42, "ok": true, "body": { "greeting": "hello" },
    });
    assert_eq!(serde_json::from_str::<Value>(&written).unwrap(), expected);
}

#[test]
fn an_error_names_its_kind_and_message() {
    let written = response(3, Err(PluginError::NotFound("no note n-1".into())));
    let expected = json!({
        "v": 1, "kind": "response", "id": 3, "ok": false,
        "body": { "kind": "not-found", "message": "no note n-1" },
    });
    assert_eq!(serde_json::from_str::<Value>(&written).unwrap(), expected);
}

#[test]
fn a_reply_that_is_not_json_answers_with_an_internal_error() {
    let written = response(5, Ok("not json".into()));
    let value: Value = serde_json::from_str(&written).unwrap();
    assert_eq!(value["ok"], json!(false));
    assert_eq!(value["body"]["kind"], json!("internal"));
}

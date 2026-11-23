//! The bridge's JSON envelope (research note 10, "The bridge"): reading what a view sends and
//! writing the responses the host sends back.

use serde_json::{Map, Value, json};

use crate::bindings::syrinx::plugin::types::PluginError;

/// The only bridge version this host speaks.
const VERSION: u64 = 1;
/// The largest integer JavaScript represents exactly, so an interface's ids round-trip.
const MAX_ID: u64 = (1 << 53) - 1;

/// What a view's message asks for, once its envelope is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Envelope {
    /// `body` is the request's JSON value, re-serialized compactly.
    Request {
        id: u64,
        method: String,
        body: String,
    },
    Cancel {
        id: u64,
    },
}

/// Why a message is not a valid envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    NotJson,
    NotAnObject,
    /// `kind` is missing or is neither `"request"` nor `"cancel"`.
    Kind,
    /// A field this kind does not carry, such as a `method` on a cancel.
    UnknownField(String),
    /// `v` is missing or is not 1.
    Version,
    /// `id` is missing or is not an integer from 0 to 2^53 − 1.
    Id,
    /// A request's `method` is missing or is not a string.
    Method,
    /// A request carries no `body`.
    Body,
}

/// Read a message's envelope: its kind, then its fields, version, id and payload.
pub fn parse(json: &str) -> Result<Envelope, EnvelopeError> {
    let value: Value = serde_json::from_str(json).map_err(|_| EnvelopeError::NotJson)?;
    let Value::Object(mut fields) = value else {
        return Err(EnvelopeError::NotAnObject);
    };
    let kind = Kind::of(&fields)?;
    only_fields(&fields, kind.fields())?;
    version(&fields)?;
    let id = id(&fields)?;
    if kind == Kind::Cancel {
        return Ok(Envelope::Cancel { id });
    }
    let method = method(&fields)?;
    let body = fields.remove("body").ok_or(EnvelopeError::Body)?;
    Ok(Envelope::Request {
        id,
        method,
        body: body.to_string(),
    })
}

/// The response to request `id`: the plugin's reply, or the error it returned.
pub fn response(id: u64, result: Result<String, PluginError>) -> String {
    // Defensive: the session already checked the reply with `validate::json`.
    let outcome = result.and_then(|reply| {
        serde_json::from_str::<Value>(&reply)
            .map_err(|_| PluginError::Internal("the plugin's reply is not JSON".into()))
    });
    let envelope = match outcome {
        Ok(body) => json!({ "v": VERSION, "kind": "response", "id": id, "ok": true, "body": body }),
        Err(error) => json!({
            "v": VERSION, "kind": "response", "id": id, "ok": false, "body": error_body(&error),
        }),
    };
    envelope.to_string()
}

/// The two kinds a view may send, each with the fields it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Request,
    Cancel,
}

impl Kind {
    fn of(fields: &Map<String, Value>) -> Result<Self, EnvelopeError> {
        match fields.get("kind").and_then(Value::as_str) {
            Some("request") => Ok(Kind::Request),
            Some("cancel") => Ok(Kind::Cancel),
            _ => Err(EnvelopeError::Kind),
        }
    }

    fn fields(self) -> &'static [&'static str] {
        match self {
            Kind::Request => &["v", "kind", "id", "method", "body"],
            Kind::Cancel => &["v", "kind", "id"],
        }
    }
}

fn only_fields(fields: &Map<String, Value>, allowed: &[&str]) -> Result<(), EnvelopeError> {
    match fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(unknown) => Err(EnvelopeError::UnknownField(unknown.clone())),
        None => Ok(()),
    }
}

fn version(fields: &Map<String, Value>) -> Result<(), EnvelopeError> {
    match fields.get("v").and_then(Value::as_u64) {
        Some(VERSION) => Ok(()),
        _ => Err(EnvelopeError::Version),
    }
}

fn id(fields: &Map<String, Value>) -> Result<u64, EnvelopeError> {
    match fields.get("id").and_then(Value::as_u64) {
        Some(id) if id <= MAX_ID => Ok(id),
        _ => Err(EnvelopeError::Id),
    }
}

fn method(fields: &Map<String, Value>) -> Result<String, EnvelopeError> {
    match fields.get("method").and_then(Value::as_str) {
        Some(method) => Ok(method.into()),
        None => Err(EnvelopeError::Method),
    }
}

/// An error's kind, by its WIT name, and its message.
fn error_body(error: &PluginError) -> Value {
    let (kind, message) = match error {
        PluginError::InvalidInput(message) => ("invalid-input", message),
        PluginError::NotFound(message) => ("not-found", message),
        PluginError::Denied(message) => ("denied", message),
        PluginError::Revoked(message) => ("revoked", message),
        PluginError::Unavailable(message) => ("unavailable", message),
        PluginError::Internal(message) => ("internal", message),
        PluginError::Cancelled(message) => ("cancelled", message),
    };
    json!({ "kind": kind, "message": message })
}

#[cfg(test)]
#[path = "envelope_tests.rs"]
mod tests;

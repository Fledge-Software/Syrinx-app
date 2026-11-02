//! Bounds on every value lifted from a plugin. A value over a limit becomes a
//! typed `plugin-error.invalid-input`, never a trap.

use syrinx_core::CommandId;

use crate::bindings::exports::syrinx::plugin::forms::{FieldError, FieldKind, FormSpec};
use crate::bindings::exports::syrinx::plugin::search_feed::{Batch, Change};
use crate::bindings::syrinx::plugin::types::PluginError;
use crate::limits;

fn invalid(message: impl Into<String>) -> PluginError {
    PluginError::InvalidInput(message.into())
}

/// A dotted lowercase id, by the same rule as Syrinx's own command ids.
pub fn id(what: &str, value: &str) -> Result<(), PluginError> {
    CommandId::new(value)
        .map(|_| ())
        .map_err(|error| invalid(format!("{what} {value:?}: {error}")))
}

/// A command id must also start with its plugin's id.
pub fn command_id(plugin: &str, value: &str) -> Result<(), PluginError> {
    id("command", value)?;
    let prefix = format!("{plugin}.");
    if value.starts_with(&prefix) {
        return Ok(());
    }
    Err(invalid(format!(
        "command {value:?} does not start with {prefix:?}"
    )))
}

pub fn chars(what: &str, value: &str, max: usize) -> Result<(), PluginError> {
    if value.chars().count() <= max {
        return Ok(());
    }
    Err(invalid(format!("{what} is longer than {max} characters")))
}

pub fn bytes(what: &str, value: &str, max: usize) -> Result<(), PluginError> {
    if value.len() <= max {
        return Ok(());
    }
    Err(invalid(format!("{what} is larger than {max} bytes")))
}

/// JSON for a view: bounded and well formed.
pub fn json(what: &str, value: &str) -> Result<(), PluginError> {
    bytes(what, value, limits::MAX_JSON_BYTES)?;
    serde_json::from_str::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|_| invalid(format!("{what} is not JSON")))
}

/// A form the palette will draw.
pub fn form_spec(spec: &FormSpec) -> Result<(), PluginError> {
    chars("form title", &spec.title, limits::MAX_TITLE_CHARS)?;
    chars("submit label", &spec.submit_label, limits::MAX_LABEL_CHARS)?;
    if spec.fields.is_empty() || spec.fields.len() > limits::MAX_FORM_FIELDS {
        return Err(invalid(format!(
            "a form has 1 to {} fields",
            limits::MAX_FORM_FIELDS
        )));
    }
    for field in &spec.fields {
        id("field name", &field.name)?;
        chars("field label", &field.label, limits::MAX_LABEL_CHARS)?;
        if let Some(initial) = &field.initial {
            bytes("initial value", initial, limits::MAX_FIELD_TEXT_BYTES)?;
        }
        field_kind(&field.kind)?;
    }
    Ok(())
}

fn field_kind(kind: &FieldKind) -> Result<(), PluginError> {
    match kind {
        FieldKind::Text(options) | FieldKind::Multiline(options) => {
            if let Some(placeholder) = &options.placeholder {
                chars("placeholder", placeholder, limits::MAX_LABEL_CHARS)?;
            }
            Ok(())
        }
        FieldKind::Money(currency) => chars("currency", currency, 8),
        FieldKind::Choice(options) => {
            if options.is_empty() || options.len() > limits::MAX_CHOICE_OPTIONS {
                return Err(invalid(format!(
                    "a choice has 1 to {} options",
                    limits::MAX_CHOICE_OPTIONS
                )));
            }
            for option in options {
                chars("choice value", &option.value, limits::MAX_LABEL_CHARS)?;
                chars("choice label", &option.label, limits::MAX_LABEL_CHARS)?;
            }
            Ok(())
        }
        FieldKind::Number(_) | FieldKind::Toggle | FieldKind::Date | FieldKind::DateTime => Ok(()),
    }
}

/// Field errors name fields and carry short messages.
pub fn field_errors(errors: &[FieldError]) -> Result<(), PluginError> {
    if errors.len() > limits::MAX_FORM_FIELDS {
        return Err(invalid("more field errors than a form has fields"));
    }
    for error in errors {
        id("field name", &error.name)?;
        chars("field message", &error.message, limits::MAX_MESSAGE_CHARS)?;
    }
    Ok(())
}

/// A search batch within what the host asked for. Over-limit keywords are
/// dropped, not refused (G9, item 7); the dropped ones are returned for the log.
pub fn batch(batch: &mut Batch, max: u32) -> Result<Vec<String>, PluginError> {
    if batch.changes.len() > max as usize {
        return Err(invalid(format!("the batch has more than {max} changes")));
    }
    bytes("cursor", &batch.next_cursor, limits::MAX_CURSOR_BYTES)?;
    let mut dropped = Vec::new();
    for change in &mut batch.changes {
        dropped.extend(change_fits(change)?);
    }
    Ok(dropped)
}

fn change_fits(change: &mut Change) -> Result<Vec<String>, PluginError> {
    match change {
        Change::UpsertEntry(entry) => {
            bytes("entry id", &entry.id, limits::MAX_ENTRY_ID_BYTES)?;
            chars("entry title", &entry.title, limits::MAX_TITLE_CHARS)?;
            chars("entry subtitle", &entry.subtitle, limits::MAX_TITLE_CHARS)?;
            Ok(keep_keywords(&mut entry.keywords))
        }
        Change::UpsertDocument(document) => {
            bytes("document id", &document.id, limits::MAX_ENTRY_ID_BYTES)?;
            chars("document title", &document.title, limits::MAX_TITLE_CHARS)?;
            bytes(
                "document body",
                &document.body,
                limits::MAX_DOCUMENT_BODY_BYTES,
            )?;
            Ok(Vec::new())
        }
        Change::Delete(id) => {
            bytes("deleted id", id, limits::MAX_ENTRY_ID_BYTES).map(|()| Vec::new())
        }
        Change::Reset => Ok(Vec::new()),
    }
}

/// Keep the first keywords that fit; return a description of each one dropped.
fn keep_keywords(keywords: &mut Vec<String>) -> Vec<String> {
    let mut dropped = Vec::new();
    let mut kept = Vec::new();
    for keyword in keywords.drain(..) {
        if keyword.chars().count() > limits::MAX_KEYWORD_CHARS {
            dropped.push(format!(
                "a keyword over {} characters",
                limits::MAX_KEYWORD_CHARS
            ));
        } else if kept.len() == limits::MAX_KEYWORDS {
            dropped.push(format!("a keyword past the first {}", limits::MAX_KEYWORDS));
        } else {
            kept.push(keyword);
        }
    }
    *keywords = kept;
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_follow_the_command_rule_and_the_plugin_prefix() {
        assert!(command_id("test", "test.echo").is_ok());
        assert!(command_id("test", "other.echo").is_err());
        assert!(command_id("test", "test.Echo").is_err());
        assert!(id("view kind", "").is_err());
    }

    #[test]
    fn keywords_over_the_limits_are_dropped_not_refused() {
        let mut keywords: Vec<String> = (0..20).map(|index| format!("k{index}")).collect();
        keywords.insert(0, "x".repeat(limits::MAX_KEYWORD_CHARS + 1));
        let dropped = keep_keywords(&mut keywords);
        assert_eq!(keywords.len(), limits::MAX_KEYWORDS);
        assert_eq!(keywords[0], "k0");
        assert_eq!(dropped.len(), 5);
    }

    #[test]
    fn json_must_parse() {
        assert!(json("reply", "{\"ok\":true}").is_ok());
        assert!(json("reply", "{nope").is_err());
    }
}

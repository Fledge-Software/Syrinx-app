//! Bounds on every value lifted from a plugin. A value over a limit becomes a
//! typed `plugin-error.invalid-input`, never a trap.

use std::collections::BTreeSet;

use syrinx_core::CommandId;

use crate::bindings::exports::syrinx::plugin::forms::{FieldError, FieldKind, FormSpec};
use crate::bindings::exports::syrinx::plugin::search_feed::{Batch, Change};
use crate::bindings::syrinx::plugin::types::{Outcome, PluginError};
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

/// A settings key: a bounded dotted id.
pub fn settings_key(key: &str) -> Result<(), PluginError> {
    bytes("settings key", key, limits::MAX_SETTINGS_KEY_BYTES)?;
    id("settings key", key)
}

/// A settings value: opaque bytes, bounded.
pub fn settings_value(value: &[u8]) -> Result<(), PluginError> {
    if value.len() <= limits::MAX_SETTINGS_VALUE_BYTES {
        return Ok(());
    }
    Err(invalid("the value is too large"))
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
    let mut names = BTreeSet::new();
    for field in &spec.fields {
        id("field name", &field.name)?;
        if !names.insert(field.name.as_str()) {
            return Err(invalid(format!("two fields are named {:?}", field.name)));
        }
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
            if options.max_length as usize > limits::MAX_FIELD_TEXT_BYTES {
                return Err(invalid(format!(
                    "a text field holds at most {} characters",
                    limits::MAX_FIELD_TEXT_BYTES
                )));
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
            let mut values = BTreeSet::new();
            for option in options {
                chars("choice value", &option.value, limits::MAX_LABEL_CHARS)?;
                if !values.insert(option.value.as_str()) {
                    return Err(invalid(format!(
                        "two options share the value {:?}",
                        option.value
                    )));
                }
                chars("choice label", &option.label, limits::MAX_LABEL_CHARS)?;
            }
            Ok(())
        }
        FieldKind::Number(_) | FieldKind::Toggle | FieldKind::Date | FieldKind::DateTime => Ok(()),
    }
}

/// A plugin's error with its text cut to the message limit, so a long one still arrives.
pub fn bound_error(error: PluginError) -> PluginError {
    match error {
        PluginError::NotFound(text) => PluginError::NotFound(cut(text)),
        PluginError::InvalidInput(text) => PluginError::InvalidInput(cut(text)),
        PluginError::Denied(text) => PluginError::Denied(cut(text)),
        PluginError::Unavailable(text) => PluginError::Unavailable(cut(text)),
        PluginError::Internal(text) => PluginError::Internal(cut(text)),
        PluginError::Revoked(text) => PluginError::Revoked(cut(text)),
        PluginError::Cancelled(text) => PluginError::Cancelled(cut(text)),
    }
}

/// An outcome with its text cut the same way.
pub fn bound_outcome(outcome: Outcome) -> Outcome {
    match outcome {
        Outcome::Done(detail) => Outcome::Done(detail.map(cut)),
        Outcome::Failed(error) => Outcome::Failed(bound_error(error)),
        Outcome::Uncertain(detail) => Outcome::Uncertain(cut(detail)),
    }
}

fn cut(text: String) -> String {
    if text.chars().count() <= limits::MAX_MESSAGE_CHARS {
        return text;
    }
    text.chars().take(limits::MAX_MESSAGE_CHARS).collect()
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

/// An entry or document id Syrinx's store and index both keep: not empty, bounded, and not
/// starting with the NUL the index reserves for its own markers.
fn feed_id(what: &str, id: &str) -> Result<(), PluginError> {
    if id.is_empty() || id.starts_with(limits::RESERVED_ID_START) {
        return Err(invalid(format!("{what} is empty or starts with NUL")));
    }
    bytes(what, id, limits::MAX_ENTRY_ID_BYTES)
}

/// The store ends each keyword with U+001F, so no keyword may hold it.
fn keyword_separators(keywords: &[String]) -> Result<(), PluginError> {
    if keywords
        .iter()
        .any(|keyword| keyword.contains(limits::KEYWORD_SEPARATOR))
    {
        return Err(invalid("a keyword holds U+001F"));
    }
    Ok(())
}

fn change_fits(change: &mut Change) -> Result<Vec<String>, PluginError> {
    match change {
        Change::UpsertEntry(entry) => {
            feed_id("entry id", &entry.id)?;
            chars("entry title", &entry.title, limits::MAX_TITLE_CHARS)?;
            chars("entry subtitle", &entry.subtitle, limits::MAX_TITLE_CHARS)?;
            keyword_separators(&entry.keywords)?;
            Ok(keep_keywords(&mut entry.keywords))
        }
        Change::UpsertDocument(document) => {
            feed_id("document id", &document.id)?;
            chars("document title", &document.title, limits::MAX_TITLE_CHARS)?;
            bytes(
                "document body",
                &document.body,
                limits::MAX_DOCUMENT_BODY_BYTES,
            )?;
            Ok(Vec::new())
        }
        Change::Delete(id) => feed_id("deleted id", id).map(|()| Vec::new()),
        Change::Reset => Ok(Vec::new()),
    }
}

/// The search tiers a feed's manifest declares (`catalogue`, `full-text`).
/// Only the application's runtime (`owner`) pulls feeds, so only it enforces them.
#[cfg(feature = "store")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FeedTiers {
    pub(crate) catalogue: bool,
    pub(crate) full_text: bool,
}

/// Drops each change for a tier the feed did not declare; returns a description of each,
/// for the log. A deletion and a reset reach whichever tiers the feed has.
#[cfg(feature = "store")]
pub(crate) fn keep_declared_tiers(batch: &mut Batch, tiers: FeedTiers) -> Vec<String> {
    let mut dropped = Vec::new();
    batch.changes.retain(|change| {
        let undeclared = match change {
            Change::UpsertEntry(_) => (!tiers.catalogue).then_some("a catalogue entry"),
            Change::UpsertDocument(_) => (!tiers.full_text).then_some("a full-text document"),
            Change::Delete(_) | Change::Reset => None,
        };
        let Some(what) = undeclared else {
            return true;
        };
        dropped.push(format!("{what} for a tier the feed does not declare"));
        false
    });
    dropped
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
    use crate::bindings::exports::syrinx::plugin::forms::{
        ChoiceOption, Field, TextFormat, TextOptions,
    };

    use crate::bindings::exports::syrinx::plugin::search_feed::{CatalogueEntry, Document};
    use crate::bindings::syrinx::plugin::types::Icon;

    fn entry(id: &str, keywords: &[&str]) -> Change {
        Change::UpsertEntry(CatalogueEntry {
            id: id.to_owned(),
            revision: 1,
            title: "Synthetic".to_owned(),
            subtitle: "made-up".to_owned(),
            icon: Icon::Document,
            last_used: None,
            keywords: keywords
                .iter()
                .map(|keyword| (*keyword).to_owned())
                .collect(),
        })
    }

    fn document(id: &str) -> Change {
        Change::UpsertDocument(Document {
            id: id.to_owned(),
            revision: 1,
            title: "Synthetic".to_owned(),
            body: "made-up".to_owned(),
        })
    }

    fn one(change: Change) -> Result<Vec<String>, PluginError> {
        let mut changes = Batch {
            changes: vec![change],
            next_cursor: "1".to_owned(),
            more: false,
        };
        batch(&mut changes, 8)
    }

    #[test]
    fn a_batch_the_store_or_index_would_refuse_is_refused_up_front() {
        assert!(one(entry("a", &["synthetic"])).is_ok());
        for refused in [
            entry("", &[]),
            entry("\0fed", &[]),
            entry("a", &["made\u{1f}up"]),
            document(""),
            document("\0fed"),
            Change::Delete(String::new()),
            Change::Delete("\0fed".to_owned()),
        ] {
            assert!(
                matches!(one(refused), Err(PluginError::InvalidInput(_))),
                "refused"
            );
        }
    }

    #[cfg(feature = "store")]
    #[test]
    fn changes_for_an_undeclared_tier_are_dropped_and_named() {
        let mut changes = Batch {
            changes: vec![
                entry("a", &[]),
                document("a"),
                Change::Delete("b".to_owned()),
                Change::Reset,
            ],
            next_cursor: "1".to_owned(),
            more: false,
        };
        let tiers = FeedTiers {
            catalogue: true,
            full_text: false,
        };
        let dropped = keep_declared_tiers(&mut changes, tiers);
        assert_eq!(dropped.len(), 1);
        assert!(dropped[0].contains("full-text document"));
        assert_eq!(changes.changes.len(), 3);
        assert!(
            !changes
                .changes
                .iter()
                .any(|change| matches!(change, Change::UpsertDocument(_)))
        );
    }

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

    fn field(name: &str, kind: FieldKind) -> Field {
        Field {
            name: name.into(),
            label: name.into(),
            kind,
            required: false,
            initial: None,
        }
    }

    fn form(fields: Vec<Field>) -> FormSpec {
        FormSpec {
            title: "Synthetic".into(),
            fields,
            submit_label: "Go".into(),
        }
    }

    #[test]
    fn a_form_refuses_duplicate_names_values_and_oversized_text() {
        let twice = form(vec![
            field("name", FieldKind::Toggle),
            field("name", FieldKind::Date),
        ]);
        assert!(form_spec(&twice).is_err());
        let option = |value: &str| ChoiceOption {
            value: value.into(),
            label: value.into(),
        };
        let shared = form(vec![field(
            "source",
            FieldKind::Choice(vec![option("web"), option("web")]),
        )]);
        assert!(form_spec(&shared).is_err());
        let text = |max_length: usize| {
            FieldKind::Text(TextOptions {
                max_length: u32::try_from(max_length).expect("fits"),
                placeholder: None,
                format: TextFormat::Plain,
            })
        };
        assert!(
            form_spec(&form(vec![field(
                "note",
                text(limits::MAX_FIELD_TEXT_BYTES)
            )]))
            .is_ok()
        );
        assert!(
            form_spec(&form(vec![field(
                "note",
                text(limits::MAX_FIELD_TEXT_BYTES + 1)
            )]))
            .is_err()
        );
    }

    #[test]
    fn long_error_and_outcome_text_is_cut_not_refused() {
        let long = "e".repeat(limits::MAX_MESSAGE_CHARS + 10);
        let PluginError::Internal(cut_error) = bound_error(PluginError::Internal(long.clone()))
        else {
            panic!("the case is kept");
        };
        assert_eq!(cut_error.chars().count(), limits::MAX_MESSAGE_CHARS);
        let Outcome::Uncertain(cut_detail) = bound_outcome(Outcome::Uncertain(long)) else {
            panic!("the case is kept");
        };
        assert_eq!(cut_detail.chars().count(), limits::MAX_MESSAGE_CHARS);
    }
}

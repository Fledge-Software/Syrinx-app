//! Turns one `DOMSnapshot.captureSnapshot` response into the root document's visible,
//! non-editable text (P1-25 steps 9 to 11), off the interface thread.

use atlas_core::{EligibleDocument, TEXT_LIMIT_BYTES};
use serde_json::{Map, Value};

use super::super::DiscardReason;

/// The largest snapshot response parsed at all; larger pages are skipped (F16).
pub(super) const RESPONSE_LIMIT_BYTES: usize = 32 * 1024 * 1024;

/// Parameters for the one approved call: only the `visibility` style (F1 found hidden text
/// otherwise kept), no DOM rectangles, no paint order, no colours.
pub(super) const CAPTURE_PARAMETERS: &str = r#"{"computedStyles":["visibility"],"includePaintOrder":false,"includeDOMRects":false,"includeBlendedBackgroundColors":false,"includeTextColorOpacities":false}"#;

const TEXT_NODE: i64 = 3;

/// Elements whose text is never kept: form fields, their internal trees, and non-content.
const DROPPED_ELEMENTS: &[&str] = &[
    "INPUT", "TEXTAREA", "SELECT", "SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "TITLE",
];

/// `autocomplete` tokens that mark a payment-card field (step 10).
const CARD_TOKENS: &[&str] = &[
    "cc-number",
    "cc-csc",
    "cc-exp",
    "cc-exp-month",
    "cc-exp-year",
];

/// The text kept from one snapshot.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct SnapshotText {
    pub(super) text: String,
    /// How many documents the response held, including discarded child frames.
    pub(super) documents: usize,
    pub(super) truncated: bool,
}

/// Steps 9 to 11. Input values are removed before anything else is read.
pub(super) fn read(json: &str, document: &EligibleDocument) -> Result<SnapshotText, DiscardReason> {
    if json.len() > RESPONSE_LIMIT_BYTES {
        return Err(DiscardReason::TooLarge);
    }
    let mut response: Value = serde_json::from_str(json).map_err(|_| DiscardReason::Malformed)?;
    drop_input_values(&mut response);
    let strings = strings(&response)?;
    let documents = array(&response, "documents")?;
    let root = documents.first().ok_or(DiscardReason::Malformed)?;
    // Step 9: the snapshot is of the judged document.
    let address = string_at(&strings, integer(root, "documentURL")?)?;
    if !document.matches_snapshot(address) {
        return Err(DiscardReason::AddressMismatch);
    }
    let nodes = Nodes::read(root, &strings)?;
    // Step 10: a sign-in or payment form discards the whole document.
    if nodes.has_sign_in_or_card_field() {
        return Err(DiscardReason::SignInOrPayment);
    }
    // Step 11: rendered, non-editable text of the root document only.
    let pieces = visible_pieces(root, &nodes, &strings)?;
    let (text, truncated) = bounded(normalize_whitespace(&join_pieces(&pieces)));
    Ok(SnapshotText {
        text,
        documents: documents.len(),
        truncated,
    })
}

/// Removes every document's `inputValue` and `textValue` so no later code can read them.
fn drop_input_values(response: &mut Value) {
    let Some(documents) = response.get_mut("documents").and_then(Value::as_array_mut) else {
        return;
    };
    for document in documents {
        if let Some(nodes) = document.get_mut("nodes").and_then(Value::as_object_mut) {
            nodes.remove("inputValue");
            nodes.remove("textValue");
        }
    }
}

/// The root document's node table, with what step 10 and 11 need per node.
struct Nodes {
    kind: Vec<i64>,
    names: Vec<String>,
    attributes: Vec<Vec<(String, String)>>,
    /// Whether the node is inside a form field, editable region or non-content element.
    dropped: Vec<bool>,
}

impl Nodes {
    fn read(root: &Value, strings: &[&str]) -> Result<Self, DiscardReason> {
        let nodes = object(root, "nodes")?;
        let parents = integers(nodes, "parentIndex")?;
        let kind = integers(nodes, "nodeType")?;
        let names = integers(nodes, "nodeName")?
            .into_iter()
            .map(|index| string_at(strings, index).map(str::to_ascii_uppercase))
            .collect::<Result<Vec<_>, _>>()?;
        let attributes = node_attributes(nodes, strings, names.len())?;
        if parents.len() != names.len() || kind.len() != names.len() {
            return Err(DiscardReason::Malformed);
        }
        let dropped = dropped_nodes(&parents, &names, &attributes)?;
        Ok(Self {
            kind,
            names,
            attributes,
            dropped,
        })
    }

    fn has_sign_in_or_card_field(&self) -> bool {
        self.names
            .iter()
            .zip(&self.attributes)
            .any(|(name, attributes)| is_sign_in_or_card_field(name, attributes))
    }
}

fn is_sign_in_or_card_field(name: &str, attributes: &[(String, String)]) -> bool {
    let attribute = |wanted: &str| {
        attributes
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.as_str())
    };
    let password = name == "INPUT"
        && attribute("type").is_some_and(|kind| kind.trim().eq_ignore_ascii_case("password"));
    let card = attribute("autocomplete").is_some_and(|tokens| {
        tokens.split_ascii_whitespace().any(|token| {
            CARD_TOKENS
                .iter()
                .any(|card| token.eq_ignore_ascii_case(card))
        })
    });
    password || card
}

/// Marks each node inside a dropped element or editable region. Nodes come in tree order,
/// so a parent is always marked before its children.
fn dropped_nodes(
    parents: &[i64],
    names: &[String],
    attributes: &[Vec<(String, String)>],
) -> Result<Vec<bool>, DiscardReason> {
    let mut dropped = Vec::with_capacity(names.len());
    for (index, parent) in parents.iter().enumerate() {
        let inherited = match usize::try_from(*parent) {
            Ok(parent) if parent < index => dropped[parent],
            Ok(_) => return Err(DiscardReason::Malformed),
            Err(_) => false,
        };
        dropped.push(inherited || drops_own_text(&names[index], &attributes[index]));
    }
    Ok(dropped)
}

fn drops_own_text(name: &str, attributes: &[(String, String)]) -> bool {
    let editable = attributes.iter().any(|(attribute, value)| {
        attribute.eq_ignore_ascii_case("contenteditable")
            && !value.trim().eq_ignore_ascii_case("false")
    });
    editable || DROPPED_ELEMENTS.contains(&name)
}

/// Each node's attributes as name/value pairs; the response lists them flattened.
fn node_attributes(
    nodes: &Map<String, Value>,
    strings: &[&str],
    count: usize,
) -> Result<Vec<Vec<(String, String)>>, DiscardReason> {
    let lists = nodes
        .get("attributes")
        .and_then(Value::as_array)
        .ok_or(DiscardReason::Malformed)?;
    if lists.len() != count {
        return Err(DiscardReason::Malformed);
    }
    lists
        .iter()
        .map(|list| {
            let indices = list
                .as_array()
                .ok_or(DiscardReason::Malformed)?
                .iter()
                .map(|index| index.as_i64().ok_or(DiscardReason::Malformed))
                .collect::<Result<Vec<_>, _>>()?;
            indices
                .chunks(2)
                .map(|pair| match pair {
                    [name, value] => Ok((
                        string_at(strings, *name)?.to_owned(),
                        attribute_value(strings, *value)?.to_owned(),
                    )),
                    _ => Err(DiscardReason::Malformed),
                })
                .collect()
        })
        .collect()
}

/// One laid-out text node: its text and its bounding box (x, y, width, height).
struct Piece<'a> {
    text: &'a str,
    bounds: [f64; 4],
}

/// The laid-out text nodes of the root document that step 11 keeps, in layout order.
fn visible_pieces<'a>(
    root: &Value,
    nodes: &Nodes,
    strings: &[&'a str],
) -> Result<Vec<Piece<'a>>, DiscardReason> {
    let layout = object(root, "layout")?;
    let node_indices = integers(layout, "nodeIndex")?;
    let texts = integers(layout, "text")?;
    let bounds = layout
        .get("bounds")
        .and_then(Value::as_array)
        .ok_or(DiscardReason::Malformed)?;
    let styles = layout.get("styles").and_then(Value::as_array);
    if texts.len() != node_indices.len() || bounds.len() != node_indices.len() {
        return Err(DiscardReason::Malformed);
    }
    let mut pieces = Vec::new();
    for (entry, node) in node_indices.iter().enumerate() {
        let node = usize::try_from(*node).map_err(|_| DiscardReason::Malformed)?;
        if !keeps_text(nodes, node)? || texts[entry] < 0 || hidden(styles, entry, strings)? {
            continue;
        }
        pieces.push(Piece {
            text: string_at(strings, texts[entry])?,
            bounds: rectangle(&bounds[entry])?,
        });
    }
    Ok(pieces)
}

fn keeps_text(nodes: &Nodes, node: usize) -> Result<bool, DiscardReason> {
    let kind = *nodes.kind.get(node).ok_or(DiscardReason::Malformed)?;
    Ok(kind == TEXT_NODE && !nodes.dropped[node])
}

/// Whether the requested `visibility` style hides this layout entry.
fn hidden(
    styles: Option<&Vec<Value>>,
    entry: usize,
    strings: &[&str],
) -> Result<bool, DiscardReason> {
    let Some(styles) = styles else {
        return Ok(false);
    };
    let values = styles
        .get(entry)
        .and_then(Value::as_array)
        .ok_or(DiscardReason::Malformed)?;
    let Some(visibility) = values.first() else {
        return Ok(false);
    };
    let visibility = string_at(
        strings,
        visibility.as_i64().ok_or(DiscardReason::Malformed)?,
    )?;
    Ok(matches!(visibility, "hidden" | "collapse"))
}

/// Joins pieces, without a space where one piece continues the previous on the same line.
fn join_pieces(pieces: &[Piece<'_>]) -> String {
    let mut text = String::new();
    let mut previous: Option<&Piece<'_>> = None;
    for piece in pieces {
        if previous.is_some_and(|previous| !continues(previous, piece)) {
            text.push(' ');
        }
        text.push_str(piece.text);
        previous = Some(piece);
    }
    text
}

/// Same line and touching: `Hello <b>world</b>!` stays one word sequence.
fn continues(previous: &Piece<'_>, next: &Piece<'_>) -> bool {
    let [x, y, width, height] = previous.bounds;
    let [next_x, next_y, _, next_height] = next.bounds;
    let same_line = next_y < y + height && y < next_y + next_height;
    same_line && (next_x - (x + width)).abs() <= 1.0
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Caps the text at `TEXT_LIMIT_BYTES`, on a character boundary.
fn bounded(mut text: String) -> (String, bool) {
    if text.len() <= TEXT_LIMIT_BYTES {
        return (text, false);
    }
    let mut end = TEXT_LIMIT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

fn strings(response: &Value) -> Result<Vec<&str>, DiscardReason> {
    array(response, "strings")?
        .iter()
        .map(|value| value.as_str().ok_or(DiscardReason::Malformed))
        .collect()
}

/// An attribute's value; the runtime sends -1 for an empty one, as in `contenteditable=""`.
fn attribute_value<'a>(strings: &[&'a str], index: i64) -> Result<&'a str, DiscardReason> {
    if index == -1 {
        return Ok("");
    }
    string_at(strings, index)
}

fn string_at<'a>(strings: &[&'a str], index: i64) -> Result<&'a str, DiscardReason> {
    let index = usize::try_from(index).map_err(|_| DiscardReason::Malformed)?;
    strings.get(index).copied().ok_or(DiscardReason::Malformed)
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, DiscardReason> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or(DiscardReason::Malformed)
}

fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Map<String, Value>, DiscardReason> {
    value
        .get(key)
        .and_then(Value::as_object)
        .ok_or(DiscardReason::Malformed)
}

fn integer(value: &Value, key: &str) -> Result<i64, DiscardReason> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or(DiscardReason::Malformed)
}

fn integers(map: &Map<String, Value>, key: &str) -> Result<Vec<i64>, DiscardReason> {
    map.get(key)
        .and_then(Value::as_array)
        .ok_or(DiscardReason::Malformed)?
        .iter()
        .map(|value| value.as_i64().ok_or(DiscardReason::Malformed))
        .collect()
}

fn rectangle(value: &Value) -> Result<[f64; 4], DiscardReason> {
    let numbers = value.as_array().ok_or(DiscardReason::Malformed)?;
    let mut rectangle = [0.0; 4];
    if numbers.len() != rectangle.len() {
        return Err(DiscardReason::Malformed);
    }
    for (slot, number) in rectangle.iter_mut().zip(numbers) {
        *slot = number.as_f64().ok_or(DiscardReason::Malformed)?;
    }
    Ok(rectangle)
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;

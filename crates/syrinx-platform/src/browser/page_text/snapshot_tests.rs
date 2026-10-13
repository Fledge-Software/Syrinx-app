//! Proves snapshot reading on synthetic DevTools responses, without a web view.

use super::*;
use serde_json::json;
use syrinx_core::{DocumentFacts, ObservedResponse, PageTextPolicy, judge};

const PAGE: &str = "https://plain.example.test/article";

fn document(address: &str) -> EligibleDocument {
    let policy = PageTextPolicy::new::<&str>(&[]).expect("synthetic policy");
    let responses = [ObservedResponse {
        address: address.to_owned(),
        status: 200,
        content_type: Some("text/html".to_owned()),
        cache_control: Vec::new(),
        destination: Some("document".to_owned()),
        method: "GET".to_owned(),
    }];
    let facts = DocumentFacts {
        address,
        document_address: address,
        redirects: &[],
        responses: &responses,
        left: None,
        tainted: false,
        generation: 1,
    };
    judge(Some(&policy), facts, 1).expect("eligible synthetic page")
}

/// One synthetic node: name, parent, attributes and, for text, its laid-out text and box.
struct Node {
    name: &'static str,
    parent: i64,
    attributes: Vec<(&'static str, &'static str)>,
    layout: Option<(String, [f64; 4], Option<&'static str>)>,
}

fn element(name: &'static str, parent: i64, attributes: Vec<(&'static str, &'static str)>) -> Node {
    Node {
        name,
        parent,
        attributes,
        layout: None,
    }
}

fn text(parent: i64, value: &str, bounds: [f64; 4]) -> Node {
    Node {
        name: "#text",
        parent,
        attributes: Vec::new(),
        layout: Some((value.to_owned(), bounds, None)),
    }
}

/// Builds a response shaped like `DOMSnapshot.captureSnapshot`'s, with string interning.
struct Response {
    strings: Vec<String>,
}

impl Response {
    fn intern(&mut self, value: &str) -> i64 {
        if let Some(index) = self.strings.iter().position(|known| known == value) {
            return index as i64;
        }
        self.strings.push(value.to_owned());
        (self.strings.len() - 1) as i64
    }

    fn document(&mut self, address: &str, nodes: &[Node], with_styles: bool) -> Value {
        let url = self.intern(address);
        let (mut parents, mut kinds, mut names, mut attributes) = (vec![], vec![], vec![], vec![]);
        let (mut layout_nodes, mut texts, mut bounds, mut styles) =
            (vec![], vec![], vec![], vec![]);
        for (index, node) in nodes.iter().enumerate() {
            parents.push(node.parent);
            kinds.push(if node.name == "#text" { 3 } else { 1 });
            names.push(self.intern(node.name));
            let flattened: Vec<i64> = node
                .attributes
                .iter()
                // The runtime sends -1 for an empty value, as WebView2 did in F10.
                .flat_map(|(name, value)| {
                    let value = if value.is_empty() {
                        -1
                    } else {
                        self.intern(value)
                    };
                    [self.intern(name), value]
                })
                .collect();
            attributes.push(flattened);
            if let Some((value, rectangle, visibility)) = &node.layout {
                layout_nodes.push(index as i64);
                texts.push(self.intern(value));
                bounds.push(rectangle.to_vec());
                styles.push(vec![self.intern(visibility.unwrap_or("visible"))]);
            }
        }
        let mut layout = json!({"nodeIndex": layout_nodes, "text": texts, "bounds": bounds});
        if with_styles {
            layout["styles"] = json!(styles);
        }
        json!({
            "documentURL": url,
            "nodes": {
                "parentIndex": parents,
                "nodeType": kinds,
                "nodeName": names,
                "attributes": attributes,
                "inputValue": {"index": [0], "value": [self.intern("typed-secret-sentinel")]},
                "textValue": {"index": [0], "value": [self.intern("typed-secret-sentinel")]},
            },
            "layout": layout,
        })
    }
}

fn response(documents: &[(&str, Vec<Node>)], with_styles: bool) -> String {
    let mut builder = Response {
        strings: Vec::new(),
    };
    let documents: Vec<Value> = documents
        .iter()
        .map(|(address, nodes)| builder.document(address, nodes, with_styles))
        .collect();
    json!({"documents": documents, "strings": builder.strings}).to_string()
}

fn page(nodes: Vec<Node>) -> String {
    response(&[(PAGE, nodes)], false)
}

const LINE: [f64; 4] = [0.0, 0.0, 40.0, 10.0];

#[test]
fn keeps_root_text_in_order_and_joins_inline_runs() {
    let json = page(vec![
        element("HTML", -1, vec![]),
        element("P", 0, vec![]),
        text(1, "Hello ", [0.0, 0.0, 30.0, 10.0]),
        element("B", 1, vec![]),
        text(3, "wor", [30.0, 0.0, 20.0, 10.0]),
        text(1, "ld!", [50.0, 0.0, 15.0, 10.0]),
        element("P", 0, vec![]),
        text(6, "Second  paragraph", [0.0, 20.0, 80.0, 10.0]),
    ]);
    let read = read(&json, &document(PAGE)).expect("synthetic page reads");
    assert_eq!(read.text, "Hello world! Second paragraph");
    assert_eq!(read.documents, 1);
    assert!(!read.truncated);
}

#[test]
fn input_values_editable_regions_and_scripts_are_never_kept() {
    let json = page(vec![
        element("HTML", -1, vec![]),
        text(0, "Visible", LINE),
        element("INPUT", 0, vec![("type", "text")]),
        text(2, "input-sentinel", [0.0, 20.0, 40.0, 10.0]),
        element("TEXTAREA", 0, vec![]),
        text(4, "textarea-sentinel", [0.0, 40.0, 40.0, 10.0]),
        element("DIV", 0, vec![("contenteditable", "")]),
        element("SPAN", 6, vec![]),
        text(7, "editable-sentinel", [0.0, 60.0, 40.0, 10.0]),
        element("DIV", 0, vec![("contenteditable", "false")]),
        text(9, "Static island", [0.0, 80.0, 40.0, 10.0]),
        element("SCRIPT", 0, vec![]),
        text(11, "script-sentinel", [0.0, 100.0, 40.0, 10.0]),
    ]);
    let read = read(&json, &document(PAGE)).expect("synthetic page reads");
    assert_eq!(read.text, "Visible Static island");
    assert!(!read.text.contains("sentinel"));
}

#[test]
fn password_and_card_fields_discard_the_document() {
    for attributes in [
        vec![("type", "password")],
        vec![("type", " PASSWORD ")],
        vec![("autocomplete", "cc-number")],
        vec![("autocomplete", "section-pay billing CC-CSC")],
        vec![("autocomplete", "cc-exp-month")],
    ] {
        let json = page(vec![
            element("HTML", -1, vec![]),
            text(0, "Sign in", LINE),
            element("INPUT", 0, attributes.clone()),
        ]);
        assert_eq!(
            read(&json, &document(PAGE)),
            Err(DiscardReason::SignInOrPayment),
            "{attributes:?}"
        );
    }
    let ordinary = page(vec![
        element("HTML", -1, vec![]),
        element(
            "INPUT",
            0,
            vec![("type", "email"), ("autocomplete", "email")],
        ),
    ]);
    assert!(read(&ordinary, &document(PAGE)).is_ok());
}

#[test]
fn a_snapshot_of_another_address_is_discarded() {
    let json = response(
        &[(
            "https://bank.test/",
            vec![element("HTML", -1, vec![]), text(0, "bank-sentinel", LINE)],
        )],
        false,
    );
    assert_eq!(
        read(&json, &document(PAGE)),
        Err(DiscardReason::AddressMismatch)
    );
}

#[test]
fn child_frame_documents_are_counted_but_never_read() {
    let json = response(
        &[
            (
                PAGE,
                vec![element("HTML", -1, vec![]), text(0, "Root words", LINE)],
            ),
            (
                "https://plain.example.test/frame",
                vec![element("HTML", -1, vec![]), text(0, "frame-sentinel", LINE)],
            ),
        ],
        false,
    );
    let read = read(&json, &document(PAGE)).expect("synthetic page reads");
    assert_eq!(read.text, "Root words");
    assert_eq!(read.documents, 2);
}

#[test]
fn visibility_style_hides_text_when_requested() {
    let mut hidden = text(0, "hidden-sentinel", [0.0, 20.0, 40.0, 10.0]);
    hidden.layout = hidden
        .layout
        .map(|(value, bounds, _)| (value, bounds, Some("hidden")));
    let json = response(
        &[(
            PAGE,
            vec![element("HTML", -1, vec![]), text(0, "Shown", LINE), hidden],
        )],
        true,
    );
    assert_eq!(
        read(&json, &document(PAGE)).map(|read| read.text),
        Ok("Shown".into())
    );
}

#[test]
fn oversized_malformed_and_out_of_range_responses_are_discarded() {
    let oversized = " ".repeat(RESPONSE_LIMIT_BYTES + 1);
    assert_eq!(
        read(&oversized, &document(PAGE)),
        Err(DiscardReason::TooLarge)
    );
    for json in [
        "not json".to_owned(),
        "{}".to_owned(),
        json!({"documents": [], "strings": []}).to_string(),
        json!({"documents": [{"documentURL": 9}], "strings": []}).to_string(),
    ] {
        assert_eq!(
            read(&json, &document(PAGE)),
            Err(DiscardReason::Malformed),
            "{json}"
        );
    }
    // A parent that follows its child breaks tree order.
    let json = page(vec![
        element("HTML", 1, vec![]),
        element("BODY", -1, vec![]),
    ]);
    assert_eq!(read(&json, &document(PAGE)), Err(DiscardReason::Malformed));
}

#[test]
fn long_text_is_capped_on_a_character_boundary() {
    let long = "\u{e9}".repeat(TEXT_LIMIT_BYTES);
    let json = page(vec![element("HTML", -1, vec![]), text(0, &long, LINE)]);
    let read = read(&json, &document(PAGE)).expect("synthetic page reads");
    assert!(read.truncated);
    assert_eq!(read.text.len(), TEXT_LIMIT_BYTES);
    assert!(read.text.chars().all(|character| character == '\u{e9}'));
}

#[test]
fn input_values_are_removed_before_reading() {
    let mut value: Value = serde_json::from_str(&page(vec![element("HTML", -1, vec![])]))
        .expect("synthetic response parses");
    drop_input_values(&mut value);
    let nodes = &value["documents"][0]["nodes"];
    assert!(nodes.get("inputValue").is_none());
    assert!(nodes.get("textValue").is_none());
}

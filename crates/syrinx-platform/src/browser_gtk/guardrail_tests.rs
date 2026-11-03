//! Reads the Linux page engine's source for the page-view rules in `AGENTS.md`, so a change that
//! gives a page a custom identity, an injected script or a bridge into Syrinx fails the gate.

use std::{fs, path::PathBuf};

/// Calls that would change what a page sees or let it reach Syrinx (`AGENTS.md`, "Page web views
/// and Google"). Each is refused anywhere in `browser_gtk/`.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "user_agent",
        "a custom user agent shows Google the WebKitGTK brand",
    ),
    (
        "UserContentManager",
        "a user content manager injects scripts or styles",
    ),
    ("UserScript", "a user script runs inside the page"),
    ("UserStyleSheet", "a user style sheet changes the page"),
    ("add_script", "a user script runs inside the page"),
    ("add_style_sheet", "a user style sheet changes the page"),
    (
        "register_script_message_handler",
        "a message handler is an IPC bridge",
    ),
    (
        "register_uri_scheme",
        "a custom scheme on the page context is a bridge into Syrinx",
    ),
    ("run_javascript", "Syrinx never runs script in a page"),
    ("evaluate_javascript", "Syrinx never runs script in a page"),
    (
        "call_async_javascript",
        "Syrinx never runs script in a page",
    ),
    (
        "set_enable_developer_extras(true)",
        "the inspector stays off on pages",
    ),
    (
        "set_sandbox_enabled(false)",
        "WebKit's bubblewrap sandbox stays on",
    ),
];

/// Every Rust file of the Linux page engine except this one, with its text.
fn engine_sources() -> Vec<(PathBuf, String)> {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/browser_gtk");
    let mut sources = Vec::new();
    for entry in fs::read_dir(&folder).expect("read browser_gtk") {
        let path = entry.expect("browser_gtk entry").path();
        let is_rust = path.extension().is_some_and(|extension| extension == "rs");
        if !is_rust || path.ends_with("guardrail_tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read engine source");
        sources.push((path, text));
    }
    assert!(sources.len() > 5, "the engine's sources were found");
    sources
}

fn source(name: &str) -> String {
    engine_sources()
        .into_iter()
        .find(|(path, _)| path.ends_with(name))
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("{name} is part of the Linux engine"))
}

#[test]
fn no_page_gets_an_identity_script_or_bridge() {
    let mut found = Vec::new();
    for (path, text) in engine_sources() {
        for (call, reason) in FORBIDDEN {
            if text.contains(call) {
                found.push(format!("{}: {call} ({reason})", path.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "page-view rules broken:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_sandbox_is_on_and_the_inspector_off() {
    assert!(source("engine.rs").contains("context.set_sandbox_enabled(true);"));
    assert!(source("page.rs").contains("settings.set_enable_developer_extras(false);"));
}

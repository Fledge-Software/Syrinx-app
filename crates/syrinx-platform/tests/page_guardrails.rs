//! Reads the Windows page engine's source for the page-view rules in `AGENTS.md`, so a page web
//! view or popup that gains a bridge into Syrinx, an injected script or a custom identity fails
//! the gate. It catches direct calls only, so the native plugin-view type must live outside
//! `src/browser/` and be the only code that holds these calls.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Calls that would let a page reach Syrinx or change what it sees (`AGENTS.md`, "Page web views
/// and Google"); each is refused in `src/browser.rs` and anywhere in `src/browser/`.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "with_ipc_handler",
        "an IPC handler adds window.ipc, which breaks Google Docs, and is a bridge into Syrinx",
    ),
    (
        "add_WebMessageReceived",
        "a web message handler is a bridge into Syrinx",
    ),
    (
        "with_custom_protocol",
        "a custom protocol serves Syrinx's files to a page",
    ),
    (
        "with_asynchronous_custom_protocol",
        "a custom protocol serves Syrinx's files to a page",
    ),
    (
        "SetVirtualHostNameToFolderMapping",
        "a virtual host serves a local folder to a page",
    ),
    (
        "with_initialization_script",
        "an initialization script runs inside the page",
    ),
    (
        "AddScriptToExecuteOnDocumentCreated",
        "a document-created script runs inside the page",
    ),
    ("ExecuteScript", "Syrinx never runs script in a page"),
    (
        "AddWebResourceRequestedFilter",
        "a request filter is the middleman Google blocks",
    ),
    (
        "AddHostObjectToScript",
        "a host object projects native code into the page",
    ),
    (
        "with_user_agent",
        "a custom user agent shows Google the WebView2 brand",
    ),
    (
        "SetUserAgent",
        "a custom user agent shows Google the WebView2 brand",
    ),
    (
        "--user-agent",
        "a user-agent browser argument shows Google another brand",
    ),
    (
        "setUserAgentOverride",
        "a DevTools user-agent override shows Google another brand",
    ),
];

/// `src/browser.rs` and every Rust file under `src/browser/`, with its text.
fn engine_sources() -> Vec<(PathBuf, String)> {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = vec![read_source(&source.join("browser.rs"))];
    collect_sources(&source.join("browser"), &mut sources);
    sources
}

/// Add the Rust files in `folder` and its subfolders to `sources`.
fn collect_sources(folder: &Path, sources: &mut Vec<(PathBuf, String)>) {
    for entry in fs::read_dir(folder).expect("read the browser folder") {
        let path = entry.expect("browser folder entry").path();
        if path.is_dir() {
            collect_sources(&path, sources);
            continue;
        }
        if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(read_source(&path));
        }
    }
}

fn read_source(path: &Path) -> (PathBuf, String) {
    let text = fs::read_to_string(path).expect("read engine source");
    (path.to_path_buf(), text)
}

/// Each forbidden call found in `sources`, with its file and reason.
fn forbidden_calls(sources: &[(PathBuf, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (path, text) in sources {
        for (call, reason) in FORBIDDEN {
            if text.contains(call) {
                found.push(format!("{}: {call} ({reason})", path.display()));
            }
        }
    }
    found
}

#[test]
fn the_page_engine_was_read() {
    let sources = engine_sources();
    let page = sources
        .iter()
        .find(|(path, _)| path.ends_with("src/browser/page.rs"))
        .expect("browser/page.rs is among the files read");
    assert!(page.1.contains("WebViewBuilder"), "page.rs builds the page");
    let module = sources
        .iter()
        .any(|(path, _)| path.ends_with("src/browser.rs"));
    assert!(module, "browser.rs is among the files read");
}

#[test]
fn a_planted_call_is_reported() {
    let planted = vec![(
        PathBuf::from("planted.rs"),
        "builder.with_ipc_handler(handler)".to_string(),
    )];
    let found = forbidden_calls(&planted);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("planted.rs: with_ipc_handler"),
        "{found:?}"
    );
}

#[test]
fn no_page_gets_a_bridge_script_or_identity() {
    let found = forbidden_calls(&engine_sources());
    assert!(
        found.is_empty(),
        "page-view rules broken:\n{}",
        found.join("\n")
    );
}

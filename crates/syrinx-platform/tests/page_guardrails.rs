//! Reads the Windows page engine's source for the page-view rules in `AGENTS.md`, so a page web
//! view or popup that gains a bridge into Syrinx, an injected script or a custom identity fails
//! the gate. The plugin view's native type therefore lives outside `src/browser/`.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Calls that would let a page reach Syrinx or change what it sees (`AGENTS.md`, "Page web views
/// and Google"); each is refused anywhere in `src/browser/`.
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
        "with_initialization_script",
        "an initialization script runs inside the page",
    ),
    (
        "AddScriptToExecuteOnDocumentCreated",
        "a document-created script runs inside the page",
    ),
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
];

/// Every Rust file under `src/browser/`, with its text.
fn engine_sources() -> Vec<(PathBuf, String)> {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/browser");
    let mut sources = Vec::new();
    collect_sources(&folder, &mut sources);
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
            let text = fs::read_to_string(&path).expect("read engine source");
            sources.push((path, text));
        }
    }
}

#[test]
fn the_page_engine_was_read() {
    let sources = engine_sources();
    let read_page = sources
        .iter()
        .any(|(path, _)| path.ends_with("src/browser/page.rs"));
    assert!(read_page, "browser/page.rs is among the files read");
}

#[test]
fn no_page_gets_a_bridge_script_or_identity() {
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

//! What the resolver serves, the paths it refuses and the checks against the signed list.

use std::sync::atomic::{AtomicU8, Ordering};

use super::{FileResolver, ListedFile, Missing, Response, Served};

const PAGE: &[u8] = b"<p>hello</p>";
const SCRIPT: &[u8] = b"console.log(1)";

/// An obviously fake digest that is still one-to-one for short files: the length, then the
/// bytes. The resolver only compares digests.
fn fake_sha256(bytes: &[u8]) -> Result<[u8; 32], String> {
    if bytes.len() >= 32 {
        return Err("the fake digest takes at most 31 bytes".into());
    }
    let mut digest = [0_u8; 32];
    digest[0] = bytes.len() as u8;
    digest[1..=bytes.len()].copy_from_slice(bytes);
    Ok(digest)
}

/// Fills every byte with a counter that moves on each call, so each nonce differs.
fn counting_fill(bytes: &mut [u8]) -> Result<(), String> {
    static CALLS: AtomicU8 = AtomicU8::new(0);
    bytes.fill(CALLS.fetch_add(1, Ordering::Relaxed));
    Ok(())
}

fn broken_fill(_: &mut [u8]) -> Result<(), String> {
    Err("no random source".into())
}

fn listed(path: &str, bytes: &[u8]) -> ListedFile {
    ListedFile {
        path: path.into(),
        size: bytes.len() as u64,
        sha256: fake_sha256(bytes).expect("a short fixture"),
    }
}

/// The package as stored: the listed page and script, a script the list does not name, a
/// file whose stored bytes differ from the list, and a style the reader cannot load.
fn stored(path: &str) -> Result<Vec<u8>, String> {
    match path {
        "ui/index.html" => Ok(PAGE.to_vec()),
        "ui/js/app.js" | "ui/extra.js" => Ok(SCRIPT.to_vec()),
        "ui/grown.css" => Ok(b"p { color: red; }".to_vec()),
        "ui/swapped.css" => Ok(b"p{color:red}".to_vec()),
        _ => Err(format!("{path} is not in the package")),
    }
}

/// How the tests' package is read.
type Reader = fn(&str) -> Result<Vec<u8>, String>;

fn resolver() -> FileResolver<Reader> {
    let files = vec![
        listed("ui/index.html", PAGE),
        listed("ui/js/app.js", SCRIPT),
        listed("ui/grown.css", b"p{}"),
        listed("ui/swapped.css", b"p{color:blue"),
        listed("ui/gone.css", b"p{}"),
        listed("module.wasm", b"\0asm"),
    ];
    FileResolver::new(files, "ui/", fake_sha256, counting_fill, stored)
}

fn served(response: Response) -> Served {
    match response {
        Response::Found(served) => served,
        Response::NotFound(missing) => panic!("expected a file, got {missing:?}"),
    }
}

fn header<'a>(served: &'a Served, name: &str) -> Option<&'a str> {
    served
        .headers
        .iter()
        .find(|(header, _)| *header == name)
        .map(|(_, value)| value.as_str())
}

fn missing(path: &str) -> Missing {
    match resolver().resolve(path) {
        Response::NotFound(missing) => missing,
        Response::Found(_) => panic!("{path} should not be served"),
    }
}

#[test]
fn the_root_serves_the_index_with_the_policy() {
    let response = resolver().resolve("/");
    assert_eq!(response.status(), 200);
    let page = served(response);
    assert_eq!(page.body, PAGE);
    assert_eq!(
        header(&page, "Content-Type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(header(&page, "X-Content-Type-Options"), Some("nosniff"));
    let policy = header(&page, "Content-Security-Policy").expect("a document has the policy");
    assert!(policy.starts_with("default-src 'none'; "), "{policy}");
}

#[test]
fn a_nested_script_is_served_without_a_policy() {
    let script = served(resolver().resolve("/js/app.js"));
    assert_eq!(script.body, SCRIPT);
    assert_eq!(
        header(&script, "Content-Type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(header(&script, "X-Content-Type-Options"), Some("nosniff"));
    assert_eq!(header(&script, "Content-Security-Policy"), None);
}

#[test]
fn each_document_load_gets_a_fresh_nonce() {
    let resolver = resolver();
    let first = served(resolver.resolve("/index.html"));
    let second = served(resolver.resolve("/index.html"));
    assert_ne!(
        header(&first, "Content-Security-Policy"),
        header(&second, "Content-Security-Policy")
    );
}

#[test]
fn a_document_without_a_nonce_is_not_served() {
    let files = vec![listed("ui/index.html", PAGE)];
    let resolver = FileResolver::new(files, "ui/", fake_sha256, broken_fill, stored);
    assert_eq!(
        resolver.resolve("/"),
        Response::NotFound(Missing::NonceFailed("no random source".into()))
    );
}

#[test]
fn a_path_that_is_not_plain_is_refused() {
    for path in [
        "",
        "index.html",
        "/index.html?x=1",
        "/index.html#top",
        "/%69ndex.html",
        "/js\\app.js",
        "/js/../index.html",
        "/./index.html",
        "/js//app.js",
        "//example.com/index.html",
        "/js/",
    ] {
        assert_eq!(missing(path), Missing::BadPath, "{path:?}");
    }
}

#[test]
fn only_listed_files_inside_the_ui_folder_are_served() {
    assert_eq!(missing("/extra.js"), Missing::Unlisted);
    assert_eq!(missing("/module.wasm"), Missing::Unlisted);
}

#[test]
fn bytes_that_differ_from_the_list_are_refused() {
    assert_eq!(
        missing("/grown.css"),
        Missing::SizeMismatch {
            listed: 3,
            read: 17
        }
    );
    assert_eq!(missing("/swapped.css"), Missing::HashMismatch);
    assert_eq!(
        missing("/gone.css"),
        Missing::ReadFailed("ui/gone.css is not in the package".into())
    );
}

#[test]
fn a_failed_digest_is_refused() {
    let long = [b'x'; 40];
    let files = vec![
        listed("ui/index.html", PAGE),
        ListedFile {
            path: "ui/long.txt".into(),
            size: 40,
            sha256: [0; 32],
        },
    ];
    let read = |path: &str| match path {
        "ui/long.txt" => Ok(long.to_vec()),
        _ => stored(path),
    };
    let resolver = FileResolver::new(files, "ui/", fake_sha256, counting_fill, read);
    assert!(matches!(
        resolver.resolve("/long.txt"),
        Response::NotFound(Missing::HashFailed(_))
    ));
}

#[test]
fn a_refused_request_is_a_404() {
    assert_eq!(resolver().resolve("/swapped.css").status(), 404);
}

#[test]
fn media_types_follow_the_extension() {
    let cases = [
        ("app.mjs", "text/javascript; charset=utf-8"),
        ("style.css", "text/css; charset=utf-8"),
        ("data.json", "application/json"),
        ("icon.svg", "image/svg+xml"),
        ("icon.png", "image/png"),
        ("module.wasm", "application/wasm"),
        ("font.woff2", "font/woff2"),
        ("notes.unknown", "application/octet-stream"),
        ("INDEX.HTML", "application/octet-stream"),
        ("README", "application/octet-stream"),
    ];
    for (path, expected) in cases {
        assert_eq!(super::media_type(path).0, expected, "{path}");
    }
    assert!(
        super::media_type("icon.svg").1,
        "an SVG document can run script"
    );
}

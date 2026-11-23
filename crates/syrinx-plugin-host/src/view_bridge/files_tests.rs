//! What the resolver serves, the paths it refuses and the checks against the signed list.

use std::sync::atomic::{AtomicU8, Ordering};

use super::{FileResolver, InvalidUiFolder, ListedFile, Missing, Served};
use crate::view_bridge::{Nonce, content_security_policy};

const PAGE: &[u8] = b"<p>hello</p>";
const SCRIPT: &[u8] = b"console.log(1)";
const ICON: &[u8] = b"<svg/>";
const FONT: &[u8] = b"wOF2";

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

/// Always the same bytes, so a test knows the nonce a load got.
fn fixed_fill(bytes: &mut [u8]) -> Result<(), String> {
    bytes.fill(0x5a);
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

/// The package as stored: the listed page, script, icon and font, a script the list does not
/// name, `grown.css` whose size differs from the list, `swapped.css` whose digest differs,
/// and `uix.js` beside the ui folder. Anything else (`gone.css`) cannot be read.
fn stored(path: &str) -> Result<Vec<u8>, String> {
    match path {
        "ui/index.html" => Ok(PAGE.to_vec()),
        "ui/js/app.js" | "ui/extra.js" | "uix.js" => Ok(SCRIPT.to_vec()),
        "ui/icon.svg" => Ok(ICON.to_vec()),
        "ui/font.woff2" => Ok(FONT.to_vec()),
        "ui/grown.css" => Ok(b"p { color: red; }".to_vec()),
        "ui/swapped.css" => Ok(b"p{color:red}".to_vec()),
        _ => Err(format!("{path} is not in the package")),
    }
}

/// How the tests' package is read.
type Reader = fn(&str) -> Result<Vec<u8>, String>;

fn package() -> Vec<ListedFile> {
    vec![
        listed("ui/index.html", PAGE),
        listed("ui/js/app.js", SCRIPT),
        listed("ui/icon.svg", ICON),
        listed("ui/font.woff2", FONT),
        listed("ui/grown.css", b"p{}"),
        listed("ui/swapped.css", b"p{color:blue"),
        listed("ui/gone.css", b"p{}"),
        listed("module.wasm", b"\0asm"),
        listed("uix.js", SCRIPT),
    ]
}

fn resolver_with(fill: fn(&mut [u8]) -> Result<(), String>) -> FileResolver<Reader> {
    FileResolver::new(package(), "ui/", fake_sha256, fill, stored as Reader)
        .expect("a plain ui folder")
}

fn resolver() -> FileResolver<Reader> {
    resolver_with(counting_fill)
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
        Err(missing) => missing,
        Ok(_) => panic!("{path} should not be served"),
    }
}

#[test]
fn the_root_serves_the_index_with_this_load_s_policy() {
    let page = resolver_with(fixed_fill)
        .resolve("/")
        .expect("the index is served");
    let nonce = Nonce::generate(fixed_fill).expect("the nonce is made");
    assert_eq!(page.body, PAGE);
    assert_eq!(
        header(&page, "Content-Type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(header(&page, "X-Content-Type-Options"), Some("nosniff"));
    assert_eq!(
        header(&page, "Content-Security-Policy"),
        Some(content_security_policy(&nonce).as_str())
    );
}

#[test]
fn every_file_carries_this_load_s_policy() {
    let resolver = resolver_with(fixed_fill);
    let nonce = Nonce::generate(fixed_fill).expect("the nonce is made");
    let policy = content_security_policy(&nonce);
    for (path, media_type) in [
        ("/js/app.js", "text/javascript; charset=utf-8"),
        ("/icon.svg", "image/svg+xml"),
        ("/font.woff2", "font/woff2"),
    ] {
        let file = resolver.resolve(path).expect("a listed file is served");
        assert_eq!(header(&file, "Content-Type"), Some(media_type), "{path}");
        assert_eq!(header(&file, "X-Content-Type-Options"), Some("nosniff"));
        assert_eq!(
            header(&file, "Content-Security-Policy"),
            Some(policy.as_str()),
            "{path}: a worker runs under its own response's policy"
        );
    }
}

#[test]
fn each_load_gets_a_fresh_nonce() {
    let resolver = resolver();
    let first = resolver
        .resolve("/index.html")
        .expect("the index is served");
    let second = resolver
        .resolve("/index.html")
        .expect("the index is served");
    assert_ne!(
        header(&first, "Content-Security-Policy"),
        header(&second, "Content-Security-Policy")
    );
}

#[test]
fn a_file_without_a_nonce_is_not_served() {
    assert_eq!(
        resolver_with(broken_fill).resolve("/js/app.js"),
        Err(Missing::NonceFailed("no random source".into()))
    );
}

#[test]
fn a_folder_without_its_slash_cannot_reach_a_neighbour() {
    let joined = FileResolver::new(package(), "ui", fake_sha256, counting_fill, stored);
    assert_eq!(
        joined.err(),
        Some(InvalidUiFolder("ui".into())),
        "ui + x.js would be uix.js"
    );
}

#[test]
fn a_ui_folder_that_is_not_plain_is_refused() {
    for folder in ["ui", "", "/", "/ui/", "./ui/", "ui/../", "ui//", "../ui/"] {
        let resolver = FileResolver::new(package(), folder, fake_sha256, counting_fill, stored);
        assert_eq!(
            resolver.err(),
            Some(InvalidUiFolder(folder.into())),
            "{folder:?}"
        );
    }
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
    assert_eq!(missing("/../uix.js"), Missing::BadPath);
    assert_eq!(missing("x.js"), Missing::BadPath);
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
    let files = vec![ListedFile {
        path: "ui/long.txt".into(),
        size: 40,
        sha256: [0; 32],
    }];
    let read = |path: &str| match path {
        "ui/long.txt" => Ok(long.to_vec()),
        _ => stored(path),
    };
    let resolver = FileResolver::new(files, "ui/", fake_sha256, counting_fill, read)
        .expect("a plain ui folder");
    assert!(matches!(
        resolver.resolve("/long.txt"),
        Err(Missing::HashFailed(_))
    ));
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
        assert_eq!(super::media_type(path), expected, "{path}");
    }
}

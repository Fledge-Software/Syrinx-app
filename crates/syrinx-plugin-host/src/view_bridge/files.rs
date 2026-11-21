//! Serves a plugin view's files from its signed package: only listed files, only with their
//! listed size and digest, and every document with Syrinx's Content Security Policy.

use std::collections::BTreeMap;

use super::csp::{Nonce, RandomFill, content_security_policy};

/// A SHA-256 of a file's bytes; the application passes `syrinx_update::sha256`, which this
/// crate cannot link. #266 defines the same alias for the plugin cache; the two are merged
/// once it lands.
pub type Sha256 = fn(&[u8]) -> Result<[u8; 32], String>;

/// One file of the package's signed list, the shape of `syrinx_update::FileEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// The path inside the package, such as `ui/index.html`.
    pub path: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

/// The answer to one request on the view's origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Found(Served),
    /// Sent as an empty 404; the reason is for Syrinx's log, never for the view.
    NotFound(Missing),
}

impl Response {
    pub fn status(&self) -> u16 {
        match self {
            Response::Found(_) => 200,
            Response::NotFound(_) => 404,
        }
    }
}

/// A 200: the file's bytes and the headers to send with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

/// Why a request was answered with a 404.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// A query, fragment, escape, backslash, dot segment or empty segment.
    BadPath,
    Unlisted,
    ReadFailed(String),
    SizeMismatch {
        listed: u64,
        read: u64,
    },
    HashFailed(String),
    HashMismatch,
    /// No nonce could be made, so the document cannot get its policy.
    NonceFailed(String),
}

/// A file's media type by extension, and whether it is a document that can run script and
/// so needs the policy; an extension not listed is served as `application/octet-stream`.
const MEDIA_TYPES: &[(&str, &str, bool)] = &[
    ("html", "text/html; charset=utf-8", true),
    ("htm", "text/html; charset=utf-8", true),
    ("svg", "image/svg+xml", true),
    ("js", "text/javascript; charset=utf-8", false),
    ("mjs", "text/javascript; charset=utf-8", false),
    ("css", "text/css; charset=utf-8", false),
    ("json", "application/json", false),
    ("txt", "text/plain; charset=utf-8", false),
    ("png", "image/png", false),
    ("jpg", "image/jpeg", false),
    ("jpeg", "image/jpeg", false),
    ("gif", "image/gif", false),
    ("webp", "image/webp", false),
    ("wasm", "application/wasm", false),
    ("woff", "font/woff", false),
    ("woff2", "font/woff2", false),
];
const UNKNOWN_MEDIA_TYPE: &str = "application/octet-stream";

/// One plugin package's interface files, checked against its signed list on every read.
pub struct FileResolver<R> {
    files: BTreeMap<String, ListedFile>,
    /// The manifest's `entry.ui`, such as `ui/`.
    ui_folder: String,
    sha256: Sha256,
    random: RandomFill,
    read: R,
}

impl<R> FileResolver<R>
where
    R: Fn(&str) -> Result<Vec<u8>, String>,
{
    /// `read` loads a package path's bytes; `random` makes each document's nonce.
    pub fn new(
        files: Vec<ListedFile>,
        ui_folder: &str,
        sha256: Sha256,
        random: RandomFill,
        read: R,
    ) -> Self {
        let files = files
            .into_iter()
            .map(|file| (file.path.clone(), file))
            .collect();
        Self {
            files,
            ui_folder: ui_folder.into(),
            sha256,
            random,
            read,
        }
    }

    /// Answer a request for `request_path`, the URL's path on the view's origin.
    pub fn resolve(&self, request_path: &str) -> Response {
        match self.serve(request_path) {
            Ok(served) => Response::Found(served),
            Err(missing) => Response::NotFound(missing),
        }
    }

    fn serve(&self, request_path: &str) -> Result<Served, Missing> {
        let relative = relative_path(request_path).ok_or(Missing::BadPath)?;
        let package_path = format!("{}{relative}", self.ui_folder);
        let listed = self.files.get(&package_path).ok_or(Missing::Unlisted)?;
        let body = (self.read)(&package_path).map_err(Missing::ReadFailed)?;
        self.verify(listed, &body)?;
        let headers = self.headers(relative)?;
        Ok(Served { headers, body })
    }

    /// The bytes read must be the ones the signed list names.
    fn verify(&self, listed: &ListedFile, body: &[u8]) -> Result<(), Missing> {
        let read = body.len() as u64;
        if read != listed.size {
            return Err(Missing::SizeMismatch {
                listed: listed.size,
                read,
            });
        }
        let digest = (self.sha256)(body).map_err(Missing::HashFailed)?;
        if digest != listed.sha256 {
            return Err(Missing::HashMismatch);
        }
        Ok(())
    }

    /// The media type, `nosniff`, and for a document the policy with a fresh nonce.
    fn headers(&self, relative: &str) -> Result<Vec<(&'static str, String)>, Missing> {
        let (media_type, is_document) = media_type(relative);
        let mut headers = vec![
            ("Content-Type", media_type.to_string()),
            ("X-Content-Type-Options", "nosniff".to_string()),
        ];
        if is_document {
            let nonce = Nonce::generate(self.random).map_err(Missing::NonceFailed)?;
            headers.push(("Content-Security-Policy", content_security_policy(&nonce)));
        }
        Ok(headers)
    }
}

/// The path below the ui folder (`/` is `index.html`), or `None` for anything but plain
/// names separated by single slashes.
fn relative_path(request_path: &str) -> Option<&str> {
    if request_path.contains(['?', '#', '%', '\\']) {
        return None;
    }
    let relative = request_path.strip_prefix('/')?;
    if relative.is_empty() {
        return Some("index.html");
    }
    let plain = relative
        .split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    plain.then_some(relative)
}

fn media_type(path: &str) -> (&'static str, bool) {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let extension = file_name.rsplit_once('.').map(|(_, extension)| extension);
    MEDIA_TYPES
        .iter()
        .find(|(known, _, _)| Some(*known) == extension)
        .map_or(
            (UNKNOWN_MEDIA_TYPE, false),
            |(_, media_type, is_document)| (*media_type, *is_document),
        )
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;

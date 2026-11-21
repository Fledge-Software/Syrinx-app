//! Serves a plugin view's files from its signed package: only listed files, only with their
//! listed size and digest, and each with Syrinx's Content Security Policy.

use std::collections::BTreeMap;

use super::csp::{Nonce, RandomFill, content_security_policy};
use crate::manifest::is_ui_folder;

/// A SHA-256 of a file's bytes; the application passes a thin wrapper around
/// `syrinx_update::sha256`, which this crate cannot link, turning its `UpdateError` into text.
/// #266 defines the same alias for the plugin cache; the two are merged once it lands.
pub type Sha256 = fn(&[u8]) -> Result<[u8; 32], String>;

/// One file of the package's signed list, the shape of `syrinx_update::FileEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// The path inside the package, such as `ui/index.html`.
    pub path: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

/// A 200: the file's bytes and the headers to send with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

/// Why a request is answered with an empty 404; the reason is for Syrinx's log, never for
/// the view.
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
    /// No nonce could be made, so the file cannot get its policy.
    NonceFailed(String),
}

/// An `entry.ui` that is not a relative folder of plain names ending with `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidUiFolder(pub String);

/// A file's media type by extension; an extension not listed is `application/octet-stream`.
const MEDIA_TYPES: &[(&str, &str)] = &[
    ("html", "text/html; charset=utf-8"),
    ("htm", "text/html; charset=utf-8"),
    ("svg", "image/svg+xml"),
    ("js", "text/javascript; charset=utf-8"),
    ("mjs", "text/javascript; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("json", "application/json"),
    ("txt", "text/plain; charset=utf-8"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("wasm", "application/wasm"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
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
    /// `read` loads a package path's bytes; `random` makes each response's nonce.
    pub fn new(
        files: Vec<ListedFile>,
        ui_folder: &str,
        sha256: Sha256,
        random: RandomFill,
        read: R,
    ) -> Result<Self, InvalidUiFolder> {
        // The folder is joined to request paths, so `ui` would also reach `uix.js`.
        if !is_ui_folder(ui_folder) {
            return Err(InvalidUiFolder(ui_folder.into()));
        }
        let files = files
            .into_iter()
            .map(|file| (file.path.clone(), file))
            .collect();
        Ok(Self {
            files,
            ui_folder: ui_folder.into(),
            sha256,
            random,
            read,
        })
    }

    /// Answer a request for `request_path`, the URL's path on the view's origin.
    pub fn resolve(&self, request_path: &str) -> Result<Served, Missing> {
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

    /// The media type, `nosniff` and the policy with a fresh nonce. Every response carries
    /// the policy, since a worker's own response headers set the policy it runs under.
    fn headers(&self, relative: &str) -> Result<Vec<(&'static str, String)>, Missing> {
        let nonce = Nonce::generate(self.random).map_err(Missing::NonceFailed)?;
        Ok(vec![
            ("Content-Type", media_type(relative).to_string()),
            ("X-Content-Type-Options", "nosniff".to_string()),
            ("Content-Security-Policy", content_security_policy(&nonce)),
        ])
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

fn media_type(path: &str) -> &'static str {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let extension = file_name.rsplit_once('.').map(|(_, extension)| extension);
    MEDIA_TYPES
        .iter()
        .find(|(known, _)| Some(*known) == extension)
        .map_or(UNKNOWN_MEDIA_TYPE, |(_, media_type)| media_type)
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;

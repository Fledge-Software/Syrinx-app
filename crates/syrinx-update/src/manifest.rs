//! The signed manifest: one canonical text form, parsed strictly and bounded.

use std::collections::BTreeSet;

use crate::error::{ManifestProblem, UpdateError, manifest_error};
use crate::path::PackagePath;
use crate::version::{Version, parse_canonical_decimal};

/// Largest manifest accepted, in bytes: room for `MAX_FILES` lines of the longest paths.
pub const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
/// Most files one package may list.
pub const MAX_FILES: usize = 4096;
/// Largest single payload file, in bytes.
pub const MAX_FILE_BYTES: u64 = 1 << 30;
/// Largest total of all listed payload sizes, in bytes.
pub const MAX_TOTAL_BYTES: u64 = 4 << 30;

const HEADER: &str = "syrinx-update-manifest 1";
const VERSION_FIELD: &str = "version ";
const MINIMUM_FIELD: &str = "minimum-version ";
const FILE_FIELD: &str = "file ";
const HASH_HEX_DIGITS: usize = 64;

/// One payload file's path, exact size and SHA-256 hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    path: PackagePath,
    size: u64,
    sha256: [u8; 32],
}

impl FileEntry {
    pub fn new(path: PackagePath, size: u64, sha256: [u8; 32]) -> Self {
        Self { path, size, sha256 }
    }

    pub fn path(&self) -> &PackagePath {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// A parsed manifest whose bytes are known to be canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    version: Version,
    minimum_version: Version,
    files: Vec<FileEntry>,
}

impl Manifest {
    /// Builds a manifest for a release tool or test, sorting files and applying every parse rule.
    pub fn new(
        version: Version,
        minimum_version: Version,
        mut files: Vec<FileEntry>,
    ) -> Result<Self, UpdateError> {
        files.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
        let candidate = Self {
            version,
            minimum_version,
            files,
        };
        Self::parse(&candidate.to_bytes())
    }

    /// Parses canonical manifest bytes, refusing anything else.
    pub fn parse(bytes: &[u8]) -> Result<Self, UpdateError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(UpdateError::TooLarge {
                what: "manifest",
                limit: MAX_MANIFEST_BYTES as u64,
            });
        }
        let body = manifest_body(bytes)?;
        let mut lines = body
            .split('\n')
            .enumerate()
            .map(|(index, line)| (index + 1, line));

        // Read the header and the two version fields in their fixed order.
        let (_, header) = lines
            .next()
            .ok_or(manifest_error(1, ManifestProblem::Header))?;
        if header != HEADER {
            return Err(manifest_error(1, ManifestProblem::Header));
        }
        let version = parse_version_field(lines.next(), 2, VERSION_FIELD, "version")?;
        let minimum_version =
            parse_version_field(lines.next(), 3, MINIMUM_FIELD, "minimum-version")?;
        if minimum_version > version {
            return Err(manifest_error(3, ManifestProblem::MinimumAboveVersion));
        }

        let files = parse_file_lines(lines)?;
        let manifest = Self {
            version,
            minimum_version,
            files,
        };
        if manifest.to_bytes() != bytes {
            return Err(manifest_error(0, ManifestProblem::NonCanonical));
        }
        Ok(manifest)
    }

    /// The unique canonical rendering, which is what gets signed.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut text = format!(
            "{HEADER}\n{VERSION_FIELD}{}\n{MINIMUM_FIELD}{}\n",
            self.version, self.minimum_version
        );
        for file in &self.files {
            text.push_str(FILE_FIELD);
            push_hex(&mut text, &file.sha256);
            text.push_str(&format!(" {} {}\n", file.size, file.path.as_str()));
        }
        text.into_bytes()
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub fn minimum_version(&self) -> Version {
        self.minimum_version
    }

    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }
}

/// Decode the text and strip its single required final newline.
fn manifest_body(bytes: &[u8]) -> Result<&str, UpdateError> {
    text_body(bytes).map_err(|fault| {
        let problem = match fault {
            TextFault::Encoding => ManifestProblem::Encoding,
            TextFault::MissingFinalNewline => ManifestProblem::MissingFinalNewline,
        };
        manifest_error(0, problem)
    })
}

/// Why signed text failed before its lines were read.
pub(crate) enum TextFault {
    /// Not UTF-8, or holds a carriage return or NUL.
    Encoding,
    /// The text does not end with exactly one newline.
    MissingFinalNewline,
}

/// Decode signed LF text and strip its single required final newline.
pub(crate) fn text_body(bytes: &[u8]) -> Result<&str, TextFault> {
    let text = std::str::from_utf8(bytes).map_err(|_| TextFault::Encoding)?;
    if text.contains(['\r', '\0']) {
        return Err(TextFault::Encoding);
    }
    text.strip_suffix('\n')
        .ok_or(TextFault::MissingFinalNewline)
}

/// Read one `name value` version line at its expected position.
fn parse_version_field(
    line: Option<(usize, &str)>,
    number: usize,
    prefix: &str,
    field: &'static str,
) -> Result<Version, UpdateError> {
    let value = line
        .and_then(|(_, text)| text.strip_prefix(prefix))
        .ok_or(manifest_error(number, ManifestProblem::Field(field)))?;
    Version::parse(value).ok_or(manifest_error(number, ManifestProblem::Version))
}

/// Read every remaining line as a sorted, unique, bounded file entry.
fn parse_file_lines<'a>(
    lines: impl Iterator<Item = (usize, &'a str)>,
) -> Result<Vec<FileEntry>, UpdateError> {
    let mut files: Vec<FileEntry> = Vec::new();
    let mut folded = FoldedNames::default();
    let mut total: u64 = 0;
    let mut last_line = 3;
    for (number, line) in lines {
        last_line = number;
        if files.len() == MAX_FILES {
            return Err(manifest_error(number, ManifestProblem::TooManyFiles));
        }
        let entry = parse_file_line(line).map_err(|problem| manifest_error(number, problem))?;
        check_order(files.last(), &entry).map_err(|problem| manifest_error(number, problem))?;
        folded
            .insert(&entry.path)
            .map_err(|problem| manifest_error(number, problem))?;
        total = total
            .checked_add(entry.size)
            .filter(|sum| *sum <= MAX_TOTAL_BYTES)
            .ok_or(manifest_error(number, ManifestProblem::TotalTooLarge))?;
        files.push(entry);
    }
    if files.is_empty() {
        return Err(manifest_error(last_line, ManifestProblem::NoFiles));
    }
    Ok(files)
}

/// File and directory names as Windows compares them, with ASCII letter case folded.
#[derive(Default)]
struct FoldedNames {
    files: BTreeSet<String>,
    directories: BTreeSet<String>,
}

impl FoldedNames {
    /// Refuse a path that repeats a file or makes a file double as a directory.
    fn insert(&mut self, path: &PackagePath) -> Result<(), ManifestProblem> {
        let folded = path.folded();
        if self.files.contains(&folded) {
            return Err(ManifestProblem::DuplicatePath);
        }
        let ancestors: Vec<&str> = folded
            .match_indices('/')
            .map(|(index, _)| &folded[..index])
            .collect();
        let file_as_directory = ancestors
            .iter()
            .any(|ancestor| self.files.contains(*ancestor));
        if file_as_directory || self.directories.contains(&folded) {
            return Err(ManifestProblem::PathCollision);
        }
        self.directories
            .extend(ancestors.into_iter().map(str::to_owned));
        self.files.insert(folded);
        Ok(())
    }
}

/// Split `file <hash> <size> <path>` and validate each field.
fn parse_file_line(line: &str) -> Result<FileEntry, ManifestProblem> {
    let fields = line
        .strip_prefix(FILE_FIELD)
        .ok_or(ManifestProblem::UnexpectedLine)?;
    let mut parts = fields.splitn(3, ' ');
    let (Some(hash), Some(size), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ManifestProblem::UnexpectedLine);
    };
    let sha256 = parse_hash(hash).ok_or(ManifestProblem::Hash)?;
    let size = parse_canonical_decimal(size)
        .filter(|size| *size <= MAX_FILE_BYTES)
        .ok_or(ManifestProblem::Size)?;
    let path = PackagePath::parse(path).map_err(ManifestProblem::Path)?;
    Ok(FileEntry::new(path, size, sha256))
}

/// Require strictly ascending byte order, naming an exact repeat as a duplicate.
fn check_order(previous: Option<&FileEntry>, entry: &FileEntry) -> Result<(), ManifestProblem> {
    let Some(previous) = previous else {
        return Ok(());
    };
    match previous.path.as_str().cmp(entry.path.as_str()) {
        std::cmp::Ordering::Less => Ok(()),
        std::cmp::Ordering::Equal => Err(ManifestProblem::DuplicatePath),
        std::cmp::Ordering::Greater => Err(ManifestProblem::Unsorted),
    }
}

/// Decode exactly 64 lowercase hex digits.
pub(crate) fn parse_hash(text: &str) -> Option<[u8; 32]> {
    if text.len() != HASH_HEX_DIGITS {
        return None;
    }
    let mut hash = [0u8; 32];
    for (byte, pair) in hash.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *byte = (hex_value(pair[0])? << 4) | hex_value(pair[1])?;
    }
    Some(hash)
}

fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}

/// Append lowercase hex digits.
pub(crate) fn push_hex(text: &mut String, bytes: &[u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
}

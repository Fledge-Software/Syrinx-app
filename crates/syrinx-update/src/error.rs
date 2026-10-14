//! Typed refusals, so callers and tests can tell every rejected update apart.

use std::fmt;
use std::io;

use crate::time::Timestamp;
use crate::version::Version;

/// Why a package, a latest statement, a signature or a key was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// An input exceeded its fixed bound before it was fully read.
    TooLarge { what: &'static str, limit: u64 },
    /// The detached signature was not exactly the expected length.
    SignatureLength { actual: usize },
    /// The detached signature did not start with the expected magic.
    SignatureFormat,
    /// The signature names a key that is not pinned in this executable.
    UnknownKey,
    /// The signature does not verify against the named pinned key.
    BadSignature,
    /// A pinned public key is not a valid P-256 point; CNG's status is kept.
    InvalidKey { status: i32 },
    /// Two pinned keys share an identifier.
    DuplicateKey,
    /// Windows CNG reported a failure other than a bad signature.
    Crypto {
        operation: &'static str,
        status: i32,
    },
    /// The signed manifest is malformed or not in canonical form.
    Manifest {
        line: usize,
        problem: ManifestProblem,
    },
    /// The package is not newer than the installed version (rollback).
    NotNewer {
        installed: Version,
        offered: Version,
    },
    /// The installed version is older than the package allows updating from.
    InstalledTooOld {
        installed: Version,
        minimum: Version,
    },
    /// The payload holds a file the manifest does not list.
    UnlistedFile { name: String },
    /// A listed file is absent from the payload.
    MissingFile { path: String },
    /// A listed file's length differs from the manifest.
    SizeMismatch { path: String, expected: u64 },
    /// A listed file's SHA-256 hash differs from the manifest.
    HashMismatch { path: String },
    /// A payload entry is a link, junction, other reparse point or non-Unicode name.
    UnsafeEntry { name: String },
    /// Reading part of the package failed.
    Read { what: String, kind: io::ErrorKind },
    /// The signed latest statement is malformed or not in canonical form.
    Statement {
        line: usize,
        problem: StatementProblem,
    },
    /// The statement's expiry is not after the local clock.
    StatementExpired { expires: Timestamp, now: Timestamp },
    /// The statement claims an issue time more than a day ahead of the local clock.
    StatementIssuedAhead { issued: Timestamp, now: Timestamp },
    /// The statement's serial is lower than one accepted before (a replay).
    StatementReplayed { accepted: u64, offered: u64 },
    /// The statement reuses an accepted serial with different bytes.
    StatementSerialReused { serial: u64 },
    /// The manifest's SHA-256 differs from the one the statement names.
    ManifestNotStated,
    /// The manifest installs a different version from the one the statement names.
    StatementVersionMismatch { stated: Version, manifest: Version },
    /// The local clock is outside the range a statement can spell (1970 to 9999).
    Clock,
    /// A signing key is not an ECDSA P-256 key, or its stored public half is malformed.
    UnsuitableSigningKey,
}

/// What is wrong with a latest-statement line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementProblem {
    /// Not UTF-8, or holds a carriage return or NUL.
    Encoding,
    /// The text does not end with exactly one newline.
    MissingFinalNewline,
    /// The first line is not the supported format header.
    Header,
    /// A required field line is missing or misnamed.
    Field(&'static str),
    /// The serial is not canonical decimal within 64 bits.
    Serial,
    /// The version is not canonical `MAJOR.MINOR.PATCH`.
    Version,
    /// The manifest hash is not 64 lowercase hex digits.
    Hash,
    /// A time is not canonical `YYYY-MM-DDTHH:MM:SSZ` between 1970 and 9999.
    Timestamp,
    /// `expires` is not after `issued`, or is more than 60 days after it.
    Lifetime,
    /// A line follows `expires`.
    ExtraLine,
    /// The text parses but is not the unique canonical rendering.
    NonCanonical,
}

/// What is wrong with a manifest line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestProblem {
    /// Not UTF-8, or holds a carriage return or NUL.
    Encoding,
    /// The text does not end with exactly one newline.
    MissingFinalNewline,
    /// The first line is not the supported format header.
    Header,
    /// A required field line is missing or misnamed.
    Field(&'static str),
    /// A version is not canonical `MAJOR.MINOR.PATCH`.
    Version,
    /// `minimum-version` is greater than `version`.
    MinimumAboveVersion,
    /// A line after the header fields is not a `file` line.
    UnexpectedLine,
    /// A hash is not 64 lowercase hex digits.
    Hash,
    /// A size is not canonical decimal or exceeds the per-file bound.
    Size,
    /// A file path is unsafe or malformed.
    Path(PathProblem),
    /// Two paths are equal, or equal ignoring ASCII letter case.
    DuplicatePath,
    /// A file path is also, ignoring ASCII letter case, a directory of another file.
    PathCollision,
    /// File lines are not in ascending byte order of path.
    Unsorted,
    /// The manifest lists no files.
    NoFiles,
    /// The manifest lists more files than the bound allows.
    TooManyFiles,
    /// The listed sizes add up to more than the package bound.
    TotalTooLarge,
    /// The text parses but is not the unique canonical rendering.
    NonCanonical,
}

/// Why a payload path is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathProblem {
    /// The path or one of its components is empty (including leading `/`).
    EmptyComponent,
    /// The path exceeds the total length bound.
    TooLong,
    /// The path has more components than allowed.
    TooDeep,
    /// A component exceeds the per-component length bound.
    ComponentTooLong,
    /// A character outside ASCII letters, digits, `.`, `_` and `-`.
    Character,
    /// A `.` or `..` component.
    DotComponent,
    /// A component ending in `.`, which Windows strips.
    TrailingDot,
    /// A reserved Windows device name such as `CON` or `LPT1`.
    DeviceName,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { what, limit } => write!(formatter, "{what} exceeds {limit} bytes"),
            Self::SignatureLength { actual } => {
                write!(formatter, "signature has {actual} bytes")
            }
            Self::SignatureFormat => formatter.write_str("signature has an unknown format"),
            Self::UnknownKey => formatter.write_str("signature names no pinned key"),
            Self::BadSignature => formatter.write_str("signature does not verify"),
            Self::InvalidKey { status } => {
                write!(
                    formatter,
                    "pinned key is not a valid P-256 point ({status:#010x})"
                )
            }
            Self::DuplicateKey => formatter.write_str("two pinned keys share an identifier"),
            Self::Crypto { operation, status } => {
                write!(formatter, "{operation} failed with status {status:#010x}")
            }
            Self::Manifest { line, problem } => {
                write!(formatter, "manifest line {line}: {problem:?}")
            }
            Self::NotNewer { installed, offered } => {
                write!(formatter, "package {offered} is not newer than {installed}")
            }
            Self::InstalledTooOld { installed, minimum } => {
                write!(formatter, "installed {installed} is older than {minimum}")
            }
            Self::UnlistedFile { name } => write!(formatter, "unlisted file {name}"),
            Self::MissingFile { path } => write!(formatter, "missing file {path}"),
            Self::SizeMismatch { path, expected } => {
                write!(formatter, "{path} is not {expected} bytes")
            }
            Self::HashMismatch { path } => write!(formatter, "{path} has the wrong hash"),
            Self::UnsafeEntry { name } => write!(formatter, "unsafe payload entry {name}"),
            Self::Read { what, kind } => write!(formatter, "reading {what} failed: {kind}"),
            Self::Statement { line, problem } => {
                write!(formatter, "latest statement line {line}: {problem:?}")
            }
            Self::StatementExpired { expires, now } => {
                write!(
                    formatter,
                    "latest statement expired at {expires} (now {now})"
                )
            }
            Self::StatementIssuedAhead { issued, now } => {
                write!(
                    formatter,
                    "latest statement issued at {issued}, ahead of {now}"
                )
            }
            Self::StatementReplayed { accepted, offered } => {
                write!(
                    formatter,
                    "latest statement serial {offered} is below accepted {accepted}"
                )
            }
            Self::StatementSerialReused { serial } => {
                write!(
                    formatter,
                    "latest statement serial {serial} was accepted with other content"
                )
            }
            Self::ManifestNotStated => {
                formatter.write_str("manifest does not match the latest statement")
            }
            Self::StatementVersionMismatch { stated, manifest } => {
                write!(
                    formatter,
                    "latest statement names {stated} but the manifest installs {manifest}"
                )
            }
            Self::Clock => formatter.write_str("the local clock is outside 1970 to 9999"),
            Self::UnsuitableSigningKey => {
                formatter.write_str("signing key is not an ECDSA P-256 key")
            }
        }
    }
}

impl std::error::Error for UpdateError {}

/// Builds a latest-statement refusal for a one-based line number.
pub(crate) fn statement_error(line: usize, problem: StatementProblem) -> UpdateError {
    UpdateError::Statement { line, problem }
}

/// Builds a manifest refusal for a one-based line number.
pub(crate) fn manifest_error(line: usize, problem: ManifestProblem) -> UpdateError {
    UpdateError::Manifest { line, problem }
}

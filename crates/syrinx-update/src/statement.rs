//! The latest-version statement's one canonical text form, parsed strictly and bounded (P1-39a).

use crate::error::{StatementProblem, UpdateError, statement_error};
use crate::manifest::{TextFault, parse_hash, push_hex, text_body};
use crate::time::{SECONDS_PER_DAY, Timestamp};
use crate::version::{Version, parse_canonical_decimal};

/// Largest statement accepted, in bytes.
pub const MAX_STATEMENT_BYTES: usize = 4 * 1024;
/// Longest a statement may stay valid: `expires` at most 60 days after `issued`.
pub const MAX_STATEMENT_LIFETIME_SECONDS: u64 = 60 * SECONDS_PER_DAY;

const HEADER: &str = "syrinx-update-latest 1";
const SERIAL_FIELD: &str = "serial ";
const VERSION_FIELD: &str = "version ";
const HASH_FIELD: &str = "manifest-sha256 ";
const ISSUED_FIELD: &str = "issued ";
const EXPIRES_FIELD: &str = "expires ";

/// "The newest version is X, its manifest hashes to H, and this expires on D."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    serial: u64,
    version: Version,
    manifest_sha256: [u8; 32],
    issued: Timestamp,
    expires: Timestamp,
}

impl Statement {
    /// Builds a statement for the release tool or a test, applying every parse rule.
    pub fn new(
        serial: u64,
        version: Version,
        manifest_sha256: [u8; 32],
        issued: Timestamp,
        expires: Timestamp,
    ) -> Result<Self, UpdateError> {
        let candidate = Self {
            serial,
            version,
            manifest_sha256,
            issued,
            expires,
        };
        Self::parse(&candidate.to_bytes())
    }

    /// Parses canonical statement bytes, refusing anything else.
    pub fn parse(bytes: &[u8]) -> Result<Self, UpdateError> {
        if bytes.len() > MAX_STATEMENT_BYTES {
            return Err(UpdateError::TooLarge {
                what: "latest statement",
                limit: MAX_STATEMENT_BYTES as u64,
            });
        }
        let mut lines = statement_body(bytes)?.split('\n');

        // Read the header and the five fields in their fixed order.
        if lines.next() != Some(HEADER) {
            return Err(statement_error(1, StatementProblem::Header));
        }
        let serial = field(lines.next(), 2, SERIAL_FIELD, "serial").and_then(|text| {
            parse_canonical_decimal(text).ok_or(statement_error(2, StatementProblem::Serial))
        })?;
        let version = field(lines.next(), 3, VERSION_FIELD, "version").and_then(|text| {
            Version::parse(text).ok_or(statement_error(3, StatementProblem::Version))
        })?;
        let manifest_sha256 = field(lines.next(), 4, HASH_FIELD, "manifest-sha256")
            .and_then(|text| parse_hash(text).ok_or(statement_error(4, StatementProblem::Hash)))?;
        let issued = timestamp_field(lines.next(), 5, ISSUED_FIELD, "issued")?;
        let expires = timestamp_field(lines.next(), 6, EXPIRES_FIELD, "expires")?;
        if lines.next().is_some() {
            return Err(statement_error(7, StatementProblem::ExtraLine));
        }
        check_lifetime(issued, expires)?;

        let statement = Self {
            serial,
            version,
            manifest_sha256,
            issued,
            expires,
        };
        if statement.to_bytes() != bytes {
            return Err(statement_error(0, StatementProblem::NonCanonical));
        }
        Ok(statement)
    }

    /// The unique canonical rendering, which is what gets signed.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut text = format!(
            "{HEADER}\n{SERIAL_FIELD}{}\n{VERSION_FIELD}{}\n{HASH_FIELD}",
            self.serial, self.version
        );
        push_hex(&mut text, &self.manifest_sha256);
        text.push_str(&format!(
            "\n{ISSUED_FIELD}{}\n{EXPIRES_FIELD}{}\n",
            self.issued, self.expires
        ));
        text.into_bytes()
    }

    /// Increases with every statement the owner signs; replays carry a lower one.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// The newest version the owner has released.
    pub fn version(&self) -> Version {
        self.version
    }

    /// SHA-256 of that version's exact `manifest` bytes.
    pub fn manifest_sha256(&self) -> &[u8; 32] {
        &self.manifest_sha256
    }

    pub fn issued(&self) -> Timestamp {
        self.issued
    }

    pub fn expires(&self) -> Timestamp {
        self.expires
    }
}

/// Decode the text and strip its single required final newline.
fn statement_body(bytes: &[u8]) -> Result<&str, UpdateError> {
    text_body(bytes).map_err(|fault| {
        let problem = match fault {
            TextFault::Encoding => StatementProblem::Encoding,
            TextFault::MissingFinalNewline => StatementProblem::MissingFinalNewline,
        };
        statement_error(0, problem)
    })
}

/// The value of one `name value` line at its expected position.
fn field<'a>(
    line: Option<&'a str>,
    number: usize,
    prefix: &str,
    name: &'static str,
) -> Result<&'a str, UpdateError> {
    line.and_then(|text| text.strip_prefix(prefix))
        .ok_or(statement_error(number, StatementProblem::Field(name)))
}

fn timestamp_field(
    line: Option<&str>,
    number: usize,
    prefix: &str,
    name: &'static str,
) -> Result<Timestamp, UpdateError> {
    let text = field(line, number, prefix, name)?;
    Timestamp::parse(text).ok_or(statement_error(number, StatementProblem::Timestamp))
}

/// Require `issued < expires <= issued + 60 days`.
fn check_lifetime(issued: Timestamp, expires: Timestamp) -> Result<(), UpdateError> {
    let lifetime = expires
        .unix_seconds()
        .checked_sub(issued.unix_seconds())
        .filter(|seconds| (1..=MAX_STATEMENT_LIFETIME_SECONDS).contains(seconds));
    if lifetime.is_none() {
        return Err(statement_error(6, StatementProblem::Lifetime));
    }
    Ok(())
}

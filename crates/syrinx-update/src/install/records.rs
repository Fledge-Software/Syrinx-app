//! The small local records an update keeps: `pending`, `state` and `outcome`.
//!
//! Each is LF text with a fixed header and fixed fields, rewritten whole and read strictly.
//! They are not signed: the helper rechecks the signed manifest before it trusts `pending`.

use std::fmt;

use super::files::{read_small, remove_file_if_present, write_atomically};
use super::{InstallError, UpdateLayout};
use crate::latest::AcceptedStatement;
use crate::manifest::{parse_hash, push_hex};
use crate::time::Timestamp;
use crate::version::{Version, parse_canonical_decimal};

/// Largest record read back; each is a few lines.
const RECORD_LIMIT: usize = 1024;
/// The value an absent optional field is written as.
const NONE: &str = "none";

/// What an update last did, as the fixed categories the proposal's diagnostics use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateResult {
    UpToDate,
    /// A check found a newer version and is waiting for the owner's yes (P1-39c).
    Available,
    Staged,
    Installed,
    StatementExpired,
    VerificationFailed,
    StagingFailed,
    /// A download from the update host failed (P1-39c).
    NetworkFailed,
    /// The owner cancelled a download (P1-39c).
    Cancelled,
    ApplyFailed,
    ApplyRolledBack,
    /// The new version did not start and the previous one could not be put back.
    RollbackFailed,
}

const RESULTS: [(UpdateResult, &str); 12] = [
    (UpdateResult::UpToDate, "up_to_date"),
    (UpdateResult::Available, "available"),
    (UpdateResult::Staged, "staged"),
    (UpdateResult::Installed, "installed"),
    (UpdateResult::StatementExpired, "statement_expired"),
    (UpdateResult::VerificationFailed, "verification_failed"),
    (UpdateResult::StagingFailed, "staging_failed"),
    (UpdateResult::NetworkFailed, "network_failed"),
    (UpdateResult::Cancelled, "cancelled"),
    (UpdateResult::ApplyFailed, "apply_failed"),
    (UpdateResult::ApplyRolledBack, "apply_rolled_back"),
    (UpdateResult::RollbackFailed, "apply_rollback_failed"),
];

impl UpdateResult {
    pub fn as_str(self) -> &'static str {
        RESULTS
            .iter()
            .find(|(result, _)| *result == self)
            .map_or("unknown", |(_, name)| name)
    }

    fn parse(text: &str) -> Option<Self> {
        RESULTS
            .iter()
            .find(|(_, name)| *name == text)
            .map(|(result, _)| *result)
    }
}

impl fmt::Display for UpdateResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A staged copy that is complete: written last, after `Syrinx.new` and the helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub version: Version,
    /// The version that staged it, which the helper's rollback check compares against.
    pub from: Version,
    pub manifest_sha256: [u8; 32],
}

const PENDING_HEADER: &str = "syrinx-update-pending 1";
const PENDING_FIELDS: [&str; 3] = ["version", "from", "manifest-sha256"];

impl Pending {
    pub fn read(layout: &UpdateLayout) -> Result<Option<Self>, InstallError> {
        const WHAT: &str = "pending update";
        let Some(bytes) = read_small(&layout.pending(), RECORD_LIMIT, WHAT)? else {
            return Ok(None);
        };
        let malformed = InstallError::Record { what: WHAT };
        let values = parse(&bytes, PENDING_HEADER, &PENDING_FIELDS).ok_or(malformed.clone())?;
        Ok(Some(Self {
            version: Version::parse(values[0]).ok_or(malformed.clone())?,
            from: Version::parse(values[1]).ok_or(malformed.clone())?,
            manifest_sha256: parse_hash(values[2]).ok_or(malformed)?,
        }))
    }

    pub fn write(&self, layout: &UpdateLayout) -> Result<(), InstallError> {
        let mut hash = String::new();
        push_hex(&mut hash, &self.manifest_sha256);
        let values = [self.version.to_string(), self.from.to_string(), hash];
        let bytes = render(PENDING_HEADER, &PENDING_FIELDS, &values);
        write_atomically(&layout.pending(), &bytes, "recording the staged update")
    }
}

/// The last statement accepted, so a replay is refused, and the last result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateState {
    pub accepted: Option<AcceptedRecord>,
    pub result: Option<UpdateResult>,
    /// The version the result concerns, such as the staged one.
    pub version: Option<Version>,
}

/// An accepted statement's serial and hash, with its expiry for Settings to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcceptedRecord {
    pub statement: AcceptedStatement,
    pub expires: Timestamp,
}

const STATE_HEADER: &str = "syrinx-update-state 1";
const STATE_FIELDS: [&str; 5] = ["serial", "statement-sha256", "expires", "result", "version"];

impl UpdateState {
    /// The stored state; an unreadable record is refused, so a lost serial never passes as new.
    pub fn read(layout: &UpdateLayout) -> Result<Self, InstallError> {
        const WHAT: &str = "update state";
        let Some(bytes) = read_small(&layout.state(), RECORD_LIMIT, WHAT)? else {
            return Ok(Self::default());
        };
        let malformed = InstallError::Record { what: WHAT };
        let values = parse(&bytes, STATE_HEADER, &STATE_FIELDS).ok_or(malformed.clone())?;
        Ok(Self {
            accepted: accepted_fields(values[0], values[1], values[2]).ok_or(malformed.clone())?,
            result: optional(values[3], UpdateResult::parse).ok_or(malformed.clone())?,
            version: optional(values[4], Version::parse).ok_or(malformed)?,
        })
    }

    pub fn write(&self, layout: &UpdateLayout) -> Result<(), InstallError> {
        let (serial, hash, expires) = match &self.accepted {
            Some(record) => {
                let mut hash = String::new();
                push_hex(&mut hash, record.statement.sha256());
                (
                    record.statement.serial().to_string(),
                    hash,
                    record.expires.to_string(),
                )
            }
            None => (NONE.to_owned(), NONE.to_owned(), NONE.to_owned()),
        };
        let result = self
            .result
            .map_or(NONE.to_owned(), |result| result.as_str().to_owned());
        let version = self
            .version
            .map_or(NONE.to_owned(), |version| version.to_string());
        let values = [serial, hash, expires, result, version];
        let bytes = render(STATE_HEADER, &STATE_FIELDS, &values);
        write_atomically(&layout.state(), &bytes, "recording the update state")
    }

    /// The statement record `verify_statement` compares a new one with.
    pub fn previous(&self) -> Option<AcceptedStatement> {
        self.accepted.map(|record| record.statement)
    }
}

/// Serial, hash and expiry are all present or all `none`.
fn accepted_fields(serial: &str, hash: &str, expires: &str) -> Option<Option<AcceptedRecord>> {
    if [serial, hash, expires] == [NONE; 3] {
        return Some(None);
    }
    let statement = AcceptedStatement::new(parse_canonical_decimal(serial)?, parse_hash(hash)?);
    Some(Some(AcceptedRecord {
        statement,
        expires: Timestamp::parse(expires)?,
    }))
}

/// `none`, or a value `parse` accepts.
fn optional<T>(text: &str, parse: impl Fn(&str) -> Option<T>) -> Option<Option<T>> {
    if text == NONE {
        return Some(None);
    }
    parse(text).map(Some)
}

/// What the helper did, shown once by the next Syrinx to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub result: UpdateResult,
    /// The version the update tried to install.
    pub attempted: Version,
    /// The version installed afterwards.
    pub current: Version,
}

const OUTCOME_HEADER: &str = "syrinx-update-outcome 1";
const OUTCOME_FIELDS: [&str; 3] = ["result", "attempted", "current"];

impl Outcome {
    pub fn write(&self, layout: &UpdateLayout) -> Result<(), InstallError> {
        let values = [
            self.result.as_str().to_owned(),
            self.attempted.to_string(),
            self.current.to_string(),
        ];
        let bytes = render(OUTCOME_HEADER, &OUTCOME_FIELDS, &values);
        write_atomically(&layout.outcome(), &bytes, "recording the update outcome")
    }

    /// Read and remove the outcome, so it is told once.
    pub fn take(layout: &UpdateLayout) -> Result<Option<Self>, InstallError> {
        const WHAT: &str = "update outcome";
        let Some(bytes) = read_small(&layout.outcome(), RECORD_LIMIT, WHAT)? else {
            return Ok(None);
        };
        remove_file_if_present(&layout.outcome(), "removing the update outcome")?;
        let malformed = InstallError::Record { what: WHAT };
        let values = parse(&bytes, OUTCOME_HEADER, &OUTCOME_FIELDS).ok_or(malformed.clone())?;
        Ok(Some(Self {
            result: UpdateResult::parse(values[0]).ok_or(malformed.clone())?,
            attempted: Version::parse(values[1]).ok_or(malformed.clone())?,
            current: Version::parse(values[2]).ok_or(malformed)?,
        }))
    }
}

/// The header, then one `name value` line per field, each ending in LF.
fn render(header: &str, names: &[&str], values: &[String]) -> Vec<u8> {
    let mut text = format!("{header}\n");
    for (name, value) in names.iter().zip(values) {
        text.push_str(&format!("{name} {value}\n"));
    }
    text.into_bytes()
}

/// The field values, in order, when `bytes` is exactly a rendering of `names`.
fn parse<'a>(bytes: &'a [u8], header: &str, names: &[&str]) -> Option<Vec<&'a str>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.strip_suffix('\n')?.split('\n');
    if lines.next()? != header {
        return None;
    }
    let mut values = Vec::with_capacity(names.len());
    for name in names {
        let value = lines.next()?.strip_prefix(name)?.strip_prefix(' ')?;
        if value.is_empty() || value.contains([' ', '\r']) {
            return None;
        }
        values.push(value);
    }
    lines.next().is_none().then_some(values)
}

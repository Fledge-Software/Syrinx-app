//! Accepting a signed latest statement: signature, expiry, serial, then the manifest it names.

use crate::cng;
use crate::error::UpdateError;
use crate::keys::{KeyId, KeySet};
use crate::manifest::MAX_MANIFEST_BYTES;
use crate::signature::{DetachedSignature, SignatureKind};
use crate::statement::{MAX_STATEMENT_BYTES, Statement};
use crate::time::{SECONDS_PER_DAY, Timestamp};
use crate::version::Version;
use crate::{VerifiedManifest, check_signature, verify_manifest};

/// How far ahead of the local clock a statement's `issued` time may be.
pub const MAX_ISSUED_AHEAD_SECONDS: u64 = SECONDS_PER_DAY;

/// The last statement Syrinx accepted: what P1-39 keeps to refuse replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptedStatement {
    serial: u64,
    sha256: [u8; 32],
}

impl AcceptedStatement {
    /// Rebuild the record from stored state; `sha256` is over the accepted statement's bytes.
    pub fn new(serial: u64, sha256: [u8; 32]) -> Self {
        Self { serial, sha256 }
    }

    pub fn serial(&self) -> u64 {
        self.serial
    }

    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// A statement whose signature, form, freshness and serial have all been accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedStatement {
    statement: Statement,
    key_id: KeyId,
    sha256: [u8; 32],
}

impl VerifiedStatement {
    pub fn statement(&self) -> &Statement {
        &self.statement
    }

    /// Which pinned key signed this statement.
    pub fn key_id(&self) -> KeyId {
        self.key_id
    }

    /// The record to store once this statement is acted on.
    pub fn accepted(&self) -> AcceptedStatement {
        AcceptedStatement::new(self.statement.serial(), self.sha256)
    }

    /// Verify the manifest this statement names: its hash first, then P1-38's checks.
    pub fn verify_manifest(
        &self,
        manifest_bytes: &[u8],
        signature_bytes: &[u8],
        keys: &KeySet,
        installed: Version,
    ) -> Result<VerifiedManifest, UpdateError> {
        if manifest_bytes.len() > MAX_MANIFEST_BYTES {
            return Err(UpdateError::TooLarge {
                what: "manifest",
                limit: MAX_MANIFEST_BYTES as u64,
            });
        }
        if cng::sha256(manifest_bytes)? != *self.statement.manifest_sha256() {
            return Err(UpdateError::ManifestNotStated);
        }
        let verified = verify_manifest(manifest_bytes, signature_bytes, keys, installed)?;
        let manifest = verified.manifest().version();
        if manifest != self.statement.version() {
            return Err(UpdateError::StatementVersionMismatch {
                stated: self.statement.version(),
                manifest,
            });
        }
        Ok(verified)
    }
}

/// Verify a statement's signature before parsing it, then its expiry and serial.
///
/// `now` is the local clock; `previous` is the last statement accepted, if any.
pub fn verify_statement(
    statement_bytes: &[u8],
    signature_bytes: &[u8],
    keys: &KeySet,
    now: Timestamp,
    previous: Option<&AcceptedStatement>,
) -> Result<VerifiedStatement, UpdateError> {
    if statement_bytes.len() > MAX_STATEMENT_BYTES {
        return Err(UpdateError::TooLarge {
            what: "latest statement",
            limit: MAX_STATEMENT_BYTES as u64,
        });
    }
    let signature = DetachedSignature::parse(signature_bytes, SignatureKind::Statement)?;
    let key = keys
        .find(signature.key_id())
        .ok_or(UpdateError::UnknownKey)?;
    check_signature(key, statement_bytes, &signature)?;

    // Only signed bytes reach the parser.
    let statement = Statement::parse(statement_bytes)?;
    check_freshness(&statement, now)?;
    let sha256 = cng::sha256(statement_bytes)?;
    check_serial(&statement, &sha256, previous)?;
    Ok(VerifiedStatement {
        statement,
        key_id: signature.key_id(),
        sha256,
    })
}

/// Refuse an expired statement, and one issued more than a day ahead of the clock.
fn check_freshness(statement: &Statement, now: Timestamp) -> Result<(), UpdateError> {
    if statement.expires() <= now {
        return Err(UpdateError::StatementExpired {
            expires: statement.expires(),
            now,
        });
    }
    let ahead = statement
        .issued()
        .unix_seconds()
        .saturating_sub(now.unix_seconds());
    if ahead > MAX_ISSUED_AHEAD_SECONDS {
        return Err(UpdateError::StatementIssuedAhead {
            issued: statement.issued(),
            now,
        });
    }
    Ok(())
}

/// Refuse a lower serial, and an equal serial whose bytes differ from the accepted ones.
fn check_serial(
    statement: &Statement,
    sha256: &[u8; 32],
    previous: Option<&AcceptedStatement>,
) -> Result<(), UpdateError> {
    let Some(previous) = previous else {
        return Ok(());
    };
    let serial = statement.serial();
    if serial < previous.serial() {
        return Err(UpdateError::StatementReplayed {
            accepted: previous.serial(),
            offered: serial,
        });
    }
    if serial == previous.serial() && sha256 != previous.sha256() {
        return Err(UpdateError::StatementSerialReused { serial });
    }
    Ok(())
}

//! Signing a release's manifest and latest statement, then checking both as Syrinx would.
//!
//! The weekly re-sign (#55) signs only a new statement, for a manifest it has verified first.

use crate::check_signature;
use crate::cng;
use crate::error::UpdateError;
use crate::keys::{KeyId, KeySet};
use crate::latest::verify_statement;
use crate::manifest::Manifest;
use crate::signature::{DetachedSignature, SIGNATURE_BYTES, SignatureKind};
use crate::statement::Statement;
use crate::time::Timestamp;
use crate::version::Version;

/// An ECDSA P-256 private key that signs digests without revealing its private half.
pub trait Signer {
    /// The raw `X || Y` public point a release pins.
    fn public_point(&self) -> Result<[u8; 64], UpdateError>;

    /// Sign a SHA-256 digest, returning `r || s`.
    fn sign_digest(&self, digest: &[u8; 32]) -> Result<[u8; 64], UpdateError>;
}

/// What the release tool writes: `manifest.sig`, `latest` and `latest.sig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedRelease {
    pub manifest_signature: [u8; SIGNATURE_BYTES],
    pub statement: Vec<u8>,
    pub statement_signature: [u8; SIGNATURE_BYTES],
    pub key_id: KeyId,
}

/// What a weekly re-sign writes: a new `latest` and `latest.sig`, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedStatement {
    pub statement: Vec<u8>,
    pub statement_signature: [u8; SIGNATURE_BYTES],
    pub key_id: KeyId,
}

/// A published release's four signed files, as the weekly re-sign downloads them.
#[derive(Debug, Clone, Copy)]
pub struct PublishedRelease<'a> {
    pub manifest: &'a [u8],
    pub manifest_signature: &'a [u8],
    pub statement: &'a [u8],
    pub statement_signature: &'a [u8],
}

/// The statement fields the owner chooses at signing time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatementTerms {
    pub serial: u64,
    pub issued: Timestamp,
    pub expires: Timestamp,
}

/// Sign `manifest_bytes` and a statement naming it, refusing a key `keys` does not pin.
///
/// Both signatures are then checked against `keys`, so a release Syrinx would refuse is never written.
pub fn sign_release(
    manifest_bytes: &[u8],
    signer: &impl Signer,
    keys: &KeySet,
    terms: StatementTerms,
) -> Result<SignedRelease, UpdateError> {
    let manifest = Manifest::parse(manifest_bytes)?;
    let key_id = KeyId::of(&signer.public_point()?)?;
    let key = keys.find(key_id).ok_or(UpdateError::UnknownKey)?;
    let statement = statement_bytes(&manifest, manifest_bytes, terms)?;
    let manifest_signature = sign_bytes(signer, key_id, SignatureKind::Manifest, manifest_bytes)?;
    let statement_signature = sign_bytes(signer, key_id, SignatureKind::Statement, &statement)?;

    // Check the pair as an installed copy would at the moment of issue.
    verify_statement(
        &statement,
        &statement_signature.to_bytes(),
        keys,
        terms.issued,
        None,
    )
    .map(drop)?;
    check_signature(key, manifest_bytes, &manifest_signature)?;
    Ok(SignedRelease {
        manifest_signature: manifest_signature.to_bytes(),
        statement,
        statement_signature: statement_signature.to_bytes(),
        key_id,
    })
}

/// Sign the SHA-256 of `bytes` as a detached signature of `kind`.
fn sign_bytes(
    signer: &impl Signer,
    key_id: KeyId,
    kind: SignatureKind,
    bytes: &[u8],
) -> Result<DetachedSignature, UpdateError> {
    let value = signer.sign_digest(&cng::sha256(bytes)?)?;
    Ok(DetachedSignature::new(kind, key_id, value))
}

/// Verify a published release except for its statement's expiry, returning that statement.
///
/// Both signatures must verify against `keys`, the statement must name this manifest, and both
/// must be for `expected`, the tag's version, so a tampered download or an older release's
/// valid files are refused before any key is opened.
pub fn verify_published(
    release: &PublishedRelease<'_>,
    keys: &KeySet,
    expected: Version,
) -> Result<Statement, UpdateError> {
    // Only signed bytes reach the parser.
    let signature =
        DetachedSignature::parse(release.statement_signature, SignatureKind::Statement)?;
    let key = keys
        .find(signature.key_id())
        .ok_or(UpdateError::UnknownKey)?;
    check_signature(key, release.statement, &signature)?;
    let statement = Statement::parse(release.statement)?;
    if statement.version() != expected {
        return Err(UpdateError::UnexpectedVersion {
            expected,
            published: statement.version(),
        });
    }

    // At its own issue time a statement is fresh, so this sets aside only its expiry. The
    // manifest check also requires the manifest's version to equal the statement's.
    verify_statement(
        release.statement,
        release.statement_signature,
        keys,
        statement.issued(),
        None,
    )?
    .verify_manifest(
        release.manifest,
        release.manifest_signature,
        keys,
        Version::new(0, 0, 0),
    )?;
    Ok(statement)
}

/// Sign only a new statement for a published manifest of `expected`, after verifying the release.
///
/// The manifest and its signature are never re-signed; the serial must exceed the old one.
pub fn resign_statement(
    release: &PublishedRelease<'_>,
    signer: &impl Signer,
    keys: &KeySet,
    expected: Version,
    terms: StatementTerms,
) -> Result<SignedStatement, UpdateError> {
    let previous = verify_published(release, keys, expected)?;
    if terms.serial <= previous.serial() {
        return Err(UpdateError::StatementReplayed {
            accepted: previous.serial(),
            offered: terms.serial,
        });
    }
    let manifest = Manifest::parse(release.manifest)?;
    if manifest.version() != expected {
        return Err(UpdateError::UnexpectedVersion {
            expected,
            published: manifest.version(),
        });
    }
    let key_id = KeyId::of(&signer.public_point()?)?;
    keys.find(key_id).ok_or(UpdateError::UnknownKey)?;
    let statement = statement_bytes(&manifest, release.manifest, terms)?;
    let signature = sign_bytes(signer, key_id, SignatureKind::Statement, &statement)?.to_bytes();

    // Check the new statement with the old manifest signature, as an installed copy would.
    verify_statement(&statement, &signature, keys, terms.issued, None)?.verify_manifest(
        release.manifest,
        release.manifest_signature,
        keys,
        Version::new(0, 0, 0),
    )?;
    Ok(SignedStatement {
        statement,
        statement_signature: signature,
        key_id,
    })
}

/// The canonical statement naming `manifest` on the given terms.
fn statement_bytes(
    manifest: &Manifest,
    manifest_bytes: &[u8],
    terms: StatementTerms,
) -> Result<Vec<u8>, UpdateError> {
    Ok(Statement::new(
        terms.serial,
        manifest.version(),
        cng::sha256(manifest_bytes)?,
        terms.issued,
        terms.expires,
    )?
    .to_bytes())
}

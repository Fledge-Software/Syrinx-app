//! Signing a release's manifest and latest statement, then checking both as Syrinx would.

use crate::check_signature;
use crate::cng;
use crate::error::UpdateError;
use crate::keys::{KeyId, KeySet};
use crate::latest::verify_statement;
use crate::manifest::Manifest;
use crate::signature::{DetachedSignature, SIGNATURE_BYTES, SignatureKind};
use crate::statement::Statement;
use crate::time::Timestamp;

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
    let statement = Statement::new(
        terms.serial,
        manifest.version(),
        cng::sha256(manifest_bytes)?,
        terms.issued,
        terms.expires,
    )?
    .to_bytes();
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

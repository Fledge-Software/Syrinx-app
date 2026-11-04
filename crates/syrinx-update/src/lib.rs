//! Local verification of signed update packages and latest statements, and the staging and
//! install steps built on it (P1-39b): no download, network call or UI.
//!
//! The formats, key handling and install protocol are in `docs/update-format.md`.

#[cfg_attr(not(windows), path = "cng_portable.rs")]
mod cng;
mod directory;
mod error;
pub mod install;
mod keys;
mod latest;
mod manifest;
#[cfg(feature = "release-tool")]
mod ncrypt;
// The release tool signs with Windows key storage, which has no portable equivalent yet.
#[cfg(all(feature = "release-tool", not(windows)))]
compile_error!(
    "the `release-tool` feature signs with Windows key storage and builds only on Windows"
);
mod path;
mod payload;
#[cfg(any(test, feature = "release-tool"))]
mod sign;
mod signature;
mod statement;
mod time;
mod version;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_statement;

/// SHA-256 through Windows CNG, for the release tool's file listing.
pub use cng::sha256;
pub use directory::DirectoryPayload;
pub use error::{ManifestProblem, PathProblem, StatementProblem, UpdateError};
pub use keys::{KeyId, KeySet, PRODUCTION_KEYS, PinnedKey};
pub use latest::{
    AcceptedStatement, MAX_ISSUED_AHEAD_SECONDS, VerifiedStatement, verify_statement,
};
pub use manifest::{
    FileEntry, MAX_FILE_BYTES, MAX_FILES, MAX_MANIFEST_BYTES, MAX_TOTAL_BYTES, Manifest,
};
#[cfg(feature = "release-tool")]
pub use ncrypt::{KeyStorage, StoredKey};
pub use path::{MAX_COMPONENT_BYTES, MAX_COMPONENTS, MAX_PATH_BYTES, PackagePath};
pub use payload::{Payload, VerifiedFile, VerifiedPayload, read_bounded};
#[cfg(any(test, feature = "release-tool"))]
pub use sign::{SignedRelease, Signer, StatementTerms, sign_release};
pub use signature::{DetachedSignature, SIGNATURE_BYTES, SignatureKind};
pub use statement::{MAX_STATEMENT_BYTES, MAX_STATEMENT_LIFETIME_SECONDS, Statement};
pub use time::{SECONDS_PER_DAY, Timestamp};
pub use version::Version;

/// A manifest whose signature, form and version have all been accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedManifest {
    manifest: Manifest,
    key_id: KeyId,
}

impl VerifiedManifest {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Which pinned key signed this manifest.
    pub fn key_id(&self) -> KeyId {
        self.key_id
    }

    /// Check that the payload holds exactly the listed files with their sizes and hashes.
    ///
    /// The result holds the hashed handles; install from them, never by reopening the files.
    pub fn verify_payload<P: Payload>(
        &self,
        payload: P,
    ) -> Result<VerifiedPayload<P>, UpdateError> {
        payload::verify(self.manifest.files(), payload)
    }
}

/// Verify a manifest's signature before parsing it, then refuse rollbacks.
pub fn verify_manifest(
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
    let signature = DetachedSignature::parse(signature_bytes, SignatureKind::Manifest)?;
    let key = keys
        .find(signature.key_id())
        .ok_or(UpdateError::UnknownKey)?;
    check_signature(key, manifest_bytes, &signature)?;

    // Only signed bytes reach the parser.
    let manifest = Manifest::parse(manifest_bytes)?;
    check_rollback(&manifest, installed)?;
    Ok(VerifiedManifest {
        manifest,
        key_id: signature.key_id(),
    })
}

/// Hash the raw signed bytes and verify them against the named pinned key.
pub(crate) fn check_signature(
    key: &PinnedKey,
    signed_bytes: &[u8],
    signature: &DetachedSignature,
) -> Result<(), UpdateError> {
    if !cng::verify_message(key.point(), signed_bytes, signature.value())? {
        return Err(UpdateError::BadSignature);
    }
    Ok(())
}

/// Refuse a package that is not strictly newer, or that needs a newer installation.
fn check_rollback(manifest: &Manifest, installed: Version) -> Result<(), UpdateError> {
    if manifest.version() <= installed {
        return Err(UpdateError::NotNewer {
            installed,
            offered: manifest.version(),
        });
    }
    if installed < manifest.minimum_version() {
        return Err(UpdateError::InstalledTooOld {
            installed,
            minimum: manifest.minimum_version(),
        });
    }
    Ok(())
}

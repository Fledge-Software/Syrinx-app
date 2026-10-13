//! Local verification of signed update packages (P1-38): no download, install or UI.
//!
//! The format and key handling are in `docs/p1-38-update-format.md`.

mod cng;
mod directory;
mod error;
mod keys;
mod manifest;
mod path;
mod payload;
mod signature;
mod version;

#[cfg(test)]
mod tests;

pub use directory::DirectoryPayload;
pub use error::{ManifestProblem, PathProblem, UpdateError};
pub use keys::{KeyId, KeySet, PRODUCTION_KEYS, PinnedKey};
pub use manifest::{
    FileEntry, MAX_FILE_BYTES, MAX_FILES, MAX_MANIFEST_BYTES, MAX_TOTAL_BYTES, Manifest,
};
pub use path::{MAX_COMPONENT_BYTES, MAX_COMPONENTS, MAX_PATH_BYTES, PackagePath};
pub use payload::{Payload, VerifiedFile, VerifiedPayload, read_bounded};
pub use signature::{DetachedSignature, SIGNATURE_BYTES};
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
    let signature = DetachedSignature::parse(signature_bytes)?;
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

/// Hash the raw manifest bytes and verify them against the named pinned key.
fn check_signature(
    key: &PinnedKey,
    manifest_bytes: &[u8],
    signature: &DetachedSignature,
) -> Result<(), UpdateError> {
    let digest = cng::sha256(manifest_bytes)?;
    if !cng::verify_signature(key.point(), &digest, signature.value())? {
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

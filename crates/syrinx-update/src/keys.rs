//! The verification keys pinned in the executable; packages never supply their own.

use crate::cng;
use crate::error::UpdateError;

/// The release keys this executable trusts, as raw P-256 `X || Y` points.
///
/// Empty on purpose: until the owner creates a release key outside the repository,
/// every package fails as signed by an unknown key.
// TODO(DW-30): pin the owner's release public key and its separately recorded identifier.
pub const PRODUCTION_KEYS: &[PinnedKey] = &[];

/// The first 8 bytes of SHA-256 over a key's public point; it selects a key, not trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(pub [u8; 8]);

impl KeyId {
    /// Derive the identifier of a raw public point.
    pub fn of(point: &[u8; 64]) -> Result<Self, UpdateError> {
        let digest = cng::sha256(point)?;
        let mut id = [0u8; 8];
        id.copy_from_slice(&digest[..8]);
        Ok(Self(id))
    }
}

/// A public verification key compiled into Syrinx.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedKey {
    point: [u8; 64],
}

impl PinnedKey {
    pub const fn new(point: [u8; 64]) -> Self {
        Self { point }
    }

    pub fn point(&self) -> &[u8; 64] {
        &self.point
    }
}

/// Validated pinned keys, indexed by identifier.
#[derive(Debug, Clone)]
pub struct KeySet {
    keys: Vec<(KeyId, PinnedKey)>,
}

impl KeySet {
    /// Validate each point with CNG and refuse identifier collisions.
    pub fn new(pinned: &[PinnedKey]) -> Result<Self, UpdateError> {
        let mut keys: Vec<(KeyId, PinnedKey)> = Vec::with_capacity(pinned.len());
        for key in pinned {
            cng::validate_public_key(key.point())?;
            let id = KeyId::of(key.point())?;
            if keys.iter().any(|(existing, _)| *existing == id) {
                return Err(UpdateError::DuplicateKey);
            }
            keys.push((id, *key));
        }
        Ok(Self { keys })
    }

    /// The keys release packages are checked against.
    pub fn production() -> Result<Self, UpdateError> {
        Self::new(PRODUCTION_KEYS)
    }

    pub(crate) fn find(&self, id: KeyId) -> Option<&PinnedKey> {
        self.keys
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, key)| key)
    }
}

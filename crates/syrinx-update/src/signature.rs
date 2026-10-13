//! The detached `manifest.sig` layout: magic, key identifier and `r || s`.

use crate::error::UpdateError;
use crate::keys::KeyId;

/// Exact length of a detached signature file.
pub const SIGNATURE_BYTES: usize = 76;
const MAGIC: [u8; 4] = *b"AUS1";

/// A parsed detached signature; nothing in it is trusted until verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetachedSignature {
    key_id: KeyId,
    value: [u8; 64],
}

impl DetachedSignature {
    pub fn new(key_id: KeyId, value: [u8; 64]) -> Self {
        Self { key_id, value }
    }

    /// Require the exact length and magic, then split the fields.
    pub fn parse(bytes: &[u8]) -> Result<Self, UpdateError> {
        let bytes: &[u8; SIGNATURE_BYTES] =
            bytes.try_into().map_err(|_| UpdateError::SignatureLength {
                actual: bytes.len(),
            })?;
        let (magic, rest) = bytes.split_at(MAGIC.len());
        if magic != MAGIC {
            return Err(UpdateError::SignatureFormat);
        }
        let (id, value) = rest.split_at(8);
        let mut key_id = [0u8; 8];
        key_id.copy_from_slice(id);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(value);
        Ok(Self::new(KeyId(key_id), signature))
    }

    pub fn to_bytes(&self) -> [u8; SIGNATURE_BYTES] {
        let mut bytes = [0u8; SIGNATURE_BYTES];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4..12].copy_from_slice(&self.key_id.0);
        bytes[12..].copy_from_slice(&self.value);
        bytes
    }

    pub fn key_id(&self) -> KeyId {
        self.key_id
    }

    pub(crate) fn value(&self) -> &[u8; 64] {
        &self.value
    }
}

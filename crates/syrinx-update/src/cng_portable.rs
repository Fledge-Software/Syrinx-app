//! SHA-256 and ECDSA P-256 through `ring` where Windows CNG is unavailable (Linux, macOS).

use ring::agreement::{self, ECDH_P256, EphemeralPrivateKey};
use ring::digest::{Context, SHA256};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};

use crate::error::UpdateError;

/// `ring` reports no status code, so every refusal carries this negative stand-in.
const REFUSED: i32 = -1;
/// The SEC 1 tag for an uncompressed point, which `ring` expects before `X || Y`.
const UNCOMPRESSED_TAG: u8 = 0x04;

/// An incremental SHA-256 computation.
pub(crate) struct Sha256(Context);

impl Sha256 {
    pub(crate) fn new() -> Result<Self, UpdateError> {
        Ok(Self(Context::new(&SHA256)))
    }

    pub(crate) fn update(&mut self, data: &[u8]) -> Result<(), UpdateError> {
        self.0.update(data);
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<[u8; 32], UpdateError> {
        let mut digest = [0u8; 32];
        digest.copy_from_slice(self.0.finish().as_ref());
        Ok(digest)
    }
}

/// SHA-256 of one in-memory buffer.
pub fn sha256(data: &[u8]) -> Result<[u8; 32], UpdateError> {
    let mut hash = Sha256::new()?;
    hash.update(data)?;
    hash.finish()
}

/// The point in the uncompressed SEC 1 form `ring` parses.
fn uncompressed(point: &[u8; 64]) -> [u8; 65] {
    let mut encoded = [0u8; 65];
    encoded[0] = UNCOMPRESSED_TAG;
    encoded[1..].copy_from_slice(point);
    encoded
}

/// Confirm that a pinned point is a usable P-256 public key.
///
/// `ring`'s signature check cannot tell a bad key from a bad signature, so the point is
/// validated through an ephemeral key agreement, which refuses points not on the curve.
pub(crate) fn validate_public_key(point: &[u8; 64]) -> Result<(), UpdateError> {
    let random = SystemRandom::new();
    let ephemeral =
        EphemeralPrivateKey::generate(&ECDH_P256, &random).map_err(|_| UpdateError::Crypto {
            operation: "generating an ephemeral key",
            status: REFUSED,
        })?;
    let peer = agreement::UnparsedPublicKey::new(&ECDH_P256, uncompressed(point));
    agreement::agree_ephemeral(ephemeral, &peer, |_| ())
        .map_err(|_| UpdateError::InvalidKey { status: REFUSED })
}

/// Verify an `r || s` signature over the SHA-256 of `message`; `Ok(false)` means no match.
pub(crate) fn verify_message(
    point: &[u8; 64],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<bool, UpdateError> {
    validate_public_key(point)?;
    let key = UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, uncompressed(point));
    Ok(key.verify(message, signature).is_ok())
}

/// In-process key pairs that exist only for the duration of a test.
#[cfg(test)]
pub(crate) mod test_keys {
    use ring::rand::SystemRandom;
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};

    use super::REFUSED;
    use crate::error::UpdateError;
    use crate::sign::Signer;

    /// An ephemeral P-256 key pair held only in this process's memory.
    pub(crate) struct TestKeyPair {
        pair: EcdsaKeyPair,
        random: SystemRandom,
    }

    fn test_key_error(operation: &'static str) -> UpdateError {
        UpdateError::Crypto {
            operation,
            status: REFUSED,
        }
    }

    impl TestKeyPair {
        pub(crate) fn generate() -> Result<Self, UpdateError> {
            let random = SystemRandom::new();
            let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &random)
                .map_err(|_| test_key_error("generating a test key"))?;
            let pair =
                EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &random)
                    .map_err(|_| test_key_error("loading a test key"))?;
            Ok(Self { pair, random })
        }

        /// The raw `X || Y` point a release would pin.
        pub(crate) fn public_point(&self) -> Result<[u8; 64], UpdateError> {
            let encoded = self.pair.public_key().as_ref();
            if encoded.len() != 65 || encoded[0] != super::UNCOMPRESSED_TAG {
                return Err(test_key_error("exporting a test key"));
            }
            let mut point = [0u8; 64];
            point.copy_from_slice(&encoded[1..]);
            Ok(point)
        }

        /// Sign the SHA-256 of `message`, producing the `r || s` layout CNG uses.
        pub(crate) fn sign_message(&self, message: &[u8]) -> Result<[u8; 64], UpdateError> {
            let signature = self
                .pair
                .sign(&self.random, message)
                .map_err(|_| test_key_error("signing with a test key"))?;
            let bytes = signature.as_ref();
            if bytes.len() != 64 {
                return Err(UpdateError::SignatureLength {
                    actual: bytes.len(),
                });
            }
            let mut value = [0u8; 64];
            value.copy_from_slice(bytes);
            Ok(value)
        }
    }

    impl Signer for TestKeyPair {
        fn public_point(&self) -> Result<[u8; 64], UpdateError> {
            TestKeyPair::public_point(self)
        }

        /// `ring` signs whole messages only; every caller goes through `sign_message`.
        fn sign_digest(&self, _digest: &[u8; 32]) -> Result<[u8; 64], UpdateError> {
            Err(test_key_error("signing a bare digest"))
        }

        fn sign_message(&self, message: &[u8]) -> Result<[u8; 64], UpdateError> {
            TestKeyPair::sign_message(self, message)
        }
    }
}

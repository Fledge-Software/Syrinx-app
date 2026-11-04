//! SHA-256 and ECDSA P-256 through Windows CNG, so no new cryptography crate is needed.

use windows::Win32::Foundation::{NTSTATUS, STATUS_INVALID_SIGNATURE, STATUS_SUCCESS};
use windows::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM,
    BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS, BCRYPT_HASH_HANDLE, BCRYPT_KEY_HANDLE,
    BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider,
    BCryptCreateHash, BCryptDestroyHash, BCryptDestroyKey, BCryptFinishHash, BCryptHashData,
    BCryptImportKeyPair, BCryptOpenAlgorithmProvider, BCryptVerifySignature,
};
use windows::core::PCWSTR;

use crate::error::UpdateError;

/// P-256 coordinates are 32 bytes each.
const COORDINATE_BYTES: u32 = 32;
/// Bytes handed to `BCryptHashData` per call, keeping lengths far below `u32::MAX`.
const HASH_CHUNK_BYTES: usize = 1 << 20;

/// Turn a CNG status into a typed error naming the operation.
fn check(status: NTSTATUS, operation: &'static str) -> Result<(), UpdateError> {
    if status.is_err() {
        return Err(UpdateError::Crypto {
            operation,
            status: status.0,
        });
    }
    Ok(())
}

/// An open CNG algorithm provider, closed on drop.
pub(crate) struct Provider(BCRYPT_ALG_HANDLE);

impl Provider {
    fn open(algorithm: PCWSTR, operation: &'static str) -> Result<Self, UpdateError> {
        let mut handle = BCRYPT_ALG_HANDLE::default();
        let status = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut handle,
                algorithm,
                PCWSTR::null(),
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
        };
        check(status, operation)?;
        Ok(Self(handle))
    }

    pub(crate) fn ecdsa_p256() -> Result<Self, UpdateError> {
        Self::open(BCRYPT_ECDSA_P256_ALGORITHM, "opening ECDSA P-256")
    }

    pub(crate) fn handle(&self) -> BCRYPT_ALG_HANDLE {
        self.0
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        let status = unsafe { BCryptCloseAlgorithmProvider(self.0, 0) };
        report_release_failure(status, "closing a CNG provider");
    }
}

/// A CNG key, destroyed on drop. Declare it before its provider so it is released first.
pub(crate) struct Key(pub(crate) BCRYPT_KEY_HANDLE);

impl Drop for Key {
    fn drop(&mut self) {
        let status = unsafe { BCryptDestroyKey(self.0) };
        report_release_failure(status, "destroying a CNG key");
    }
}

struct HashObject(BCRYPT_HASH_HANDLE);

impl Drop for HashObject {
    fn drop(&mut self) {
        let status = unsafe { BCryptDestroyHash(self.0) };
        report_release_failure(status, "destroying a CNG hash");
    }
}

/// A drop cannot return the failure; releasing a handle Syrinx created fails only on a bug.
fn report_release_failure(status: NTSTATUS, operation: &str) {
    if status.is_err() {
        report_release_code(status.0, operation);
    }
}

/// Write a failed release's status code to standard error.
pub(crate) fn report_release_code(code: i32, operation: &str) {
    eprintln!("syrinx-update: {operation} failed with status {code:#010x}");
}

/// An incremental SHA-256 computation.
pub(crate) struct Sha256 {
    hash: HashObject,
    _provider: Provider,
}

impl Sha256 {
    pub(crate) fn new() -> Result<Self, UpdateError> {
        let provider = Provider::open(BCRYPT_SHA256_ALGORITHM, "opening SHA-256")?;
        let mut handle = BCRYPT_HASH_HANDLE::default();
        let status = unsafe { BCryptCreateHash(provider.handle(), &mut handle, None, None, 0) };
        check(status, "creating a SHA-256 hash")?;
        Ok(Self {
            hash: HashObject(handle),
            _provider: provider,
        })
    }

    pub(crate) fn update(&mut self, data: &[u8]) -> Result<(), UpdateError> {
        for chunk in data.chunks(HASH_CHUNK_BYTES) {
            let status = unsafe { BCryptHashData(self.hash.0, chunk, 0) };
            check(status, "hashing data")?;
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<[u8; 32], UpdateError> {
        let mut digest = [0u8; 32];
        let status = unsafe { BCryptFinishHash(self.hash.0, &mut digest, 0) };
        check(status, "finishing a SHA-256 hash")?;
        Ok(digest)
    }
}

/// SHA-256 of one in-memory buffer.
pub fn sha256(data: &[u8]) -> Result<[u8; 32], UpdateError> {
    let mut hash = Sha256::new()?;
    hash.update(data)?;
    hash.finish()
}

/// An imported P-256 public key with the provider it belongs to.
struct PublicKey {
    key: Key,
    _provider: Provider,
}

/// Import a raw `X || Y` point; CNG refuses points that are not on the curve.
fn import_public_key(point: &[u8; 64]) -> Result<PublicKey, UpdateError> {
    let provider = Provider::ecdsa_p256()?;
    let mut blob = Vec::with_capacity(72);
    blob.extend_from_slice(&BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
    blob.extend_from_slice(&COORDINATE_BYTES.to_le_bytes());
    blob.extend_from_slice(point);
    let mut handle = BCRYPT_KEY_HANDLE::default();
    let status = unsafe {
        BCryptImportKeyPair(
            provider.handle(),
            None,
            BCRYPT_ECCPUBLIC_BLOB,
            &mut handle,
            &blob,
            0,
        )
    };
    if status.is_err() {
        return Err(UpdateError::InvalidKey { status: status.0 });
    }
    Ok(PublicKey {
        key: Key(handle),
        _provider: provider,
    })
}

/// Confirm that a pinned point is a usable P-256 public key.
pub(crate) fn validate_public_key(point: &[u8; 64]) -> Result<(), UpdateError> {
    import_public_key(point).map(drop)
}

/// Verify an `r || s` signature over a SHA-256 digest; `Ok(false)` means it does not match.
pub(crate) fn verify_signature(
    point: &[u8; 64],
    digest: &[u8; 32],
    signature: &[u8; 64],
) -> Result<bool, UpdateError> {
    let public = import_public_key(point)?;
    let status =
        unsafe { BCryptVerifySignature(public.key.0, None, digest, signature, BCRYPT_FLAGS(0)) };
    // Only exact success is a valid signature; any other status is a refusal or an error.
    if status == STATUS_SUCCESS {
        return Ok(true);
    }
    if status == STATUS_INVALID_SIGNATURE {
        return Ok(false);
    }
    Err(UpdateError::Crypto {
        operation: "verifying a signature",
        status: status.0,
    })
}

/// Verify an `r || s` signature over the SHA-256 of `message`; `Ok(false)` means no match.
pub(crate) fn verify_message(
    point: &[u8; 64],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<bool, UpdateError> {
    verify_signature(point, &sha256(message)?, signature)
}

/// In-process key pairs that exist only for the duration of a test.
#[cfg(test)]
pub(crate) mod test_keys {
    use windows::Win32::Security::Cryptography::{
        BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS, BCRYPT_KEY_HANDLE,
        BCryptExportKey, BCryptFinalizeKeyPair, BCryptGenerateKeyPair, BCryptSignHash,
    };

    use super::{Key, Provider, check};
    use crate::error::UpdateError;
    use crate::sign::Signer;

    /// An ephemeral P-256 key pair; the private half never leaves CNG's memory.
    pub(crate) struct TestKeyPair {
        key: Key,
        _provider: Provider,
    }

    impl TestKeyPair {
        pub(crate) fn generate() -> Result<Self, UpdateError> {
            let provider = Provider::ecdsa_p256()?;
            let mut handle = BCRYPT_KEY_HANDLE::default();
            let status = unsafe { BCryptGenerateKeyPair(provider.handle(), &mut handle, 256, 0) };
            check(status, "generating a test key")?;
            let key = Key(handle);
            check(
                unsafe { BCryptFinalizeKeyPair(key.0, 0) },
                "finalizing a test key",
            )?;
            Ok(Self {
                key,
                _provider: provider,
            })
        }

        /// The raw `X || Y` point a release would pin.
        pub(crate) fn public_point(&self) -> Result<[u8; 64], UpdateError> {
            let mut blob = [0u8; 72];
            let mut written = 0u32;
            let status = unsafe {
                BCryptExportKey(
                    self.key.0,
                    None,
                    BCRYPT_ECCPUBLIC_BLOB,
                    Some(&mut blob),
                    &mut written,
                    0,
                )
            };
            check(status, "exporting a test key")?;
            let magic = BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes();
            if written != 72 || blob[..4] != magic {
                return Err(UpdateError::Crypto {
                    operation: "exporting a test key",
                    status: 0,
                });
            }
            let mut point = [0u8; 64];
            point.copy_from_slice(&blob[8..]);
            Ok(point)
        }

        /// Sign a SHA-256 digest, producing CNG's `r || s` layout.
        pub(crate) fn sign(&self, digest: &[u8; 32]) -> Result<[u8; 64], UpdateError> {
            let mut signature = [0u8; 64];
            let mut written = 0u32;
            let status = unsafe {
                BCryptSignHash(
                    self.key.0,
                    None,
                    digest,
                    Some(&mut signature),
                    &mut written,
                    BCRYPT_FLAGS(0),
                )
            };
            check(status, "signing with a test key")?;
            if written != 64 {
                return Err(UpdateError::SignatureLength {
                    actual: written as usize,
                });
            }
            Ok(signature)
        }

        /// Sign the SHA-256 of `message`, as the portable test keys do.
        pub(crate) fn sign_message(&self, message: &[u8]) -> Result<[u8; 64], UpdateError> {
            self.sign(&super::sha256(message)?)
        }
    }

    impl Signer for TestKeyPair {
        fn public_point(&self) -> Result<[u8; 64], UpdateError> {
            TestKeyPair::public_point(self)
        }

        fn sign_digest(&self, digest: &[u8; 32]) -> Result<[u8; 64], UpdateError> {
            self.sign(digest)
        }
    }
}

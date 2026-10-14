//! The owner's release key, named in Windows key storage; Syrinx asks Windows to sign and never reads it.

use windows::Win32::Security::Cryptography::{
    BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_PUBLIC_P256_MAGIC, CERT_KEY_SPEC, MS_KEY_STORAGE_PROVIDER,
    MS_SMART_CARD_KEY_STORAGE_PROVIDER, NCRYPT_FLAGS, NCRYPT_HANDLE, NCRYPT_KEY_HANDLE,
    NCRYPT_PROV_HANDLE, NCryptExportKey, NCryptFreeObject, NCryptOpenKey,
    NCryptOpenStorageProvider, NCryptSignHash,
};
use windows::core::{HSTRING, PCWSTR};

use crate::cng::report_release_code;
use crate::error::UpdateError;
use crate::sign::Signer;

/// Which Windows key storage provider holds the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStorage {
    /// A non-exportable key in Windows' software key storage.
    Software,
    /// A key on a hardware security key, through Windows' smart-card key storage.
    SmartCard,
}

impl KeyStorage {
    fn provider_name(self) -> PCWSTR {
        match self {
            Self::Software => MS_KEY_STORAGE_PROVIDER,
            Self::SmartCard => MS_SMART_CARD_KEY_STORAGE_PROVIDER,
        }
    }
}

/// An open handle to a stored P-256 key; Windows may ask the owner for a PIN when it signs.
pub struct StoredKey {
    key: KeyHandle,
    _provider: ProviderHandle,
    point: [u8; 64],
}

impl StoredKey {
    /// Open `name` in `storage` and read its public half, refusing a key that is not P-256.
    pub fn open(storage: KeyStorage, name: &str) -> Result<Self, UpdateError> {
        let provider = ProviderHandle::open(storage)?;
        let mut handle = NCRYPT_KEY_HANDLE::default();
        let name = HSTRING::from(name);
        unsafe {
            NCryptOpenKey(
                provider.0,
                &mut handle,
                &name,
                CERT_KEY_SPEC(0),
                NCRYPT_FLAGS(0),
            )
        }
        .map_err(|error| crypto_error("opening the signing key", &error))?;
        let key = KeyHandle(handle);
        let point = export_point(&key)?;
        Ok(Self {
            key,
            _provider: provider,
            point,
        })
    }
}

impl Signer for StoredKey {
    fn public_point(&self) -> Result<[u8; 64], UpdateError> {
        Ok(self.point)
    }

    fn sign_digest(&self, digest: &[u8; 32]) -> Result<[u8; 64], UpdateError> {
        let mut signature = [0u8; 64];
        let mut written = 0u32;
        unsafe {
            NCryptSignHash(
                self.key.0,
                None,
                digest,
                Some(&mut signature),
                &mut written,
                NCRYPT_FLAGS(0),
            )
        }
        .map_err(|error| crypto_error("signing with the stored key", &error))?;
        if written != 64 {
            return Err(UpdateError::UnsuitableSigningKey);
        }
        Ok(signature)
    }
}

/// Export only the public blob and check it is a P-256 ECDSA point.
fn export_point(key: &KeyHandle) -> Result<[u8; 64], UpdateError> {
    let mut blob = [0u8; 72];
    let mut written = 0u32;
    unsafe {
        NCryptExportKey(
            key.0,
            None,
            BCRYPT_ECCPUBLIC_BLOB,
            None,
            Some(&mut blob),
            &mut written,
            NCRYPT_FLAGS(0),
        )
    }
    .map_err(|error| crypto_error("exporting the signing key's public half", &error))?;
    let magic = BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes();
    if written != 72 || blob[..4] != magic {
        return Err(UpdateError::UnsuitableSigningKey);
    }
    let mut point = [0u8; 64];
    point.copy_from_slice(&blob[8..]);
    Ok(point)
}

fn crypto_error(operation: &'static str, error: &windows::core::Error) -> UpdateError {
    UpdateError::Crypto {
        operation,
        status: error.code().0,
    }
}

/// An open key storage provider, freed on drop.
struct ProviderHandle(NCRYPT_PROV_HANDLE);

impl ProviderHandle {
    fn open(storage: KeyStorage) -> Result<Self, UpdateError> {
        let mut handle = NCRYPT_PROV_HANDLE::default();
        unsafe { NCryptOpenStorageProvider(&mut handle, storage.provider_name(), 0) }
            .map_err(|error| crypto_error("opening Windows key storage", &error))?;
        Ok(Self(handle))
    }
}

impl Drop for ProviderHandle {
    fn drop(&mut self) {
        free_object(
            NCRYPT_HANDLE::from(self.0),
            "freeing a key storage provider",
        );
    }
}

/// An open stored key, freed on drop. Declare it before its provider so it is released first.
struct KeyHandle(NCRYPT_KEY_HANDLE);

impl Drop for KeyHandle {
    fn drop(&mut self) {
        free_object(NCRYPT_HANDLE::from(self.0), "freeing a stored key handle");
    }
}

fn free_object(handle: NCRYPT_HANDLE, operation: &str) {
    if let Err(error) = unsafe { NCryptFreeObject(handle) } {
        report_release_code(error.code().0, operation);
    }
}

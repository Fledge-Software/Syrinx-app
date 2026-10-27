//! The owner's release key, named in Windows key storage; Syrinx asks Windows to sign and never reads it.
//!
//! The weekly workflow also imports the key from its secret for one job and deletes it after (#55).

use windows::Win32::Foundation::{NTE_BAD_DATA, NTE_BAD_KEYSET, NTE_EXISTS};
use windows::Win32::Security::Cryptography::{
    BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPTBUFFER_VERSION, BCryptBuffer,
    BCryptBufferDesc, CERT_KEY_SPEC, CRYPT_STRING_BASE64, CryptStringToBinaryW,
    MS_KEY_STORAGE_PROVIDER, MS_SMART_CARD_KEY_STORAGE_PROVIDER, NCRYPT_FLAGS, NCRYPT_HANDLE,
    NCRYPT_KEY_HANDLE, NCRYPT_PKCS8_PRIVATE_KEY_BLOB, NCRYPT_PROV_HANDLE, NCRYPT_SILENT_FLAG,
    NCRYPTBUFFER_PKCS_KEY_NAME, NCryptDeleteKey, NCryptExportKey, NCryptFreeObject,
    NCryptImportKey, NCryptOpenKey, NCryptOpenStorageProvider, NCryptSignHash,
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
        let key = open_named(&provider, name)
            .map_err(|error| crypto_error("opening the signing key", &error))?;
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

/// Longest base64 text accepted for a key; a P-256 PKCS#8 key is under 200 characters.
const MAX_KEY_TEXT_CHARS: usize = 4096;

/// Import a base64 PKCS#8 P-256 private key into Windows' software key storage as `name`.
///
/// The stored key cannot be exported again. Returns its public point; a key that is not
/// P-256 is deleted again and refused. An existing key of the same name is never replaced.
pub fn import_software_key(name: &str, base64: &str) -> Result<[u8; 64], UpdateError> {
    let provider = ProviderHandle::open(KeyStorage::Software)?;
    // A PKCS#8 import silently replaces a key of the same name, so refuse one first.
    if open_named(&provider, name).is_ok() {
        return Err(UpdateError::Crypto {
            operation: "importing the signing key: a key of that name already exists",
            status: NTE_EXISTS.0,
        });
    }
    let mut der = decode_base64(base64)?;
    let imported = import_pkcs8(&provider, Some(name), &der);
    wipe(&mut der);
    let key = imported?;
    match export_point(&key) {
        Ok(point) => Ok(point),
        Err(error) => {
            delete_key(key)?;
            Err(error)
        }
    }
}

/// Delete `name` from Windows' software key storage; `Ok(false)` when no such key exists.
pub fn delete_software_key(name: &str) -> Result<bool, UpdateError> {
    let provider = ProviderHandle::open(KeyStorage::Software)?;
    let key = match open_named(&provider, name) {
        Ok(key) => key,
        Err(error) if error.code() == NTE_BAD_KEYSET => return Ok(false),
        Err(error) => return Err(crypto_error("opening the key to delete", &error)),
    };
    delete_key(key)?;
    Ok(true)
}

/// Open a persisted key by name, without any prompt.
fn open_named(provider: &ProviderHandle, name: &str) -> windows::core::Result<KeyHandle> {
    let mut handle = NCRYPT_KEY_HANDLE::default();
    let name = HSTRING::from(name);
    unsafe {
        NCryptOpenKey(
            provider.0,
            &mut handle,
            &name,
            CERT_KEY_SPEC(0),
            NCRYPT_SILENT_FLAG,
        )
    }?;
    Ok(KeyHandle(handle))
}

/// Import PKCS#8 bytes, persisted under `name`, or ephemeral when `name` is `None`.
fn import_pkcs8(
    provider: &ProviderHandle,
    name: Option<&str>,
    der: &[u8],
) -> Result<KeyHandle, UpdateError> {
    let mut wide_name: Vec<u16> = name.unwrap_or_default().encode_utf16().chain([0]).collect();
    let mut buffer = BCryptBuffer {
        cbBuffer: (wide_name.len() * 2) as u32,
        BufferType: NCRYPTBUFFER_PKCS_KEY_NAME,
        pvBuffer: wide_name.as_mut_ptr().cast(),
    };
    let descriptor = BCryptBufferDesc {
        ulVersion: BCRYPTBUFFER_VERSION,
        cBuffers: 1,
        pBuffers: &mut buffer,
    };
    let parameters = name.map(|_| &raw const descriptor);
    let mut handle = NCRYPT_KEY_HANDLE::default();
    unsafe {
        NCryptImportKey(
            provider.0,
            None,
            NCRYPT_PKCS8_PRIVATE_KEY_BLOB,
            parameters,
            &mut handle,
            der,
            NCRYPT_SILENT_FLAG,
        )
    }
    .map_err(|error| crypto_error("importing the signing key", &error))?;
    Ok(KeyHandle(handle))
}

/// Delete a persisted key; Windows frees the handle when the deletion succeeds.
fn delete_key(key: KeyHandle) -> Result<(), UpdateError> {
    let key = std::mem::ManuallyDrop::new(key);
    unsafe { NCryptDeleteKey(key.0, NCRYPT_SILENT_FLAG.0) }.map_err(|error| {
        // The handle survives a failed deletion, so free it here.
        drop(std::mem::ManuallyDrop::into_inner(key));
        crypto_error("deleting the signing key", &error)
    })
}

/// Decode base64 text, ignoring line breaks, refusing anything over the size bound.
fn decode_base64(text: &str) -> Result<Vec<u8>, UpdateError> {
    let text = text.trim();
    // Windows reads an empty string as NUL-terminated, past the end of the buffer.
    if text.is_empty() {
        return Err(UpdateError::Crypto {
            operation: "decoding the key's base64: it is empty",
            status: NTE_BAD_DATA.0,
        });
    }
    if text.len() > MAX_KEY_TEXT_CHARS {
        return Err(UpdateError::TooLarge {
            what: "release key text",
            limit: MAX_KEY_TEXT_CHARS as u64,
        });
    }
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let decoded = decode_wide_base64(&wide);
    wipe(&mut wide);
    decoded
}

/// Ask Windows for the decoded length, then decode into a buffer of that size.
fn decode_wide_base64(wide: &[u16]) -> Result<Vec<u8>, UpdateError> {
    let mut length = 0u32;
    unsafe { CryptStringToBinaryW(wide, CRYPT_STRING_BASE64, None, &mut length, None, None) }
        .map_err(|error| crypto_error("decoding the key's base64", &error))?;
    let mut bytes = vec![0u8; length as usize];
    unsafe {
        CryptStringToBinaryW(
            wide,
            CRYPT_STRING_BASE64,
            Some(bytes.as_mut_ptr()),
            &mut length,
            None,
            None,
        )
    }
    .map_err(|error| crypto_error("decoding the key's base64", &error))?;
    bytes.truncate(length as usize);
    Ok(bytes)
}

/// Overwrite private key material before its buffer is freed.
fn wipe<T: Copy + Default>(values: &mut [T]) {
    for value in values.iter_mut() {
        unsafe { std::ptr::write_volatile(value, T::default()) };
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

/// Exportable in-memory keys and their base64 PKCS#8 text, which exist only for a test.
#[cfg(test)]
pub(crate) mod test_keys {
    use windows::Win32::Security::Cryptography::{
        BCRYPT_ECDSA_P256_ALGORITHM, BCRYPT_ECDSA_P384_ALGORITHM, CERT_KEY_SPEC, CRYPT_STRING,
        CRYPT_STRING_BASE64, CRYPT_STRING_NOCRLF, CryptBinaryToStringW,
        NCRYPT_ALLOW_PLAINTEXT_EXPORT_FLAG, NCRYPT_EXPORT_POLICY_PROPERTY, NCRYPT_FLAGS,
        NCRYPT_HANDLE, NCRYPT_KEY_HANDLE, NCRYPT_PKCS8_PRIVATE_KEY_BLOB, NCryptCreatePersistedKey,
        NCryptExportKey, NCryptFinalizeKey, NCryptSetProperty,
    };
    use windows::core::PCWSTR;

    use super::{KeyHandle, KeyStorage, ProviderHandle, crypto_error, export_point, import_pkcs8};
    use crate::error::UpdateError;

    /// Base64 PKCS#8 text of a fresh exportable P-256 key, as the owner's secret holds it.
    pub(crate) fn p256_secret_text() -> Result<String, UpdateError> {
        secret_text(BCRYPT_ECDSA_P256_ALGORITHM)
    }

    /// The same for a P-384 key, which signing must refuse.
    pub(crate) fn p384_secret_text() -> Result<String, UpdateError> {
        secret_text(BCRYPT_ECDSA_P384_ALGORITHM)
    }

    /// The public point of base64 PKCS#8 text, through an ephemeral import that persists nothing.
    pub(crate) fn point_of(text: &str) -> Result<[u8; 64], UpdateError> {
        let provider = ProviderHandle::open(KeyStorage::Software)?;
        let key = import_pkcs8(&provider, None, &super::decode_base64(text)?)?;
        export_point(&key)
    }

    /// Whether Windows will hand out a stored key's private half (it must not).
    pub(crate) fn private_half_exports(name: &str) -> Result<bool, UpdateError> {
        let provider = ProviderHandle::open(KeyStorage::Software)?;
        let key = super::open_named(&provider, name)
            .map_err(|error| crypto_error("opening a test key", &error))?;
        Ok(pkcs8_of(&key).is_ok())
    }

    fn secret_text(algorithm: PCWSTR) -> Result<String, UpdateError> {
        let provider = ProviderHandle::open(KeyStorage::Software)?;
        let mut handle = NCRYPT_KEY_HANDLE::default();
        // No name: the key is ephemeral and never reaches the key store.
        unsafe {
            NCryptCreatePersistedKey(
                provider.0,
                &mut handle,
                algorithm,
                PCWSTR::null(),
                CERT_KEY_SPEC(0),
                NCRYPT_FLAGS(0),
            )
        }
        .map_err(|error| crypto_error("creating a test key", &error))?;
        let key = KeyHandle(handle);
        let policy = NCRYPT_ALLOW_PLAINTEXT_EXPORT_FLAG.to_le_bytes();
        unsafe {
            NCryptSetProperty(
                NCRYPT_HANDLE::from(key.0),
                NCRYPT_EXPORT_POLICY_PROPERTY,
                &policy,
                NCRYPT_FLAGS(0),
            )
        }
        .map_err(|error| crypto_error("allowing a test key's export", &error))?;
        unsafe { NCryptFinalizeKey(key.0, NCRYPT_FLAGS(0)) }
            .map_err(|error| crypto_error("finalizing a test key", &error))?;
        base64(&pkcs8_of(&key)?)
    }

    fn pkcs8_of(key: &KeyHandle) -> Result<Vec<u8>, UpdateError> {
        let mut length = 0u32;
        unsafe {
            NCryptExportKey(
                key.0,
                None,
                NCRYPT_PKCS8_PRIVATE_KEY_BLOB,
                None,
                None,
                &mut length,
                NCRYPT_FLAGS(0),
            )
        }
        .map_err(|error| crypto_error("sizing a test key's export", &error))?;
        let mut bytes = vec![0u8; length as usize];
        unsafe {
            NCryptExportKey(
                key.0,
                None,
                NCRYPT_PKCS8_PRIVATE_KEY_BLOB,
                None,
                Some(&mut bytes),
                &mut length,
                NCRYPT_FLAGS(0),
            )
        }
        .map_err(|error| crypto_error("exporting a test key", &error))?;
        bytes.truncate(length as usize);
        Ok(bytes)
    }

    fn base64(bytes: &[u8]) -> Result<String, UpdateError> {
        let flags = CRYPT_STRING(CRYPT_STRING_BASE64.0 | CRYPT_STRING_NOCRLF);
        let mut length = 0u32;
        unsafe { CryptBinaryToStringW(bytes, flags, None, &mut length) }
            .ok()
            .map_err(|error| crypto_error("sizing a test key's text", &error))?;
        let mut wide = vec![0u16; length as usize];
        unsafe {
            CryptBinaryToStringW(
                bytes,
                flags,
                Some(windows::core::PWSTR(wide.as_mut_ptr())),
                &mut length,
            )
        }
        .ok()
        .map_err(|error| crypto_error("encoding a test key's text", &error))?;
        wide.truncate(length as usize);
        String::from_utf16(&wide).map_err(|_| UpdateError::Crypto {
            operation: "encoding a test key's text",
            status: 0,
        })
    }
}

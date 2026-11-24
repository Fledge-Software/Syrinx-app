//! The workflow's key import and deletion (#55), on fresh throwaway keys under unique names.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cng;
use crate::error::UpdateError;
use crate::ncrypt::test_keys::{
    p256_secret_text, p384_secret_text, point_of, private_half_exports,
};
use crate::{
    FileEntry, KeySet, KeyStorage, Manifest, PackagePath, PinnedKey, SECONDS_PER_DAY,
    StatementTerms, StoredKey, Timestamp, Version, delete_software_key, import_software_key,
    sign_release,
};

/// A unique key name that is deleted from the software key store when the test ends.
struct ThrowawayName(String);

impl ThrowawayName {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        Self(format!(
            "syrinx-update-test-key-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
}

impl Drop for ThrowawayName {
    fn drop(&mut self) {
        // A failed test may leave its key behind; deleting an absent key is not an error.
        if let Err(error) = delete_software_key(&self.0) {
            eprintln!("could not delete test key {}: {error}", self.0);
        }
    }
}

/// A canonical one-file manifest to sign.
fn manifest_bytes() -> Result<Vec<u8>, UpdateError> {
    let contents = b"synthetic executable";
    let path = PackagePath::parse("syrinx.exe").map_err(|problem| UpdateError::Manifest {
        line: 0,
        problem: crate::ManifestProblem::Path(problem),
    })?;
    let entry = FileEntry::new(path, contents.len() as u64, cng::sha256(contents)?);
    Ok(Manifest::new(Version::new(0, 9, 0), Version::new(0, 0, 0), vec![entry])?.to_bytes())
}

fn terms() -> Result<StatementTerms, UpdateError> {
    let issued = Timestamp::now()?;
    let expires = issued
        .checked_add_seconds(SECONDS_PER_DAY)
        .ok_or(UpdateError::Clock)?;
    Ok(StatementTerms {
        serial: 1,
        issued,
        expires,
    })
}

#[test]
fn an_imported_key_signs_cannot_be_exported_and_is_deleted() -> Result<(), UpdateError> {
    let secret = p256_secret_text()?;
    let expected = point_of(&secret)?;
    let name = ThrowawayName::new();

    assert_eq!(import_software_key(&name.0, &secret)?, expected);
    let stored = StoredKey::open(KeyStorage::Software, &name.0)?;
    let pinned = KeySet::new(&[PinnedKey::new(expected)])?;
    sign_release(&manifest_bytes()?, &stored, &pinned, terms()?)?;
    drop(stored);
    assert!(!private_half_exports(&name.0)?);

    assert!(delete_software_key(&name.0)?);
    assert!(StoredKey::open(KeyStorage::Software, &name.0).is_err());
    assert!(!delete_software_key(&name.0)?);
    Ok(())
}

#[test]
fn an_existing_key_is_never_replaced() -> Result<(), UpdateError> {
    let first = p256_secret_text()?;
    let second = p256_secret_text()?;
    let name = ThrowawayName::new();

    let point = import_software_key(&name.0, &first)?;
    assert!(import_software_key(&name.0, &second).is_err());
    let stored = StoredKey::open(KeyStorage::Software, &name.0)?;
    assert_eq!(crate::Signer::public_point(&stored)?, point);
    Ok(())
}

#[test]
fn unusable_key_text_is_refused_and_stores_nothing() -> Result<(), UpdateError> {
    let name = ThrowawayName::new();
    for text in ["", "not base64 at all!", "AAAA"] {
        assert!(import_software_key(&name.0, text).is_err(), "{text:?}");
        assert!(!delete_software_key(&name.0)?, "{text:?}");
    }
    let oversized = "A".repeat(8192);
    assert!(matches!(
        import_software_key(&name.0, &oversized),
        Err(UpdateError::TooLarge { .. })
    ));

    // A P-384 key imports but is not a release key, so it is deleted again.
    assert!(import_software_key(&name.0, &p384_secret_text()?).is_err());
    assert!(!delete_software_key(&name.0)?);
    Ok(())
}

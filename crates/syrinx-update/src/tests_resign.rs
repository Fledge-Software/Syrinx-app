//! The weekly re-sign (#55): verify the published release first, then sign only a new statement.

use std::cell::Cell;

use crate::cng;
use crate::cng::test_keys::TestKeyPair;
use crate::error::UpdateError;
use crate::{
    FileEntry, KeySet, Manifest, PackagePath, PinnedKey, PublishedRelease, SECONDS_PER_DAY,
    SignedRelease, Signer, StatementTerms, Timestamp, Version, resign_statement, sign_release,
    verify_published, verify_statement,
};

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).expect("test timestamps are canonical")
}

fn terms(serial: u64, issued: Timestamp) -> StatementTerms {
    StatementTerms {
        serial,
        issued,
        expires: issued
            .checked_add_seconds(30 * SECONDS_PER_DAY)
            .expect("in range"),
    }
}

/// A signer that records whether it was ever asked to sign.
struct Watched<'a> {
    inner: &'a TestKeyPair,
    signed: Cell<bool>,
}

impl Signer for Watched<'_> {
    fn public_point(&self) -> Result<[u8; 64], UpdateError> {
        self.inner.public_point()
    }

    fn sign_digest(&self, digest: &[u8; 32]) -> Result<[u8; 64], UpdateError> {
        self.signed.set(true);
        self.inner.sign(digest)
    }
}

/// A key, the set pinning it, and a release it signed on 2026-11-05.
struct Released {
    key: TestKeyPair,
    pinned: KeySet,
    manifest: Vec<u8>,
    signed: SignedRelease,
}

fn released() -> Result<Released, UpdateError> {
    let key = TestKeyPair::generate()?;
    let pinned = KeySet::new(&[PinnedKey::new(key.public_point()?)])?;
    let contents = b"synthetic executable";
    let path = PackagePath::parse("syrinx.exe").map_err(|problem| UpdateError::Manifest {
        line: 0,
        problem: crate::ManifestProblem::Path(problem),
    })?;
    let entry = FileEntry::new(path, contents.len() as u64, cng::sha256(contents)?);
    let manifest =
        Manifest::new(Version::new(0, 7, 0), Version::new(0, 0, 0), vec![entry])?.to_bytes();
    let signed = sign_release(
        &manifest,
        &key,
        &pinned,
        terms(100, at("2026-11-05T15:00:00Z")),
    )?;
    Ok(Released {
        key,
        pinned,
        manifest,
        signed,
    })
}

impl Released {
    fn published<'a>(&'a self, manifest: &'a [u8]) -> PublishedRelease<'a> {
        PublishedRelease {
            manifest,
            manifest_signature: &self.signed.manifest_signature,
            statement: &self.signed.statement,
            statement_signature: &self.signed.statement_signature,
        }
    }

    fn watched(&self) -> Watched<'_> {
        Watched {
            inner: &self.key,
            signed: Cell::new(false),
        }
    }
}

#[test]
fn a_resign_issues_only_a_new_statement_for_the_same_manifest() -> Result<(), UpdateError> {
    let release = released()?;
    // Two months later the old statement has expired; it is still verified, apart from that.
    let later = at("2027-01-05T15:00:00Z");
    let previous = verify_published(&release.published(&release.manifest), &release.pinned)?;
    assert_eq!(previous.serial(), 100);

    let renewed = resign_statement(
        &release.published(&release.manifest),
        &release.key,
        &release.pinned,
        terms(101, later),
    )?;
    let verified = verify_statement(
        &renewed.statement,
        &renewed.statement_signature,
        &release.pinned,
        later,
        None,
    )?;
    assert_eq!(verified.statement().serial(), 101);
    assert_eq!(
        *verified.statement().manifest_sha256(),
        cng::sha256(&release.manifest)?
    );
    // The old manifest signature still carries the manifest.
    verified.verify_manifest(
        &release.manifest,
        &release.signed.manifest_signature,
        &release.pinned,
        Version::new(0, 6, 0),
    )?;
    Ok(())
}

#[test]
fn a_tampered_manifest_is_refused_before_anything_is_signed() -> Result<(), UpdateError> {
    let release = released()?;
    let mut tampered = release.manifest.clone();
    let last = tampered.len() - 2;
    tampered[last] ^= 0x01;

    assert!(verify_published(&release.published(&tampered), &release.pinned).is_err());
    let watched = release.watched();
    let result = resign_statement(
        &release.published(&tampered),
        &watched,
        &release.pinned,
        terms(101, at("2026-11-12T15:00:00Z")),
    );
    assert!(result.is_err());
    assert!(!watched.signed.get(), "the key was asked to sign");
    Ok(())
}

#[test]
fn a_release_the_pinned_keys_did_not_sign_is_refused() -> Result<(), UpdateError> {
    let release = released()?;
    let stranger = TestKeyPair::generate()?;
    let other = KeySet::new(&[PinnedKey::new(stranger.public_point()?)])?;
    assert_eq!(
        verify_published(&release.published(&release.manifest), &other).map(drop),
        Err(UpdateError::UnknownKey)
    );

    // A statement re-signed by a stranger over the same bytes is refused too.
    let both = KeySet::new(&[
        PinnedKey::new(release.key.public_point()?),
        PinnedKey::new(stranger.public_point()?),
    ])?;
    let forged = sign_release(
        &release.manifest,
        &stranger,
        &both,
        terms(100, at("2026-11-05T15:00:00Z")),
    )?;
    let mixed = PublishedRelease {
        statement_signature: &forged.statement_signature,
        ..release.published(&release.manifest)
    };
    assert!(verify_published(&mixed, &release.pinned).is_err());
    Ok(())
}

#[test]
fn a_resign_never_lowers_or_repeats_the_serial() -> Result<(), UpdateError> {
    let release = released()?;
    for serial in [99, 100] {
        let watched = release.watched();
        let result = resign_statement(
            &release.published(&release.manifest),
            &watched,
            &release.pinned,
            terms(serial, at("2026-11-12T15:00:00Z")),
        );
        assert!(
            matches!(result, Err(UpdateError::StatementReplayed { .. })),
            "{serial}"
        );
        assert!(!watched.signed.get(), "{serial}");
    }
    Ok(())
}

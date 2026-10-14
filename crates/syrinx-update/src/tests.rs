//! Tampering tests on synthetic packages signed by keys generated in-process.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cng;
use crate::cng::test_keys::TestKeyPair;
use crate::error::{ManifestProblem, PathProblem, UpdateError};
use crate::{
    DetachedSignature, DirectoryPayload, FileEntry, KeyId, KeySet, MAX_FILE_BYTES, MAX_FILES,
    MAX_MANIFEST_BYTES, Manifest, PackagePath, Payload, PinnedKey, SIGNATURE_BYTES, SignatureKind,
    VerifiedManifest, Version, read_bounded, verify_manifest,
};

const INSTALLED: Version = Version::new(1, 2, 0);
const OFFERED: Version = Version::new(1, 3, 0);

/// Payload files held in memory, standing in for P1-39's staging directory.
#[derive(Clone)]
struct MemoryPayload {
    files: BTreeMap<String, Vec<u8>>,
}

impl Payload for MemoryPayload {
    type File = Cursor<Vec<u8>>;

    fn names(&mut self) -> Result<Vec<String>, UpdateError> {
        Ok(self.files.keys().cloned().collect())
    }

    fn open(&mut self, path: &PackagePath) -> Result<Option<Self::File>, UpdateError> {
        Ok(self.files.get(path.as_str()).cloned().map(Cursor::new))
    }
}

/// A complete signed package plus the key set that trusts its signer.
struct Package {
    signer: TestKeyPair,
    keys: KeySet,
    manifest: Vec<u8>,
    signature: Vec<u8>,
    payload: MemoryPayload,
}

impl Package {
    fn verify(&self) -> Result<VerifiedManifest, UpdateError> {
        verify_manifest(&self.manifest, &self.signature, &self.keys, INSTALLED)
    }

    /// Replace the manifest with arbitrary bytes, correctly signed by the same key.
    fn resign(&mut self, manifest: Vec<u8>) -> Result<(), UpdateError> {
        self.signature = sign(&self.signer, &manifest)?;
        self.manifest = manifest;
        Ok(())
    }
}

fn synthetic_files() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("syrinx.exe".to_owned(), b"synthetic executable".repeat(500)),
        ("resources/notes.txt".to_owned(), b"made-up text\n".to_vec()),
        ("resources/empty.bin".to_owned(), Vec::new()),
    ])
}

fn entries_for(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<FileEntry>, UpdateError> {
    files
        .iter()
        .map(|(path, bytes)| {
            let path = PackagePath::parse(path).map_err(|problem| UpdateError::Manifest {
                line: 0,
                problem: ManifestProblem::Path(problem),
            })?;
            Ok(FileEntry::new(
                path,
                bytes.len() as u64,
                cng::sha256(bytes)?,
            ))
        })
        .collect()
}

fn sign(signer: &TestKeyPair, manifest: &[u8]) -> Result<Vec<u8>, UpdateError> {
    let point = signer.public_point()?;
    let value = signer.sign(&cng::sha256(manifest)?)?;
    Ok(
        DetachedSignature::new(SignatureKind::Manifest, KeyId::of(&point)?, value)
            .to_bytes()
            .to_vec(),
    )
}

fn package_with(
    version: Version,
    minimum: Version,
    files: BTreeMap<String, Vec<u8>>,
) -> Result<Package, UpdateError> {
    let signer = TestKeyPair::generate()?;
    let keys = KeySet::new(&[PinnedKey::new(signer.public_point()?)])?;
    let manifest = Manifest::new(version, minimum, entries_for(&files)?)?.to_bytes();
    let signature = sign(&signer, &manifest)?;
    Ok(Package {
        signer,
        keys,
        manifest,
        signature,
        payload: MemoryPayload { files },
    })
}

fn valid_package() -> Result<Package, UpdateError> {
    package_with(OFFERED, Version::new(1, 0, 0), synthetic_files())
}

fn manifest_problem(result: Result<impl Sized, UpdateError>) -> Option<ManifestProblem> {
    match result {
        Err(UpdateError::Manifest { problem, .. }) => Some(problem),
        _ => None,
    }
}

/// Verify a payload, discarding the handles it would install from.
fn check_payload(verified: &VerifiedManifest, payload: impl Payload) -> Result<(), UpdateError> {
    verified.verify_payload(payload).map(drop)
}

fn hex(byte: char) -> String {
    std::iter::repeat_n(byte, 64).collect()
}

#[test]
fn valid_package_verifies() -> Result<(), UpdateError> {
    let package = valid_package()?;
    let verified = package.verify()?;
    assert_eq!(verified.manifest().version(), OFFERED);
    assert_eq!(verified.manifest().files().len(), 3);
    check_payload(&verified, package.payload.clone())
}

#[test]
fn sha256_matches_the_standard_test_vector() -> Result<(), UpdateError> {
    let mut text = String::new();
    crate::manifest::push_hex(&mut text, &cng::sha256(b"abc")?);
    assert_eq!(
        text,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    Ok(())
}

#[test]
fn every_flipped_manifest_bit_is_refused() -> Result<(), UpdateError> {
    let package = valid_package()?;
    for index in 0..package.manifest.len() {
        let mut manifest = package.manifest.clone();
        manifest[index] ^= 0x01;
        let result = verify_manifest(&manifest, &package.signature, &package.keys, INSTALLED);
        assert_eq!(
            result.err(),
            Some(UpdateError::BadSignature),
            "byte {index}"
        );
    }
    Ok(())
}

#[test]
fn every_flipped_signature_byte_is_refused() -> Result<(), UpdateError> {
    let package = valid_package()?;
    for index in 0..SIGNATURE_BYTES {
        let mut signature = package.signature.clone();
        signature[index] ^= 0x80;
        let error = verify_manifest(&package.manifest, &signature, &package.keys, INSTALLED).err();
        let expected = match index {
            0..4 => UpdateError::SignatureFormat,
            4..12 => UpdateError::UnknownKey,
            _ => UpdateError::BadSignature,
        };
        assert_eq!(error, Some(expected), "byte {index}");
    }
    Ok(())
}

#[test]
fn a_flipped_payload_byte_is_refused() -> Result<(), UpdateError> {
    let package = valid_package()?;
    let verified = package.verify()?;
    for path in ["syrinx.exe", "resources/notes.txt"] {
        let mut payload = package.payload.clone();
        if let Some(bytes) = payload.files.get_mut(path) {
            bytes[0] ^= 0x01;
        }
        assert_eq!(
            check_payload(&verified, payload),
            Err(UpdateError::HashMismatch {
                path: path.to_owned()
            })
        );
    }
    Ok(())
}

#[test]
fn a_key_that_is_not_pinned_is_refused() -> Result<(), UpdateError> {
    let package = valid_package()?;
    let other = TestKeyPair::generate()?;
    let other_point = other.public_point()?;
    let other_keys = KeySet::new(&[PinnedKey::new(other_point)])?;

    // The signer is not pinned at all.
    let result = verify_manifest(
        &package.manifest,
        &package.signature,
        &other_keys,
        INSTALLED,
    );
    assert_eq!(result.err(), Some(UpdateError::UnknownKey));

    // The signature claims the pinned key's identifier but was made by another key.
    let forged = DetachedSignature::new(
        SignatureKind::Manifest,
        KeyId::of(&other_point)?,
        package.signer.sign(&cng::sha256(&package.manifest)?)?,
    );
    let result = verify_manifest(
        &package.manifest,
        &forged.to_bytes(),
        &other_keys,
        INSTALLED,
    );
    assert_eq!(result.err(), Some(UpdateError::BadSignature));
    Ok(())
}

#[test]
fn the_production_placeholder_trusts_nothing() -> Result<(), UpdateError> {
    let package = valid_package()?;
    let production = KeySet::production()?;
    let result = verify_manifest(
        &package.manifest,
        &package.signature,
        &production,
        INSTALLED,
    );
    assert_eq!(result.err(), Some(UpdateError::UnknownKey));
    Ok(())
}

#[test]
fn invalid_and_duplicate_pinned_keys_are_refused() -> Result<(), UpdateError> {
    let zero = KeySet::new(&[PinnedKey::new([0u8; 64])]).err();
    assert!(matches!(zero, Some(UpdateError::InvalidKey { status }) if status < 0));
    let point = TestKeyPair::generate()?.public_point()?;
    let mut off_curve = point;
    off_curve[63] ^= 0x01;
    let off_curve = KeySet::new(&[PinnedKey::new(off_curve)]).err();
    assert!(matches!(off_curve, Some(UpdateError::InvalidKey { status }) if status < 0));
    let duplicate = [PinnedKey::new(point), PinnedKey::new(point)];
    assert_eq!(
        KeySet::new(&duplicate).err(),
        Some(UpdateError::DuplicateKey)
    );
    Ok(())
}

#[test]
fn truncation_is_refused() -> Result<(), UpdateError> {
    let mut package = valid_package()?;

    // A truncated manifest no longer matches its signature.
    let short = &package.manifest[..package.manifest.len() - 1];
    let result = verify_manifest(short, &package.signature, &package.keys, INSTALLED);
    assert_eq!(result.err(), Some(UpdateError::BadSignature));

    // A truncated signature has the wrong length.
    let result = verify_manifest(
        &package.manifest,
        &package.signature[..SIGNATURE_BYTES - 1],
        &package.keys,
        INSTALLED,
    );
    assert_eq!(
        result.err(),
        Some(UpdateError::SignatureLength {
            actual: SIGNATURE_BYTES - 1
        })
    );

    // A truncated payload file has the wrong size.
    let verified = package.verify()?;
    let mut payload = package.payload.clone();
    if let Some(bytes) = payload.files.get_mut("syrinx.exe") {
        bytes.pop();
    }
    assert!(matches!(
        check_payload(&verified, payload),
        Err(UpdateError::SizeMismatch { .. })
    ));

    // Even a signed manifest cut mid-line is refused by the parser.
    let cut = package.manifest[..package.manifest.len() - 1].to_vec();
    package.resign(cut)?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::MissingFinalNewline)
    );
    Ok(())
}

#[test]
fn oversized_inputs_are_refused() -> Result<(), UpdateError> {
    let mut package = valid_package()?;

    // A manifest over the bound is refused before hashing or reading it whole.
    let huge = vec![b'a'; MAX_MANIFEST_BYTES + 1];
    let result = verify_manifest(&huge, &package.signature, &package.keys, INSTALLED);
    assert!(matches!(result, Err(UpdateError::TooLarge { .. })));
    let read = read_bounded(Cursor::new(huge), MAX_MANIFEST_BYTES, "manifest");
    assert!(matches!(read, Err(UpdateError::TooLarge { .. })));

    // A payload file longer than declared stops one byte past its size.
    let verified = package.verify()?;
    let mut payload = package.payload.clone();
    if let Some(bytes) = payload.files.get_mut("resources/notes.txt") {
        bytes.extend_from_slice(&[0u8; 4096]);
    }
    assert!(matches!(
        check_payload(&verified, payload),
        Err(UpdateError::SizeMismatch { .. })
    ));

    // Signed manifests with an oversized field are refused by the parser.
    let too_big = MAX_FILE_BYTES + 1;
    let text = format!(
        "syrinx-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\nfile {} {too_big} big.bin\n",
        hex('0')
    );
    package.resign(text.into_bytes())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Size)
    );

    let long_name = "a".repeat(65);
    let text = format!(
        "syrinx-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\nfile {} 1 {long_name}\n",
        hex('0')
    );
    package.resign(text.into_bytes())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Path(PathProblem::ComponentTooLong))
    );

    let version = "syrinx-update-manifest 1\nversion 4294967296.0.0\nminimum-version 1.0.0\n";
    package.resign(version.as_bytes().to_vec())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Version)
    );
    Ok(())
}

#[test]
fn too_many_files_are_refused() {
    let mut text = String::from("syrinx-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\n");
    for index in 0..=MAX_FILES {
        text.push_str(&format!("file {} 0 f{index:05}\n", hex('0')));
    }
    assert_eq!(
        manifest_problem(Manifest::parse(text.as_bytes())),
        Some(ManifestProblem::TooManyFiles)
    );
}

#[test]
fn downgrades_and_reinstalls_are_refused() -> Result<(), UpdateError> {
    let package = valid_package()?;
    for installed in [OFFERED, Version::new(1, 3, 1), Version::new(2, 0, 0)] {
        let result = verify_manifest(
            &package.manifest,
            &package.signature,
            &package.keys,
            installed,
        );
        assert_eq!(
            result.err(),
            Some(UpdateError::NotNewer {
                installed,
                offered: OFFERED
            })
        );
    }

    // A package that needs a newer starting point refuses an old installation.
    let package = package_with(OFFERED, Version::new(1, 2, 1), synthetic_files())?;
    assert_eq!(
        package.verify().err(),
        Some(UpdateError::InstalledTooOld {
            installed: INSTALLED,
            minimum: Version::new(1, 2, 1)
        })
    );
    Ok(())
}

#[test]
fn duplicate_file_entries_are_refused() -> Result<(), UpdateError> {
    let mut package = valid_package()?;
    let header = "syrinx-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\n";
    for files in [
        ["a.txt", "a.txt"],
        ["A.txt", "a.txt"],
        ["dir/File.bin", "dir/file.bin"],
    ] {
        let text = format!(
            "{header}file {zero} 0 {}\nfile {zero} 0 {}\n",
            files[0],
            files[1],
            zero = hex('0')
        );
        package.resign(text.into_bytes())?;
        assert_eq!(
            manifest_problem(package.verify()),
            Some(ManifestProblem::DuplicatePath),
            "{files:?}"
        );
    }
    Ok(())
}

#[test]
fn unsafe_paths_are_refused() -> Result<(), UpdateError> {
    let cases = [
        ("../evil.exe", PathProblem::DotComponent),
        ("resources/../../evil.exe", PathProblem::DotComponent),
        ("./syrinx.exe", PathProblem::DotComponent),
        ("/absolute.exe", PathProblem::EmptyComponent),
        ("trailing/", PathProblem::EmptyComponent),
        ("double//slash", PathProblem::EmptyComponent),
        ("", PathProblem::EmptyComponent),
        ("C:/Windows/evil.exe", PathProblem::Character),
        ("syrinx.exe:stream", PathProblem::Character),
        ("syrinx.exe::$DATA", PathProblem::Character),
        ("resources\\evil.exe", PathProblem::Character),
        ("space name.txt", PathProblem::Character),
        ("caf\u{e9}.txt", PathProblem::Character),
        ("evil.", PathProblem::TrailingDot),
        ("CON", PathProblem::DeviceName),
        ("dir/nul.txt", PathProblem::DeviceName),
        ("Com1.log", PathProblem::DeviceName),
        ("lpt9", PathProblem::DeviceName),
        ("a/b/c/d/e/f/g/h/i", PathProblem::TooDeep),
    ];
    for (path, expected) in cases {
        assert_eq!(PackagePath::parse(path), Err(expected), "{path:?}");
    }
    assert_eq!(
        PackagePath::parse(&"a/".repeat(121)),
        Err(PathProblem::TooLong)
    );
    for path in [
        "syrinx.exe",
        ".hidden",
        "com10.txt",
        "console.log",
        "a/b/c/d/e/f/g/h",
    ] {
        assert!(PackagePath::parse(path).is_ok(), "{path:?}");
    }

    // A signed manifest naming a traversal path is refused end to end.
    let mut package = valid_package()?;
    let text = format!(
        "syrinx-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\nfile {} 0 ../evil.exe\n",
        hex('0')
    );
    package.resign(text.into_bytes())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Path(PathProblem::DotComponent))
    );
    Ok(())
}

#[test]
fn non_canonical_manifests_are_refused() -> Result<(), UpdateError> {
    let mut package = valid_package()?;
    let canonical =
        String::from_utf8(package.manifest.clone()).map_err(|_| UpdateError::Manifest {
            line: 0,
            problem: ManifestProblem::Encoding,
        })?;
    let first_hash_line = canonical.lines().nth(3).unwrap_or_default().to_owned();
    let cases = [
        (canonical.replace('\n', "\r\n"), ManifestProblem::Encoding),
        (format!("\u{feff}{canonical}"), ManifestProblem::Header),
        (
            canonical.replace("version 1.3.0", "version 01.3.0"),
            ManifestProblem::Version,
        ),
        (
            canonical.replace("version 1.3.0", "version 1.3"),
            ManifestProblem::Version,
        ),
        (
            canonical.replace("version 1.3.0", "version  1.3.0"),
            ManifestProblem::Version,
        ),
        (
            canonical.replacen("file ", "file  ", 1),
            ManifestProblem::Hash,
        ),
        (
            canonical.replace(&first_hash_line, &first_hash_line.to_ascii_uppercase()),
            ManifestProblem::UnexpectedLine,
        ),
        (format!("{canonical}\n"), ManifestProblem::UnexpectedLine),
        (
            canonical.replace("syrinx-update-manifest 1", "syrinx-update-manifest 2"),
            ManifestProblem::Header,
        ),
        (
            canonical.replace("minimum-version 1.0.0", "minimum-version 1.4.0"),
            ManifestProblem::MinimumAboveVersion,
        ),
    ];
    for (text, expected) in cases {
        package.resign(text.clone().into_bytes())?;
        assert_eq!(
            manifest_problem(package.verify()),
            Some(expected),
            "{text:?}"
        );
    }

    // Uppercase hex inside an otherwise valid line, and unsorted paths.
    let header = "syrinx-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\n";
    let upper = format!("{header}file {} 0 a.txt\n", hex('A'));
    package.resign(upper.into_bytes())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Hash)
    );
    let unsorted = format!(
        "{header}file {zero} 0 b.txt\nfile {zero} 0 a.txt\n",
        zero = hex('0')
    );
    package.resign(unsorted.into_bytes())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Unsorted)
    );
    let leading_zero = format!("{header}file {} 01 a.txt\n", hex('0'));
    package.resign(leading_zero.into_bytes())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Size)
    );
    let no_files = header.to_owned();
    package.resign(no_files.into_bytes())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::NoFiles)
    );
    Ok(())
}

#[test]
fn unlisted_and_missing_payload_files_are_refused() -> Result<(), UpdateError> {
    let package = valid_package()?;
    let verified = package.verify()?;

    let mut extra = package.payload.clone();
    extra
        .files
        .insert("payload.dll".to_owned(), b"unlisted".to_vec());
    assert_eq!(
        check_payload(&verified, extra),
        Err(UpdateError::UnlistedFile {
            name: "payload.dll".to_owned()
        })
    );

    let mut missing = package.payload.clone();
    missing.files.remove("resources/notes.txt");
    assert_eq!(
        check_payload(&verified, missing),
        Err(UpdateError::MissingFile {
            path: "resources/notes.txt".to_owned()
        })
    );
    Ok(())
}

#[test]
fn versions_parse_only_in_canonical_form() {
    assert_eq!(
        Version::parse("0.10.4294967295"),
        Some(Version::new(0, 10, u32::MAX))
    );
    for text in [
        "",
        "1",
        "1.2",
        "1.2.3.4",
        "1.2.-3",
        "+1.2.3",
        "1.02.3",
        "1.2.3-beta",
        "1..3",
        "4294967296.0.0",
    ] {
        assert_eq!(Version::parse(text), None, "{text:?}");
    }
    assert!(Version::new(1, 10, 0) > Version::new(1, 9, 99));
    assert!(Version::new(2, 0, 0) > Version::new(1, 99, 99));
}

#[test]
fn listed_sizes_over_the_package_total_are_refused() {
    let mut text = String::from("syrinx-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\n");
    for index in 0..5 {
        text.push_str(&format!(
            "file {} {MAX_FILE_BYTES} part{index}.bin\n",
            hex('0')
        ));
    }
    assert_eq!(
        manifest_problem(Manifest::parse(text.as_bytes())),
        Some(ManifestProblem::TotalTooLarge)
    );
}

#[test]
fn a_signature_by_one_pinned_key_cannot_claim_another() -> Result<(), UpdateError> {
    let package = valid_package()?;
    let other = TestKeyPair::generate()?;
    let other_point = other.public_point()?;
    let signer_point = package.signer.public_point()?;
    let both = KeySet::new(&[PinnedKey::new(signer_point), PinnedKey::new(other_point)])?;

    // Each key's honest signature verifies under the two-key set.
    verify_manifest(&package.manifest, &package.signature, &both, INSTALLED)?;
    let honest_other = sign(&other, &package.manifest)?;
    verify_manifest(&package.manifest, &honest_other, &both, INSTALLED)?;

    // The signer's signature relabelled with the other key's identifier fails.
    let forged = DetachedSignature::new(
        SignatureKind::Manifest,
        KeyId::of(&other_point)?,
        package.signer.sign(&cng::sha256(&package.manifest)?)?,
    );
    let result = verify_manifest(&package.manifest, &forged.to_bytes(), &both, INSTALLED);
    assert_eq!(result.err(), Some(UpdateError::BadSignature));
    Ok(())
}

#[test]
fn a_newer_package_still_refuses_an_installation_below_its_minimum() -> Result<(), UpdateError> {
    let minimum = Version::new(1, 5, 0);
    let package = package_with(Version::new(2, 0, 0), minimum, synthetic_files())?;
    assert_eq!(
        package.verify().err(),
        Some(UpdateError::InstalledTooOld {
            installed: INSTALLED,
            minimum
        })
    );
    let at_minimum = verify_manifest(
        &package.manifest,
        &package.signature,
        &package.keys,
        minimum,
    );
    assert!(at_minimum.is_ok());
    Ok(())
}

#[test]
fn a_file_cannot_double_as_a_directory() {
    let header = "syrinx-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\n";
    for paths in [
        ["a", "a/b"],
        ["LIB/x.dll", "lib"],
        ["Docs/a.txt", "docs"],
        ["A", "a/b"],
    ] {
        let text = format!(
            "{header}file {zero} 0 {}\nfile {zero} 0 {}\n",
            paths[0],
            paths[1],
            zero = hex('0')
        );
        assert_eq!(
            manifest_problem(Manifest::parse(text.as_bytes())),
            Some(ManifestProblem::PathCollision),
            "{paths:?}"
        );
    }
    let siblings = format!(
        "{header}file {zero} 0 a/b\nfile {zero} 0 a/c\nfile {zero} 0 ab\n",
        zero = hex('0')
    );
    assert!(Manifest::parse(siblings.as_bytes()).is_ok());
}

/// A fresh, uniquely named synthetic staging area under the temporary directory.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let name = format!(
            "syrinx-update-test-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(name);
        fs::create_dir(&root)?;
        Ok(Self { root })
    }

    /// Write the synthetic files into `staging`, returning that directory.
    fn stage(&self, files: &BTreeMap<String, Vec<u8>>) -> Result<PathBuf, std::io::Error> {
        let staging = self.root.join("staging");
        fs::create_dir(&staging)?;
        for (name, bytes) in files {
            let path = staging.join(name.replace('/', "\\"));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, bytes)?;
        }
        Ok(staging)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Removal does not follow junctions; a failure leaves only synthetic files behind.
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("could not remove {}: {error}", self.root.display());
        }
    }
}

fn io_error(error: std::io::Error) -> UpdateError {
    UpdateError::Read {
        what: "test scratch".to_owned(),
        kind: error.kind(),
    }
}

#[test]
fn a_staging_directory_installs_from_held_deny_write_handles() -> Result<(), UpdateError> {
    let package = valid_package()?;
    let scratch = Scratch::new().map_err(io_error)?;
    let staging = scratch.stage(&synthetic_files()).map_err(io_error)?;
    let verified = package.verify()?;
    let mut payload = verified.verify_payload(DirectoryPayload::open(&staging)?)?;

    // The returned handles are rewound and hold exactly the verified bytes.
    let expected = synthetic_files();
    for file in payload.files_mut() {
        let path = file.entry().path().as_str().to_owned();
        let mut bytes = Vec::new();
        file.reader().read_to_end(&mut bytes).map_err(io_error)?;
        assert_eq!(Some(&bytes), expected.get(&path), "{path}");
    }

    // While they are held, no other handle can write the file or rename its directory.
    let write = OpenOptions::new()
        .write(true)
        .open(staging.join("syrinx.exe"));
    assert_eq!(write.err().and_then(|error| error.raw_os_error()), Some(32));
    let rename = fs::rename(staging.join("resources"), staging.join("moved"));
    assert!(rename.is_err());
    drop(payload);
    let write = OpenOptions::new()
        .write(true)
        .open(staging.join("syrinx.exe"));
    assert!(write.is_ok());
    Ok(())
}

#[test]
fn a_staging_directory_must_use_the_manifest_case_and_hold_nothing_else() -> Result<(), UpdateError>
{
    let package = valid_package()?;
    let verified = package.verify()?;

    let mut renamed = synthetic_files();
    if let Some(bytes) = renamed.remove("syrinx.exe") {
        renamed.insert("Syrinx.exe".to_owned(), bytes);
    }
    let scratch = Scratch::new().map_err(io_error)?;
    let staging = scratch.stage(&renamed).map_err(io_error)?;
    assert_eq!(
        check_payload(&verified, DirectoryPayload::open(&staging)?),
        Err(UpdateError::UnlistedFile {
            name: "Syrinx.exe".to_owned()
        })
    );

    let scratch = Scratch::new().map_err(io_error)?;
    let staging = scratch.stage(&synthetic_files()).map_err(io_error)?;
    fs::create_dir(staging.join("extra")).map_err(io_error)?;
    assert_eq!(
        check_payload(&verified, DirectoryPayload::open(&staging)?),
        Err(UpdateError::UnlistedFile {
            name: "extra".to_owned()
        })
    );
    Ok(())
}

#[test]
fn a_junction_in_a_staging_directory_is_refused() -> Result<(), UpdateError> {
    let package = valid_package()?;
    let verified = package.verify()?;
    let scratch = Scratch::new().map_err(io_error)?;

    // Put the real resources outside the staging directory and link them in with a junction.
    let mut files = synthetic_files();
    let outside = scratch.root.join("outside");
    fs::create_dir(&outside).map_err(io_error)?;
    for (name, leaf) in [
        ("resources/notes.txt", "notes.txt"),
        ("resources/empty.bin", "empty.bin"),
    ] {
        let bytes = files.remove(name).ok_or_else(|| UpdateError::MissingFile {
            path: name.to_owned(),
        })?;
        fs::write(outside.join(leaf), bytes).map_err(io_error)?;
    }
    let staging = scratch.stage(&files).map_err(io_error)?;
    let status = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(staging.join("resources"))
        .arg(&outside)
        .stdout(Stdio::null())
        .status()
        .map_err(io_error)?;
    assert!(status.success(), "mklink /J failed");

    assert_eq!(
        check_payload(&verified, DirectoryPayload::open(&staging)?),
        Err(UpdateError::UnsafeEntry {
            name: "resources".to_owned()
        })
    );
    Ok(())
}

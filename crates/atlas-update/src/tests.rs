//! Tampering tests on synthetic packages signed by keys generated in-process.

use std::collections::BTreeMap;
use std::io::{self, Cursor};

use crate::cng;
use crate::cng::test_keys::TestKeyPair;
use crate::error::{ManifestProblem, PathProblem, UpdateError};
use crate::{
    DetachedSignature, FileEntry, KeyId, KeySet, MAX_FILE_BYTES, MAX_FILES, MAX_MANIFEST_BYTES,
    Manifest, PackagePath, Payload, PinnedKey, SIGNATURE_BYTES, VerifiedManifest, Version,
    read_bounded, verify_manifest,
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

    fn names(&mut self) -> io::Result<Vec<String>> {
        Ok(self.files.keys().cloned().collect())
    }

    fn open(&mut self, path: &str) -> io::Result<Option<Self::File>> {
        Ok(self.files.get(path).cloned().map(Cursor::new))
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
        ("atlas.exe".to_owned(), b"synthetic executable".repeat(500)),
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
    Ok(DetachedSignature::new(KeyId::of(&point)?, value)
        .to_bytes()
        .to_vec())
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

fn hex(byte: char) -> String {
    std::iter::repeat_n(byte, 64).collect()
}

#[test]
fn valid_package_verifies() -> Result<(), UpdateError> {
    let package = valid_package()?;
    let verified = package.verify()?;
    assert_eq!(verified.manifest().version(), OFFERED);
    assert_eq!(verified.manifest().files().len(), 3);
    verified.verify_payload(&mut package.payload.clone())
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
    for path in ["atlas.exe", "resources/notes.txt"] {
        let mut payload = package.payload.clone();
        if let Some(bytes) = payload.files.get_mut(path) {
            bytes[0] ^= 0x01;
        }
        assert_eq!(
            verified.verify_payload(&mut payload),
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
    assert_eq!(
        KeySet::new(&[PinnedKey::new([0u8; 64])]).err(),
        Some(UpdateError::InvalidKey)
    );
    let point = TestKeyPair::generate()?.public_point()?;
    let mut off_curve = point;
    off_curve[63] ^= 0x01;
    assert_eq!(
        KeySet::new(&[PinnedKey::new(off_curve)]).err(),
        Some(UpdateError::InvalidKey)
    );
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
    if let Some(bytes) = payload.files.get_mut("atlas.exe") {
        bytes.pop();
    }
    assert!(matches!(
        verified.verify_payload(&mut payload),
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
        verified.verify_payload(&mut payload),
        Err(UpdateError::SizeMismatch { .. })
    ));

    // Signed manifests with an oversized field are refused by the parser.
    let too_big = MAX_FILE_BYTES + 1;
    let text = format!(
        "atlas-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\nfile {} {too_big} big.bin\n",
        hex('0')
    );
    package.resign(text.into_bytes())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Size)
    );

    let long_name = "a".repeat(65);
    let text = format!(
        "atlas-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\nfile {} 1 {long_name}\n",
        hex('0')
    );
    package.resign(text.into_bytes())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Path(PathProblem::ComponentTooLong))
    );

    let version = "atlas-update-manifest 1\nversion 4294967296.0.0\nminimum-version 1.0.0\n";
    package.resign(version.as_bytes().to_vec())?;
    assert_eq!(
        manifest_problem(package.verify()),
        Some(ManifestProblem::Version)
    );
    Ok(())
}

#[test]
fn too_many_files_are_refused() {
    let mut text = String::from("atlas-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\n");
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
    let header = "atlas-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\n";
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
        ("./atlas.exe", PathProblem::DotComponent),
        ("/absolute.exe", PathProblem::EmptyComponent),
        ("trailing/", PathProblem::EmptyComponent),
        ("double//slash", PathProblem::EmptyComponent),
        ("", PathProblem::EmptyComponent),
        ("C:/Windows/evil.exe", PathProblem::Character),
        ("atlas.exe:stream", PathProblem::Character),
        ("atlas.exe::$DATA", PathProblem::Character),
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
        "atlas.exe",
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
        "atlas-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\nfile {} 0 ../evil.exe\n",
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
            canonical.replace("atlas-update-manifest 1", "atlas-update-manifest 2"),
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
    let header = "atlas-update-manifest 1\nversion 1.3.0\nminimum-version 1.0.0\n";
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
        verified.verify_payload(&mut extra),
        Err(UpdateError::UnlistedFile {
            name: "payload.dll".to_owned()
        })
    );

    let mut missing = package.payload.clone();
    missing.files.remove("resources/notes.txt");
    assert_eq!(
        verified.verify_payload(&mut missing),
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

//! Tampering tests on synthetic plugin packages (#134), signed by keys generated in-process.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;

use crate::cng;
use crate::cng::test_keys::TestKeyPair;
use crate::error::{ManifestProblem, UpdateError};
use crate::tests::{Scratch, io_error};
use crate::{
    DetachedSignature, DirectoryPayload, FileEntry, KeyId, KeySet, Manifest, PACKAGE_FILE,
    PACKAGE_SIGNATURE_FILE, PackagePath, Payload, PinnedKey, PluginPackageList, SignatureKind,
    VerifiedPluginPackage, Version, verify_manifest, verify_plugin_package,
};

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";
const VERSION: Version = Version::new(0, 3, 0);

/// Plugin files held in memory, standing in for a package folder.
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

/// A synthetic plugin's files: the component, its manifest and interface files.
fn plugin_files() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        (
            "plugin.wasm".to_owned(),
            b"\0asm synthetic component".repeat(64),
        ),
        (
            "syrinx-plugin.toml".to_owned(),
            b"schema = 1\n# synthetic manifest\n".to_vec(),
        ),
        ("ui/index.html".to_owned(), b"<p>synthetic</p>\n".to_vec()),
        ("ui/app.js".to_owned(), b"// synthetic\n".to_vec()),
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

fn sign(signer: &TestKeyPair, kind: SignatureKind, bytes: &[u8]) -> Result<Vec<u8>, UpdateError> {
    let point = signer.public_point()?;
    let value = signer.sign_message(bytes)?;
    Ok(DetachedSignature::new(kind, KeyId::of(&point)?, value)
        .to_bytes()
        .to_vec())
}

/// A signed package: its `package` list, `package.sig`, files and the keys that trust it.
struct Package {
    id: String,
    signer: TestKeyPair,
    keys: KeySet,
    list: Vec<u8>,
    signature: Vec<u8>,
    payload: MemoryPayload,
}

impl Package {
    fn new(id: &str, version: Version) -> Result<Self, UpdateError> {
        let signer = TestKeyPair::generate()?;
        let keys = KeySet::new(&[PinnedKey::new(signer.public_point()?)])?;
        let files = plugin_files();
        let list = PluginPackageList::new(id, version, WORLD, entries_for(&files)?)?.to_bytes();
        let signature = sign(&signer, SignatureKind::PluginPackage, &list)?;
        Ok(Self {
            id: id.to_owned(),
            signer,
            keys,
            list,
            signature,
            payload: MemoryPayload { files },
        })
    }

    /// Verify as a first install into the package's own slot.
    fn verify(&self) -> Result<VerifiedPluginPackage, UpdateError> {
        self.verify_into(&self.id, None)
    }

    fn verify_into(
        &self,
        slot: &str,
        installed: Option<Version>,
    ) -> Result<VerifiedPluginPackage, UpdateError> {
        verify_plugin_package(&self.list, &self.signature, &self.keys, slot, installed)
    }

    /// Replace the list with arbitrary bytes, correctly signed by the same key.
    fn resign(&mut self, list: Vec<u8>) -> Result<(), UpdateError> {
        self.signature = sign(&self.signer, SignatureKind::PluginPackage, &list)?;
        self.list = list;
        Ok(())
    }
}

fn check_payload(
    verified: &VerifiedPluginPackage,
    payload: MemoryPayload,
) -> Result<(), UpdateError> {
    verified.verify_payload(payload).map(drop)
}

fn problem(result: Result<impl Sized, UpdateError>) -> Option<ManifestProblem> {
    match result {
        Err(UpdateError::Manifest { problem, .. }) => Some(problem),
        _ => None,
    }
}

#[test]
fn a_signed_package_verifies_with_its_id_version_and_world() -> Result<(), UpdateError> {
    let package = Package::new("notes", VERSION)?;
    let verified = package.verify()?;
    assert_eq!(verified.list().id(), "notes");
    assert_eq!(verified.list().version(), VERSION);
    assert_eq!(verified.list().world(), WORLD);
    let text = String::from_utf8_lossy(&package.list);
    assert!(text.starts_with("syrinx-plugin-package 1\nid notes\nversion 0.3.0\nworld "));
    check_payload(&verified, package.payload.clone())
}

#[test]
fn a_tampered_byte_in_the_component_ui_or_manifest_is_refused() -> Result<(), UpdateError> {
    let package = Package::new("notes", VERSION)?;
    let verified = package.verify()?;
    for path in ["plugin.wasm", "ui/app.js", "syrinx-plugin.toml"] {
        let mut payload = package.payload.clone();
        if let Some(bytes) = payload.files.get_mut(path) {
            bytes[0] ^= 0x01;
        }
        assert_eq!(
            check_payload(&verified, payload),
            Err(UpdateError::HashMismatch {
                path: path.to_owned()
            }),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn a_tampered_package_list_fails_its_signature_before_parsing() -> Result<(), UpdateError> {
    let package = Package::new("notes", VERSION)?;
    for index in 0..package.list.len() {
        let mut list = package.list.clone();
        list[index] ^= 0x01;
        assert_eq!(
            verify_plugin_package(&list, &package.signature, &package.keys, "notes", None).err(),
            Some(UpdateError::BadSignature),
            "byte {index}"
        );
    }
    Ok(())
}

#[test]
fn an_unlisted_file_is_refused_but_the_signed_files_are_not() -> Result<(), UpdateError> {
    let package = Package::new("notes", VERSION)?;
    let verified = package.verify()?;

    // The folder's own `package` and `package.sig` are expected beside the listed files.
    let mut with_signed_files = package.payload.clone();
    with_signed_files
        .files
        .insert(PACKAGE_FILE.to_owned(), package.list.clone());
    with_signed_files
        .files
        .insert(PACKAGE_SIGNATURE_FILE.to_owned(), package.signature.clone());
    check_payload(&verified, with_signed_files)?;

    for extra in ["ui/extra.js", "plugin.cwasm", "ui/package"] {
        let mut payload = package.payload.clone();
        payload
            .files
            .insert(extra.to_owned(), b"synthetic".to_vec());
        assert_eq!(
            check_payload(&verified, payload),
            Err(UpdateError::UnlistedFile {
                name: extra.to_owned()
            }),
            "{extra}"
        );
    }
    Ok(())
}

#[test]
fn a_package_renamed_into_another_ids_slot_is_refused() -> Result<(), UpdateError> {
    let package = Package::new("notes", VERSION)?;
    package.verify_into("notes", None)?;
    assert_eq!(
        package.verify_into("forge", None).err(),
        Some(UpdateError::WrongPlugin {
            expected: "forge".to_owned(),
            signed: "notes".to_owned()
        })
    );
    Ok(())
}

#[test]
fn an_older_or_equal_version_is_refused() -> Result<(), UpdateError> {
    let package = Package::new("notes", VERSION)?;
    package.verify_into("notes", Some(Version::new(0, 2, 9)))?;
    for installed in [VERSION, Version::new(0, 3, 1), Version::new(1, 0, 0)] {
        assert_eq!(
            package.verify_into("notes", Some(installed)).err(),
            Some(UpdateError::NotNewer {
                installed,
                offered: VERSION
            }),
            "{installed}"
        );
    }
    Ok(())
}

#[test]
fn a_listed_cwasm_or_signed_file_is_refused_even_when_signed() -> Result<(), UpdateError> {
    let mut package = Package::new("notes", VERSION)?;
    for reserved in ["plugin.cwasm", "lib/Native.CWASM", "package", "Package.sig"] {
        let text = format!(
            "syrinx-plugin-package 1\nid notes\nversion 0.3.0\nworld {WORLD}\nfile {} 1 {reserved}\n",
            "a".repeat(64)
        );
        package.resign(text.into_bytes())?;
        assert_eq!(
            problem(package.verify()),
            Some(ManifestProblem::ReservedPath),
            "{reserved}"
        );
    }
    Ok(())
}

#[test]
fn malformed_identity_lines_are_named() -> Result<(), UpdateError> {
    let mut package = Package::new("notes", VERSION)?;
    let file = format!("file {} 1 plugin.wasm\n", "a".repeat(64));
    for (header, expected) in [
        (
            "id Notes\nversion 0.3.0\nworld syrinx:plugin/plugin@0.1.0\n",
            ManifestProblem::PluginId,
        ),
        (
            "id notes.extra\nversion 0.3.0\nworld syrinx:plugin/plugin@0.1.0\n",
            ManifestProblem::PluginId,
        ),
        (
            "id notes\nversion 0.3\nworld syrinx:plugin/plugin@0.1.0\n",
            ManifestProblem::Version,
        ),
        (
            "id notes\nversion 0.3.0\nworld other:plugin/plugin@0.1.0\n",
            ManifestProblem::World,
        ),
        (
            "version 0.3.0\nid notes\nworld syrinx:plugin/plugin@0.1.0\n",
            ManifestProblem::Field("id"),
        ),
    ] {
        package.resign(format!("syrinx-plugin-package 1\n{header}{file}").into_bytes())?;
        assert_eq!(problem(package.verify()), Some(expected), "{header}");
    }
    Ok(())
}

#[test]
fn update_manifests_and_plugin_lists_never_pass_for_each_other() -> Result<(), UpdateError> {
    let package = Package::new("notes", VERSION)?;

    // An update manifest signed as a plugin list fails the plugin header, and the reverse.
    let manifest = Manifest::new(
        Version::new(1, 3, 0),
        Version::new(1, 0, 0),
        entries_for(&plugin_files())?,
    )?
    .to_bytes();
    let as_plugin = sign(&package.signer, SignatureKind::PluginPackage, &manifest)?;
    assert_eq!(
        problem(verify_plugin_package(
            &manifest,
            &as_plugin,
            &package.keys,
            "notes",
            None
        )),
        Some(ManifestProblem::Header)
    );
    let as_update = sign(&package.signer, SignatureKind::Manifest, &package.list)?;
    assert_eq!(
        problem(verify_manifest(
            &package.list,
            &as_update,
            &package.keys,
            Version::new(1, 0, 0)
        )),
        Some(ManifestProblem::Header)
    );

    // An update signature's magic is refused before any key is looked up.
    let update_magic = sign(&package.signer, SignatureKind::Manifest, &package.list)?;
    assert_eq!(
        verify_plugin_package(&package.list, &update_magic, &package.keys, "notes", None).err(),
        Some(UpdateError::SignatureFormat)
    );
    Ok(())
}

#[test]
fn a_package_signed_by_an_unpinned_key_is_refused() -> Result<(), UpdateError> {
    let package = Package::new("notes", VERSION)?;
    let stranger = Package::new("notes", VERSION)?;
    assert_eq!(
        verify_plugin_package(
            &package.list,
            &package.signature,
            &stranger.keys,
            "notes",
            None
        )
        .err(),
        Some(UpdateError::UnknownKey)
    );
    Ok(())
}

#[test]
fn a_package_folder_verifies_from_held_handles() -> Result<(), UpdateError> {
    let package = Package::new("notes", VERSION)?;
    let verified = package.verify()?;
    let scratch = Scratch::new().map_err(io_error)?;
    let folder = scratch.stage(&package.payload.files).map_err(io_error)?;
    fs::write(folder.join(PACKAGE_FILE), &package.list).map_err(io_error)?;
    fs::write(folder.join(PACKAGE_SIGNATURE_FILE), &package.signature).map_err(io_error)?;
    let mut payload = verified.verify_payload(DirectoryPayload::open(&folder)?)?;
    assert_eq!(payload.files_mut().len(), plugin_files().len());
    drop(payload);

    // A precompiled module dropped into the folder is an unlisted entry.
    fs::write(folder.join("plugin.cwasm"), b"synthetic native code").map_err(io_error)?;
    assert_eq!(
        verified
            .verify_payload(DirectoryPayload::open(&folder)?)
            .map(drop),
        Err(UpdateError::UnlistedFile {
            name: "plugin.cwasm".to_owned()
        })
    );
    Ok(())
}

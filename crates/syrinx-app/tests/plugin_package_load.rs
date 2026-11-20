//! A plugin package end to end (#134): the real full fixture and its manifest, signed with a
//! fresh test key, go through every check in the order Syrinx loads a package.

use std::fs;
use std::path::{Path, PathBuf};

use syrinx_core::CommandRegistry;
use syrinx_plugin_host::manifest::{DiscoveryOptions, MANIFEST_FILE, PluginManifest};
use syrinx_plugin_host::{
    IdentityMismatch, PluginRegistry, RegisteredPlugin, Runtime, SignedIdentity, check_signed,
};
use syrinx_update::{
    DetachedSignature, DirectoryPayload, FileEntry, KeyId, KeySet, PACKAGE_FILE,
    PACKAGE_SIGNATURE_FILE, PackagePath, PinnedKey, PluginPackageList, SignatureKind, TestKeyPair,
    UpdateError, VerifiedPluginPackage, Version, read_bounded, sha256, verify_plugin_package,
};

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";
const MODULE_FILE: &str = "plugin.wasm";
const READ_LIMIT: usize = 64 * 1024 * 1024;

#[path = "support/plugin_fixture.rs"]
mod plugin_fixture;

use plugin_fixture::fixture_workspace;

/// The full fixture's component, built once per run under this test's own target folder.
fn full_component() -> &'static [u8] {
    plugin_fixture::full_component(&Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-fixtures"))
}

/// The full fixture's real `syrinx-plugin.toml`, with its id replaced when a test needs to.
fn full_manifest(id: &str) -> Vec<u8> {
    let text = fs::read_to_string(fixture_workspace().join("full").join(MANIFEST_FILE))
        .expect("the fixture's manifest");
    text.replace("id = \"test\"", &format!("id = \"{id}\""))
        .replace("\"test.", &format!("\"{id}."))
        .into_bytes()
}

/// A package folder on disk: its files, `package` and `package.sig`, and the key that trusts it.
struct Package {
    folder: PathBuf,
    keys: KeySet,
    list: Vec<u8>,
    signature: Vec<u8>,
}

impl Package {
    /// Sign `files` as plugin `signed_id` with a key generated for this package alone.
    fn sign(name: &str, signed_id: &str, files: &[(&str, Vec<u8>)]) -> Result<Self, UpdateError> {
        let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("plugin-packages")
            .join(format!("{name}-{}", std::process::id()));
        if folder.exists() {
            fs::remove_dir_all(&folder).expect("an old copy of this test's own folder");
        }
        fs::create_dir_all(&folder).expect("the package folder");
        let mut entries = Vec::new();
        for (path, bytes) in files {
            fs::write(folder.join(path), bytes).expect("a package file");
            let path = PackagePath::parse(path).expect("a plain file name");
            entries.push(FileEntry::new(path, bytes.len() as u64, sha256(bytes)?));
        }
        let list = PluginPackageList::new(signed_id, Version::new(0, 1, 0), WORLD, entries)?;
        let list = list.to_bytes();

        let signer = TestKeyPair::generate()?;
        let point = signer.public_point()?;
        let value = signer.sign_message(&list)?;
        let signature =
            DetachedSignature::new(SignatureKind::PluginPackage, KeyId::of(&point)?, value)
                .to_bytes()
                .to_vec();
        fs::write(folder.join(PACKAGE_FILE), &list).expect("package");
        fs::write(folder.join(PACKAGE_SIGNATURE_FILE), &signature).expect("package.sig");
        let keys = KeySet::new(&[PinnedKey::new(point)])?;
        Ok(Self {
            folder,
            keys,
            list,
            signature,
        })
    }

    /// The bytes as read back from disk, as Syrinx would read the folder.
    fn signed_files(&self) -> (Vec<u8>, Vec<u8>) {
        let list = fs::read(self.folder.join(PACKAGE_FILE)).expect("package");
        let signature = fs::read(self.folder.join(PACKAGE_SIGNATURE_FILE)).expect("package.sig");
        assert_eq!((&list, &signature), (&self.list, &self.signature));
        (list, signature)
    }
}

/// The verified folder's manifest and module, read from the handles that were hashed.
struct VerifiedFiles {
    package: VerifiedPluginPackage,
    manifest: Vec<u8>,
    module: Vec<u8>,
}

/// Steps 1 and 2: the signature, slot and version, then every listed file from held handles.
fn verify(package: &Package, slot: &str) -> Result<VerifiedFiles, UpdateError> {
    let (list, signature) = package.signed_files();
    let verified = verify_plugin_package(&list, &signature, &package.keys, slot, None)?;
    let mut payload = verified.verify_payload(DirectoryPayload::open(&package.folder)?)?;
    let mut manifest = Vec::new();
    let mut module = Vec::new();
    for file in payload.files_mut() {
        let path = file.entry().path().as_str().to_owned();
        let bytes = read_bounded(file.reader(), READ_LIMIT, "a verified package file")?;
        match path.as_str() {
            MANIFEST_FILE => manifest = bytes,
            MODULE_FILE => module = bytes,
            _ => {}
        }
    }
    Ok(VerifiedFiles {
        package: verified,
        manifest,
        module,
    })
}

/// Steps 3 and 4: the signed lines against the manifest, then registration from the manifest.
fn register<'a>(
    verified: &VerifiedFiles,
    plugins: &'a mut PluginRegistry,
    commands: &mut CommandRegistry,
) -> Result<&'a RegisteredPlugin, String> {
    let list = verified.package.list();
    let lines =
        SignedIdentity::from_signed_lines(list.id(), &list.version().to_string(), list.world());
    let manifest = PluginManifest::parse(&verified.manifest, DiscoveryOptions::default())
        .map_err(|error| error.to_string())?;
    let signed = check_signed(&lines, manifest).map_err(|error| error.to_string())?;
    plugins
        .register(signed, commands)
        .map_err(|error| error.to_string())
}

#[test]
fn a_signed_fixture_folder_verifies_registers_and_compiles() -> Result<(), UpdateError> {
    let package = Package::sign(
        "full",
        "test",
        &[
            (MODULE_FILE, full_component().to_vec()),
            (MANIFEST_FILE, full_manifest("test")),
        ],
    )?;
    let verified = verify(&package, "test")?;
    assert_eq!(verified.module, full_component());

    let mut commands = CommandRegistry::browser();
    let before = commands.len();
    let mut plugins = PluginRegistry::new();
    let registered = register(&verified, &mut plugins, &mut commands).expect("registers");
    assert_eq!(commands.len(), before + registered.command_keys().len());
    assert!(commands.find("test.echo").is_some());

    // Step 5, at load or activation: the component against the registered manifest.
    Runtime::new()
        .expect("engine")
        .compile_package(registered, &verified.module)
        .expect("the signed component agrees with its manifest");
    Ok(())
}

#[test]
fn a_tampered_component_is_refused_before_it_is_compiled() -> Result<(), UpdateError> {
    let package = Package::sign(
        "tampered",
        "test",
        &[
            (MODULE_FILE, full_component().to_vec()),
            (MANIFEST_FILE, full_manifest("test")),
        ],
    )?;
    let mut module = full_component().to_vec();
    let middle = module.len() / 2;
    module[middle] ^= 0x01;
    fs::write(package.folder.join(MODULE_FILE), &module).expect("tampers");

    // The hash check fails, so no bytes ever reach `Component::new`.
    assert_eq!(
        verify(&package, "test").err(),
        Some(UpdateError::HashMismatch {
            path: MODULE_FILE.to_owned()
        })
    );
    Ok(())
}

#[test]
fn a_package_signed_for_slot_evil_that_claims_to_be_notes_registers_nothing()
-> Result<(), UpdateError> {
    let package = Package::sign(
        "evil",
        "evil",
        &[
            (MODULE_FILE, full_component().to_vec()),
            (MANIFEST_FILE, full_manifest("notes")),
        ],
    )?;
    // The signature, the slot and every file check out: the lie is in the manifest.
    let verified = verify(&package, "evil")?;

    let mut commands = CommandRegistry::browser();
    let before = commands.len();
    let mut plugins = PluginRegistry::new();
    let refusal = register(&verified, &mut plugins, &mut commands).expect_err("refused");
    let expected = IdentityMismatch::Id {
        signed: "evil".into(),
        manifest: "notes".into(),
    };
    assert_eq!(refusal, expected.to_string());
    assert_eq!(commands.len(), before);
    assert!(commands.find("notes.echo").is_none());
    assert!(plugins.is_empty());
    Ok(())
}

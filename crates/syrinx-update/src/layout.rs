//! The host's `updates/v1/` files for one signed release, checked as an installed copy would first (#55).

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use crate::directory::DirectoryPayload;
use crate::keys::KeySet;
use crate::latest::verify_statement;
use crate::manifest::{MAX_MANIFEST_BYTES, Manifest};
use crate::payload::read_bounded;
use crate::signature::SIGNATURE_BYTES;
use crate::statement::MAX_STATEMENT_BYTES;
use crate::time::Timestamp;
use crate::version::Version;

/// The signed files a package folder holds after `sign-update`, named as on the host.
const MANIFEST: &str = "manifest";
const MANIFEST_SIGNATURE: &str = "manifest.sig";
const STATEMENT: &str = "latest";
const STATEMENT_SIGNATURE: &str = "latest.sig";

/// The folder under a version that holds one file per manifest entry.
const FILES_FOLDER: &str = "files";

/// What a host layout carries besides the latest statement and its signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFiles {
    /// A release: `<version>/manifest`, `manifest.sig` and `files/…` as well.
    Include,
    /// A weekly re-sign: only `latest` and `latest.sig` change on the host.
    Omit,
}

/// The four signed files read from a package folder.
struct SignedFiles {
    manifest: Vec<u8>,
    manifest_signature: Vec<u8>,
    statement: Vec<u8>,
    statement_signature: Vec<u8>,
}

/// Write `output\updates\v1\…` from a signed package folder, returning that `v1` folder.
///
/// The statement and manifest are verified against `keys` at `now`, and copied payload files
/// against the manifest, so nothing an installed copy would refuse is written. `output`
/// must not already hold an `updates` folder.
pub fn write_host_layout(
    package: &Path,
    output: &Path,
    keys: &KeySet,
    now: Timestamp,
    files: HostFiles,
) -> Result<PathBuf, String> {
    let signed = SignedFiles::read(package)?;
    let manifest = signed.verify(keys, now)?;
    let root = fresh_root(output)?;
    if files == HostFiles::Include {
        let version = root.join(manifest.version().to_string());
        create_folder(&version)?;
        write_new(&version.join(MANIFEST), &signed.manifest)?;
        write_new(
            &version.join(MANIFEST_SIGNATURE),
            &signed.manifest_signature,
        )?;
        copy_payload(package, &version.join(FILES_FOLDER), &manifest)?;
    }

    // The statement goes last, so a partial layout never names files it lacks.
    write_new(&root.join(STATEMENT_SIGNATURE), &signed.statement_signature)?;
    write_new(&root.join(STATEMENT), &signed.statement)?;
    Ok(root)
}

impl SignedFiles {
    fn read(package: &Path) -> Result<Self, String> {
        Ok(Self {
            manifest: read_file(package, MANIFEST, MAX_MANIFEST_BYTES)?,
            manifest_signature: read_file(package, MANIFEST_SIGNATURE, SIGNATURE_BYTES)?,
            statement: read_file(package, STATEMENT, MAX_STATEMENT_BYTES)?,
            statement_signature: read_file(package, STATEMENT_SIGNATURE, SIGNATURE_BYTES)?,
        })
    }

    /// Accept the statement as of `now`, then the manifest it names.
    fn verify(&self, keys: &KeySet, now: Timestamp) -> Result<Manifest, String> {
        let statement =
            verify_statement(&self.statement, &self.statement_signature, keys, now, None)
                .map_err(|error| format!("The latest statement would be refused: {error}."))?;
        // Every published release is newer than 0.0.0, so this checks only signature and hash.
        let manifest = statement
            .verify_manifest(
                &self.manifest,
                &self.manifest_signature,
                keys,
                Version::new(0, 0, 0),
            )
            .map_err(|error| format!("The manifest would be refused: {error}."))?;
        Ok(manifest.manifest().clone())
    }
}

/// Create `output\updates\v1`, refusing to write over an earlier layout.
fn fresh_root(output: &Path) -> Result<PathBuf, String> {
    let updates = output.join("updates");
    if updates.exists() {
        return Err(format!("{} already exists.", updates.display()));
    }
    fs::create_dir_all(output)
        .map_err(|error| format!("Could not create {}: {error}", output.display()))?;
    create_folder(&updates)?;
    let root = updates.join("v1");
    create_folder(&root)?;
    Ok(root)
}

/// Copy each listed file under `files`, then check the copies exactly as a download is checked.
fn copy_payload(package: &Path, files: &Path, manifest: &Manifest) -> Result<(), String> {
    create_folder(files)?;
    for entry in manifest.files() {
        let relative = entry.path().as_str().replace('/', "\\");
        let target = files.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        }
        fs::copy(package.join(&relative), &target)
            .map_err(|error| format!("Could not copy {relative}: {error}"))?;
    }
    let payload = DirectoryPayload::open(files)
        .map_err(|error| format!("Could not open {}: {error}.", files.display()))?;
    crate::payload::verify(manifest.files(), payload)
        .map(drop)
        .map_err(|error| format!("The copied files do not match the manifest: {error}."))
}

fn read_file(package: &Path, name: &str, limit: usize) -> Result<Vec<u8>, String> {
    let path = package.join(name);
    let file =
        File::open(&path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    read_bounded(file, limit, "a signed release file")
        .map_err(|error| format!("Could not read {}: {error}.", path.display()))
}

fn create_folder(path: &Path) -> Result<(), String> {
    fs::create_dir(path).map_err(|error| format!("Could not create {}: {error}", path.display()))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| {
            std::io::Write::write_all(&mut file, bytes)?;
            file.sync_all()
        })
        .map_err(|error| format!("Could not write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::cng::test_keys::TestKeyPair;
    use crate::tests::Scratch;
    use crate::{
        FileEntry, PackagePath, PinnedKey, SECONDS_PER_DAY, SignedRelease, StatementTerms,
        UpdateError, sign_release,
    };

    fn synthetic_files() -> BTreeMap<&'static str, &'static [u8]> {
        BTreeMap::from([
            ("syrinx.exe", &b"synthetic executable"[..]),
            ("syrinx-package.json", &b"{}\n"[..]),
            ("scripts/install.ps1", &b"# made-up script\n"[..]),
        ])
    }

    fn now() -> Timestamp {
        Timestamp::parse("2026-11-05T15:00:00Z").expect("canonical")
    }

    /// A package folder holding the files, a manifest and its signed release.
    fn signed_package(scratch: &Scratch, signer: &TestKeyPair, keys: &KeySet) -> PathBuf {
        let package = scratch.root.join("package");
        let mut entries = Vec::new();
        for (name, bytes) in synthetic_files() {
            let path = package.join(name.replace('/', "\\"));
            fs::create_dir_all(path.parent().expect("a parent")).expect("a synthetic folder");
            fs::write(&path, bytes).expect("a synthetic file");
            let checked = PackagePath::parse(name).expect("a safe path");
            let digest = crate::cng::sha256(bytes).expect("CNG hashes");
            entries.push(FileEntry::new(checked, bytes.len() as u64, digest));
        }
        entries.sort_by(|left, right| left.path().as_str().cmp(right.path().as_str()));
        let manifest = Manifest::new(Version::new(0, 7, 0), Version::new(0, 0, 0), entries)
            .expect("a valid manifest")
            .to_bytes();
        let terms = StatementTerms {
            serial: 1_800_000_000,
            issued: now(),
            expires: now()
                .checked_add_seconds(30 * SECONDS_PER_DAY)
                .expect("in range"),
        };
        let release = sign_release(&manifest, signer, keys, terms).expect("a signed release");
        write_release(&package, &manifest, &release);
        package
    }

    fn write_release(package: &Path, manifest: &[u8], release: &SignedRelease) {
        fs::write(package.join(MANIFEST), manifest).expect("the manifest");
        fs::write(package.join(MANIFEST_SIGNATURE), release.manifest_signature)
            .expect("its signature");
        fs::write(package.join(STATEMENT), &release.statement).expect("the statement");
        fs::write(
            package.join(STATEMENT_SIGNATURE),
            release.statement_signature,
        )
        .expect("its signature");
    }

    fn pinned(signer: &TestKeyPair) -> Result<KeySet, UpdateError> {
        KeySet::new(&[PinnedKey::new(signer.public_point()?)])
    }

    /// Every file under `root`, as `/`-separated relative paths.
    fn listing(root: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(folder) = pending.pop() {
            for entry in fs::read_dir(&folder).expect("a listable folder") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                let relative = path.strip_prefix(root).expect("under the root");
                found.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
        found.sort();
        found
    }

    #[test]
    fn a_release_layout_holds_the_signed_files_and_one_file_per_entry() -> Result<(), UpdateError> {
        let scratch = Scratch::new().map_err(crate::tests::io_error)?;
        let signer = TestKeyPair::generate()?;
        let keys = pinned(&signer)?;
        let package = signed_package(&scratch, &signer, &keys);
        let output = scratch.root.join("site");

        let root = write_host_layout(&package, &output, &keys, now(), HostFiles::Include)
            .expect("the layout is written");
        assert_eq!(root, output.join("updates").join("v1"));
        assert_eq!(
            listing(&output),
            [
                "updates/v1/0.7.0/files/scripts/install.ps1",
                "updates/v1/0.7.0/files/syrinx-package.json",
                "updates/v1/0.7.0/files/syrinx.exe",
                "updates/v1/0.7.0/manifest",
                "updates/v1/0.7.0/manifest.sig",
                "updates/v1/latest",
                "updates/v1/latest.sig",
            ]
        );
        let copied = fs::read(root.join("0.7.0/files/syrinx.exe")).expect("a copied file");
        assert_eq!(copied, b"synthetic executable");
        assert_eq!(
            fs::read(root.join(STATEMENT)).expect("the statement"),
            fs::read(package.join(STATEMENT)).expect("the original")
        );

        // A second layout into the same output is refused rather than mixed in.
        assert!(write_host_layout(&package, &output, &keys, now(), HostFiles::Include).is_err());
        Ok(())
    }

    #[test]
    fn a_weekly_resign_layout_holds_only_the_statement() -> Result<(), UpdateError> {
        let scratch = Scratch::new().map_err(crate::tests::io_error)?;
        let signer = TestKeyPair::generate()?;
        let keys = pinned(&signer)?;
        let package = signed_package(&scratch, &signer, &keys);
        let output = scratch.root.join("site");

        write_host_layout(&package, &output, &keys, now(), HostFiles::Omit)
            .expect("the layout is written");
        assert_eq!(
            listing(&output),
            ["updates/v1/latest", "updates/v1/latest.sig"]
        );
        Ok(())
    }

    #[test]
    fn nothing_is_written_for_a_release_an_installed_copy_would_refuse() -> Result<(), UpdateError>
    {
        let scratch = Scratch::new().map_err(crate::tests::io_error)?;
        let signer = TestKeyPair::generate()?;
        let keys = pinned(&signer)?;
        let package = signed_package(&scratch, &signer, &keys);

        // Signed by a key the executable does not pin.
        let other = pinned(&TestKeyPair::generate()?)?;
        let output = scratch.root.join("unknown-key");
        assert!(write_host_layout(&package, &output, &other, now(), HostFiles::Include).is_err());
        assert!(!output.join("updates").exists());

        // Published after the statement expired.
        let late = now()
            .checked_add_seconds(31 * SECONDS_PER_DAY)
            .expect("in range");
        let output = scratch.root.join("expired");
        assert!(write_host_layout(&package, &output, &keys, late, HostFiles::Omit).is_err());
        assert!(!output.join("updates").exists());

        // A payload file changed after signing: the copy is checked and refused.
        fs::write(package.join("syrinx.exe"), b"tampered executable!").expect("a change");
        let output = scratch.root.join("tampered");
        assert!(write_host_layout(&package, &output, &keys, now(), HostFiles::Include).is_err());
        assert!(!output.join("updates/v1/latest").exists());
        Ok(())
    }
}

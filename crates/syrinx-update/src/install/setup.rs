//! The release package carried inside `syrinx-setup.exe`, and the install or upgrade it performs.
//!
//! The payload is `PAYLOAD_MAGIC`, the manifest's length and bytes, then every file in the
//! manifest's order. Its trust is the setup program's own: the owner chose to run it.

use std::fs;
use std::path::Path;
use std::time::Duration;

use super::apply::swap;
use super::error::io_error;
use super::files::{
    entry_present, is_program_folder, prepare_destination, read_small, remove_program_folder,
    rename_patiently, write_new,
};
use super::layout::{PACKAGE_RECORD, REQUIRED_PROGRAM_FILES};
use super::lock::ApplyLock;
use super::stage::discard_staged;
use super::{InstallError, UpdateLayout};
use crate::cng;
use crate::manifest::{MAX_MANIFEST_BYTES, Manifest};
use crate::version::Version;

/// First bytes of every setup payload, so a wrong or empty file is refused at once.
pub const PAYLOAD_MAGIC: &[u8] = b"SYRINX-SETUP-PAYLOAD 1\n";

/// P1-38's manifest, which every package carries beside `syrinx-package.json`.
pub const MANIFEST_FILE: &str = "manifest";

/// Bytes of the little-endian manifest length that follows the magic.
const LENGTH_BYTES: usize = 8;

/// A payload whose every file matched its manifest entry.
#[derive(Debug)]
pub struct SetupPackage<'a> {
    manifest: Manifest,
    manifest_bytes: &'a [u8],
    files: Vec<&'a [u8]>,
}

impl<'a> SetupPackage<'a> {
    /// Split `payload` into its files, refusing any size, hash or layout mismatch.
    pub fn read(payload: &'a [u8]) -> Result<Self, InstallError> {
        let rest = payload
            .strip_prefix(PAYLOAD_MAGIC)
            .ok_or(InstallError::NotAPackage)?;
        let (manifest_bytes, mut rest) = split_manifest(rest)?;
        let manifest = Manifest::parse(manifest_bytes)?;
        require_program_files(&manifest)?;
        let mut files = Vec::with_capacity(manifest.files().len());
        for entry in manifest.files() {
            let size = usize::try_from(entry.size()).map_err(|_| InstallError::NotAPackage)?;
            if rest.len() < size {
                return Err(InstallError::NotAPackage);
            }
            let (file, after) = rest.split_at(size);
            if &cng::sha256(file)? != entry.sha256() {
                return Err(InstallError::NotAPackage);
            }
            files.push(file);
            rest = after;
        }
        if !rest.is_empty() {
            return Err(InstallError::NotAPackage);
        }
        Ok(Self {
            manifest,
            manifest_bytes,
            files,
        })
    }

    pub fn version(&self) -> Version {
        self.manifest.version()
    }
}

/// Build the payload from a package folder that `cargo xtask package` wrote.
pub fn write_payload(folder: &Path) -> Result<Vec<u8>, InstallError> {
    let manifest_bytes = read_small(
        &folder.join(MANIFEST_FILE),
        MAX_MANIFEST_BYTES,
        "reading the manifest",
    )?
    .ok_or(InstallError::NotAPackage)?;
    let manifest = Manifest::parse(&manifest_bytes)?;
    require_program_files(&manifest)?;
    let length = u64::try_from(manifest_bytes.len()).map_err(|_| InstallError::NotAPackage)?;
    let mut payload = PAYLOAD_MAGIC.to_vec();
    payload.extend_from_slice(&length.to_le_bytes());
    payload.extend_from_slice(&manifest_bytes);
    for entry in manifest.files() {
        let path = entry
            .path()
            .as_str()
            .split('/')
            .fold(folder.to_path_buf(), |joined, part| joined.join(part));
        let bytes = fs::read(&path).map_err(io_error("reading a package file"))?;
        if bytes.len() as u64 != entry.size() || &cng::sha256(&bytes)? != entry.sha256() {
            return Err(InstallError::NotAPackage);
        }
        payload.extend_from_slice(&bytes);
    }
    // Reading it back proves the writer and the reader agree.
    SetupPackage::read(&payload)?;
    Ok(payload)
}

/// What is in the program folder before setup runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Existing {
    Nothing,
    Version(Version),
    /// A Syrinx folder without a readable manifest, such as one from before P1-39a.
    Unknown,
}

/// Read the installed version, refusing a `Syrinx` folder that Syrinx's tooling did not make.
pub fn existing_install(layout: &UpdateLayout) -> Result<Existing, InstallError> {
    let program = layout.program();
    if !entry_present(&program, "inspecting the program folder")? {
        return Ok(Existing::Nothing);
    }
    if !is_program_folder(&program)? {
        return Err(InstallError::UnexpectedEntry { what: "Syrinx" });
    }
    let manifest = read_small(
        &program.join(MANIFEST_FILE),
        MAX_MANIFEST_BYTES,
        "reading the installed manifest",
    )?;
    // A missing or damaged manifest only changes the dialog's heading; setup replaces it anyway.
    let Some(bytes) = manifest else {
        return Ok(Existing::Unknown);
    };
    match Manifest::parse(&bytes) {
        Ok(manifest) => Ok(Existing::Version(manifest.version())),
        Err(_damaged) => Ok(Existing::Unknown),
    }
}

/// Install `package` as `Syrinx`, keeping the version it replaces as `Syrinx.previous`.
///
/// The data folder's profile is never touched. Any in-app update waiting in `Syrinx.new` is
/// discarded with its records, since setup replaces the program itself.
pub fn install_package(
    layout: &UpdateLayout,
    package: &SetupPackage<'_>,
    rename_patience: Duration,
) -> Result<Version, InstallError> {
    fs::create_dir_all(layout.programs()).map_err(io_error("creating the Programs folder"))?;
    fs::create_dir_all(layout.data()).map_err(io_error("creating the data folder"))?;
    let _lock = ApplyLock::try_acquire(layout)?.ok_or(InstallError::Busy)?;
    let replacing = !matches!(existing_install(layout)?, Existing::Nothing);
    // Refuse a foreign Syrinx.previous before writing 45 MB that the swap would then refuse.
    if replacing && !previous_is_replaceable(layout)? {
        return Err(InstallError::UnexpectedEntry {
            what: "Syrinx.previous",
        });
    }
    discard_staged(layout)?;
    if let Err(error) = write_staged(layout, package) {
        return Err(with_cleanup(
            error,
            remove_program_folder(&layout.staged(), "Syrinx.new"),
        ));
    }
    if !replacing {
        rename_patiently(
            &layout.staged(),
            &layout.program(),
            rename_patience,
            "moving Syrinx into place",
        )?;
        return Ok(package.version());
    }
    match swap(layout, rename_patience) {
        Ok(true) => Ok(package.version()),
        Ok(false) => Err(with_cleanup(
            InstallError::ProgramInUse,
            remove_program_folder(&layout.staged(), "Syrinx.new"),
        )),
        // Moved aside but not put back: the owner must be told where the old version is.
        Err(error) if !layout.program().exists() => Err(InstallError::SetAside(Box::new(error))),
        Err(error) => Err(error),
    }
}

/// Whether `Syrinx.previous` is absent or a folder Syrinx's tooling made.
fn previous_is_replaceable(layout: &UpdateLayout) -> Result<bool, InstallError> {
    let previous = layout.previous();
    Ok(!entry_present(&previous, "inspecting Syrinx.previous")? || is_program_folder(&previous)?)
}

/// `primary`, or both errors when cleaning up after it also failed.
fn with_cleanup(primary: InstallError, cleanup: Result<(), InstallError>) -> InstallError {
    match cleanup {
        Ok(()) => primary,
        Err(cleanup) => InstallError::Incomplete {
            primary: Box::new(primary),
            cleanup: Box::new(cleanup),
        },
    }
}

/// Write every file into a fresh `Syrinx.new`, the package record first so it is known as ours.
fn write_staged(layout: &UpdateLayout, package: &SetupPackage<'_>) -> Result<(), InstallError> {
    let staged = layout.staged();
    fs::create_dir(&staged).map_err(io_error("creating Syrinx.new"))?;
    let entries = package.manifest.files().iter().zip(&package.files);
    let (record, others): (Vec<_>, Vec<_>) =
        entries.partition(|(entry, _)| entry.path().as_str() == PACKAGE_RECORD);
    for (entry, bytes) in record.into_iter().chain(others) {
        let destination = prepare_destination(&staged, entry.path(), "creating a program folder")?;
        write_new(&destination, bytes, "writing a program file")?;
    }
    write_new(
        &staged.join(MANIFEST_FILE),
        package.manifest_bytes,
        "writing the manifest",
    )
}

fn split_manifest(rest: &[u8]) -> Result<(&[u8], &[u8]), InstallError> {
    let (length, rest) = rest
        .split_first_chunk::<LENGTH_BYTES>()
        .ok_or(InstallError::NotAPackage)?;
    let length = usize::try_from(u64::from_le_bytes(*length))
        .ok()
        .filter(|length| *length <= MAX_MANIFEST_BYTES && *length <= rest.len())
        .ok_or(InstallError::NotAPackage)?;
    Ok(rest.split_at(length))
}

fn require_program_files(manifest: &Manifest) -> Result<(), InstallError> {
    for name in REQUIRED_PROGRAM_FILES {
        if !manifest
            .files()
            .iter()
            .any(|entry| entry.path().as_str() == name)
        {
            return Err(InstallError::MissingProgramFile { name });
        }
    }
    Ok(())
}

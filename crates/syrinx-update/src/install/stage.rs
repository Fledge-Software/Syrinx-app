//! Stages a package folder: verify it, copy it in, verify the copy, then place it beside the program.

use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom};
use std::path::Path;

use super::error::io_error;
use super::fault::{Step, checkpoint};
use super::files::{
    copy_exact, ensure_plain_directory, prepare_destination, remove_file_if_present,
    remove_program_folder, remove_update_folder, write_new,
};
use super::layout::{APPLY_HELPER, PACKAGE_RECORD, REQUIRED_PROGRAM_FILES};
use super::lock::ApplyLock;
use super::records::{AcceptedRecord, Pending, UpdateResult, UpdateState};
use super::{InstallError, UpdateLayout};
use crate::directory::DirectoryPayload;
use crate::error::UpdateError;
use crate::keys::KeySet;
use crate::latest::{VerifiedStatement, verify_statement};
use crate::manifest::{FileEntry, MAX_MANIFEST_BYTES, Manifest};
use crate::payload::{VerifiedPayload, read_bounded};
use crate::signature::SIGNATURE_BYTES;
use crate::statement::MAX_STATEMENT_BYTES;
use crate::time::Timestamp;
use crate::version::Version;
use crate::{VerifiedManifest, cng};

/// Free space kept beyond two copies of the package: one incoming, one staged.
pub const SPACE_MARGIN_BYTES: u64 = 256 * 1024 * 1024;

/// What staging needs besides the folders: the package, the trusted keys and the clock.
pub struct StageRequest<'a> {
    /// The folder `cargo xtask package` and `sign-update` wrote, chosen by the owner.
    pub source: &'a Path,
    /// Always `KeySet::production()` in Syrinx; tests pass in-memory keys.
    pub keys: &'a KeySet,
    pub installed: Version,
    pub now: Timestamp,
}

/// How staging ended when nothing was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Staged {
    /// The statement names the installed version.
    UpToDate(Version),
    /// `Syrinx.new` holds this verified version, ready for Restart now or the next quit.
    Ready { version: Version, bytes: u64 },
}

/// Stage the package in `request.source`, holding the update lock throughout.
///
/// `free_space` reports the bytes free on the volume holding a folder.
pub fn stage_from_folder(
    layout: &UpdateLayout,
    request: &StageRequest<'_>,
    free_space: &dyn Fn(&Path) -> io::Result<u64>,
) -> Result<Staged, InstallError> {
    let _lock = ApplyLock::try_acquire(layout)?.ok_or(InstallError::Busy)?;
    let mut state = UpdateState::read(layout)?;
    let staged = stage_locked(layout, request, &mut state, free_space);
    match staged {
        Ok(staged) => Ok(staged),
        Err(error) => Err(record_failure(layout, state, error)),
    }
}

/// The verification, copying and records, in the order that keeps every step recoverable.
fn stage_locked(
    layout: &UpdateLayout,
    request: &StageRequest<'_>,
    state: &mut UpdateState,
    free_space: &dyn Fn(&Path) -> io::Result<u64>,
) -> Result<Staged, InstallError> {
    let statement = read_statement(request, state)?;
    let accepted = AcceptedRecord {
        statement: statement.accepted(),
        expires: statement.statement().expires(),
    };
    let version = statement.statement().version();
    if version == request.installed {
        *state = recorded(accepted, UpdateResult::UpToDate, version);
        state.write(layout)?;
        return Ok(Staged::UpToDate(version));
    }

    // Everything is checked before the first byte is written.
    let manifest_bytes = read_package_file(request.source, "manifest", MAX_MANIFEST_BYTES)?;
    let signature_bytes = read_package_file(request.source, "manifest.sig", SIGNATURE_BYTES)?;
    let verified = statement.verify_manifest(
        &manifest_bytes,
        &signature_bytes,
        request.keys,
        request.installed,
    )?;
    require_program_files(verified.manifest())?;
    let bytes = package_bytes(verified.manifest());
    check_space(layout, bytes, free_space)?;

    discard_staged(layout)?;
    checkpoint(Step::StagingCleared)?;
    let manifest = SignedManifest {
        verified: &verified,
        bytes: &manifest_bytes,
        signature: &signature_bytes,
    };
    let finished = copy_and_install(layout, request.source, &manifest).and_then(|()| {
        // The serial is stored before `pending`, so a crash between them only repeats staging.
        *state = recorded(accepted, UpdateResult::Staged, version);
        state.write(layout)?;
        let pending = Pending {
            version,
            from: request.installed,
            manifest_sha256: cng::sha256(&manifest_bytes)?,
        };
        pending.write(layout)
    });
    if let Err(primary) = finished {
        return Err(cleaned_up(primary, layout));
    }
    Ok(Staged::Ready { version, bytes })
}

fn recorded(accepted: AcceptedRecord, result: UpdateResult, version: Version) -> UpdateState {
    UpdateState {
        accepted: Some(accepted),
        result: Some(result),
        version: Some(version),
    }
}

/// Verify `latest` against the pinned keys, the clock and the last accepted serial.
fn read_statement(
    request: &StageRequest<'_>,
    state: &UpdateState,
) -> Result<VerifiedStatement, InstallError> {
    let statement = read_package_file(request.source, "latest", MAX_STATEMENT_BYTES)?;
    let signature = read_package_file(request.source, "latest.sig", SIGNATURE_BYTES)?;
    let previous = state.previous();
    Ok(verify_statement(
        &statement,
        &signature,
        request.keys,
        request.now,
        previous.as_ref(),
    )?)
}

/// Read one of the package's signed files, at most `limit` bytes.
fn read_package_file(
    source: &Path,
    name: &'static str,
    limit: usize,
) -> Result<Vec<u8>, InstallError> {
    let file = match File::open(source.join(name)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(InstallError::NotAPackage);
        }
        Err(error) => return Err(io_error("reading the update package")(error)),
    };
    Ok(read_bounded(file, limit, name)?)
}

/// Refuse a package that could not start, swap itself in, or be recognised later.
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

fn package_bytes(manifest: &Manifest) -> u64 {
    manifest.files().iter().map(FileEntry::size).sum()
}

/// Require room for an incoming and a staged copy, plus a margin, on both volumes.
fn check_space(
    layout: &UpdateLayout,
    bytes: u64,
    free_space: &dyn Fn(&Path) -> io::Result<u64>,
) -> Result<(), InstallError> {
    let needed = bytes.saturating_mul(2).saturating_add(SPACE_MARGIN_BYTES);
    for folder in [layout.data(), layout.programs()] {
        let available = free_space(folder).map_err(io_error("checking free space"))?;
        if available < needed {
            return Err(InstallError::NotEnoughSpace { needed, available });
        }
    }
    Ok(())
}

/// Remove an earlier staged update and anything a stopped staging left; `pending` goes first.
pub(crate) fn discard_staged(layout: &UpdateLayout) -> Result<(), InstallError> {
    remove_file_if_present(&layout.pending(), "removing the pending record")?;
    remove_program_folder(&layout.staged(), "Syrinx.new")?;
    remove_update_folder(&layout.apply_folder(), "the apply folder")?;
    remove_update_folder(&layout.incoming(), "the incoming folder")
}

/// The verified manifest with the exact bytes and signature that were checked.
struct SignedManifest<'a> {
    verified: &'a VerifiedManifest,
    bytes: &'a [u8],
    signature: &'a [u8],
}

/// Copy the package into `incoming`, verify that copy, install from its handles, then drop it.
fn copy_and_install(
    layout: &UpdateLayout,
    source: &Path,
    manifest: &SignedManifest<'_>,
) -> Result<(), InstallError> {
    let version = manifest.verified.manifest().version();
    ensure_plain_directory(&layout.incoming(), "the incoming folder")?;
    let incoming = layout.incoming().join(version.to_string());
    fs::create_dir(&incoming).map_err(io_error("creating the incoming folder"))?;
    copy_to_incoming(source, &incoming, manifest.verified.manifest().files())?;
    checkpoint(Step::CopiedToIncoming)?;

    // From here only the verified handles are read; the folder is never listed again.
    let payload = DirectoryPayload::open(&incoming)?;
    let mut payload = manifest.verified.verify_payload(payload)?;
    install_files(layout, &mut payload, manifest)?;
    drop(payload);
    remove_update_folder(&layout.incoming(), "the incoming folder")?;
    checkpoint(Step::IncomingRemoved)
}

/// Copy each listed file, and only those, refusing one longer or shorter than listed.
fn copy_to_incoming(
    source: &Path,
    incoming: &Path,
    files: &[FileEntry],
) -> Result<(), InstallError> {
    for entry in files {
        let path = entry.path().as_str();
        let from = path
            .split('/')
            .fold(source.to_path_buf(), |joined, part| joined.join(part));
        let mut reader = match File::open(&from) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(UpdateError::MissingFile {
                    path: path.to_owned(),
                }
                .into());
            }
            Err(error) => return Err(io_error("reading the update package")(error)),
        };
        let to = prepare_destination(incoming, entry.path(), "copying the update package")?;
        if !copy_exact(&mut reader, &to, entry.size(), "copying the update package")? {
            return Err(UpdateError::SizeMismatch {
                path: path.to_owned(),
                expected: entry.size(),
            }
            .into());
        }
    }
    Ok(())
}

/// Fill `Syrinx.new` and `Updates\apply` from the verified handles, package record first.
fn install_files(
    layout: &UpdateLayout,
    payload: &mut VerifiedPayload<DirectoryPayload>,
    manifest: &SignedManifest<'_>,
) -> Result<(), InstallError> {
    let staged = layout.staged();
    fs::create_dir(&staged).map_err(io_error("creating Syrinx.new"))?;
    let files = payload.files_mut();
    // The record goes first, so an interrupted copy is still recognisably Syrinx's.
    files.sort_by_key(|file| file.entry().path().as_str() != PACKAGE_RECORD);
    for (index, file) in files.iter_mut().enumerate() {
        let entry = file.entry().clone();
        let to = prepare_destination(&staged, entry.path(), "filling Syrinx.new")?;
        copy_verified(file.reader(), &to, &entry)?;
        if index == 0 {
            checkpoint(Step::FirstStagedFile)?;
        }
    }
    checkpoint(Step::StagedCopied)?;

    ensure_plain_directory(&layout.apply_folder(), "the apply folder")?;
    let helper = files
        .iter_mut()
        .find(|file| file.entry().path().as_str() == APPLY_HELPER)
        .ok_or(InstallError::MissingProgramFile { name: APPLY_HELPER })?;
    let entry = helper.entry().clone();
    copy_verified(helper.reader(), &layout.helper(), &entry)?;
    write_new(
        &layout.apply_manifest(),
        manifest.bytes,
        "keeping the manifest",
    )?;
    write_new(
        &layout.apply_signature(),
        manifest.signature,
        "keeping the manifest",
    )?;
    checkpoint(Step::HelperCopied)
}

/// Copy one verified file from its hashed handle, then rewind the handle for another copy.
fn copy_verified(reader: &mut File, to: &Path, entry: &FileEntry) -> Result<(), InstallError> {
    let exact = copy_exact(reader, to, entry.size(), "installing a verified file")?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(io_error("installing a verified file"))?;
    if !exact {
        return Err(UpdateError::SizeMismatch {
            path: entry.path().as_str().to_owned(),
            expected: entry.size(),
        }
        .into());
    }
    Ok(())
}

/// Remove what a failed staging left, keeping the first error and adding a cleanup failure.
fn cleaned_up(primary: InstallError, layout: &UpdateLayout) -> InstallError {
    #[cfg(test)]
    if matches!(primary, InstallError::Interrupted(_)) {
        // A fault hook stands in for a crash, which cleans nothing up.
        return primary;
    }
    match discard_staged(layout) {
        Ok(()) => primary,
        Err(cleanup) => InstallError::Incomplete {
            primary: Box::new(primary),
            cleanup: Box::new(cleanup),
        },
    }
}

/// Record the failure's category, keeping the accepted serial as it was.
fn record_failure(
    layout: &UpdateLayout,
    mut state: UpdateState,
    error: InstallError,
) -> InstallError {
    #[cfg(test)]
    if matches!(error, InstallError::Interrupted(_)) {
        return error;
    }
    state.result = Some(failure_category(&error));
    state.version = None;
    match state.write(layout) {
        Ok(()) => error,
        Err(cleanup) => InstallError::Incomplete {
            primary: Box::new(error),
            cleanup: Box::new(cleanup),
        },
    }
}

fn failure_category(error: &InstallError) -> UpdateResult {
    match error {
        InstallError::Update(UpdateError::StatementExpired { .. }) => {
            UpdateResult::StatementExpired
        }
        InstallError::Update(_) | InstallError::NotAPackage => UpdateResult::VerificationFailed,
        _ => UpdateResult::StagingFailed,
    }
}

//! Stages a package: verify it, copy it in, verify the copy, then place it beside the program.

use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom};
use std::path::Path;

use super::error::{TransferProblem, io_error};
use super::fault::{Step, checkpoint};
use super::files::{
    copy_exact, ensure_plain_directory, prepare_destination, remove_file_if_present,
    remove_program_folder, remove_update_folder, write_atomically, write_new,
};
use super::layout::{APPLY_HELPER, PACKAGE_RECORD, REQUIRED_PROGRAM_FILES};
use super::lock::ApplyLock;
use super::records::{AcceptedRecord, Pending, UpdateResult, UpdateState};
use super::source::{FolderSource, PackageSource, SignedFile};
use super::{InstallError, UpdateLayout};
use crate::directory::DirectoryPayload;
use crate::error::UpdateError;
use crate::keys::KeySet;
use crate::latest::{VerifiedStatement, verify_statement};
use crate::manifest::{FileEntry, MAX_MANIFEST_BYTES, Manifest};
use crate::payload::VerifiedPayload;
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

/// The trusted keys, the installed version and the clock, whatever the package's source.
#[derive(Clone, Copy)]
pub struct StageTerms<'a> {
    /// Always `KeySet::production()` in Syrinx; tests pass in-memory keys.
    pub keys: &'a KeySet,
    pub installed: Version,
    pub now: Timestamp,
}

impl<'a> StageRequest<'a> {
    fn terms(&self) -> StageTerms<'a> {
        StageTerms {
            keys: self.keys,
            installed: self.installed,
            now: self.now,
        }
    }
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
    let source = FolderSource::new(request.source);
    stage_from_source(layout, &source, &request.terms(), free_space)
}

/// Stage the package `source` provides, holding the update lock throughout.
///
/// Used for a folder and for the update host alike; `free_space` is as for `stage_from_folder`.
pub fn stage_from_source(
    layout: &UpdateLayout,
    source: &dyn PackageSource,
    terms: &StageTerms<'_>,
    free_space: &dyn Fn(&Path) -> io::Result<u64>,
) -> Result<Staged, InstallError> {
    let _lock = ApplyLock::try_acquire(layout)?.ok_or(InstallError::Busy)?;
    let mut state = UpdateState::read(layout)?;
    let staged = stage_locked(layout, source, terms, &mut state, free_space);
    match staged {
        Ok(staged) => Ok(staged),
        Err(error) => Err(record_failure(layout, state, error)),
    }
}

/// The verification, copying and records, in the order that keeps every step recoverable.
fn stage_locked(
    layout: &UpdateLayout,
    source: &dyn PackageSource,
    terms: &StageTerms<'_>,
    state: &mut UpdateState,
    free_space: &dyn Fn(&Path) -> io::Result<u64>,
) -> Result<Staged, InstallError> {
    let statement = read_statement(source, terms, state)?;
    let accepted = accepted_record(&statement);
    let version = statement.statement().version();
    if version == terms.installed {
        *state = recorded(accepted, UpdateResult::UpToDate, version);
        state.write(layout)?;
        return Ok(Staged::UpToDate(version));
    }

    // Everything is checked before the first byte is written.
    let stated = read_stated_manifest(source, &statement, terms)?;
    let verified = stated.verified;
    let manifest_bytes = stated.bytes;
    let signature_bytes = stated.signature;
    let bytes = package_bytes(verified.manifest());
    check_space(layout, bytes, free_space)?;

    discard_staged(layout)?;
    // Marks `Syrinx.new` as Syrinx's own until `pending` takes over.
    write_atomically(
        &layout.staging_marker(),
        b"staging\n",
        "marking the staging",
    )?;
    checkpoint(Step::StagingCleared)?;
    let manifest = SignedManifest {
        verified: &verified,
        bytes: &manifest_bytes,
        signature: &signature_bytes,
    };
    let finished = copy_and_install(layout, source, &manifest).and_then(|()| {
        // The serial is stored before `pending`, so a crash between them only repeats staging.
        *state = recorded(accepted, UpdateResult::Staged, version);
        state.write(layout)?;
        let pending = Pending {
            version,
            from: terms.installed,
            manifest_sha256: cng::sha256(&manifest_bytes)?,
        };
        pending.write(layout)?;
        remove_file_if_present(&layout.staging_marker(), "clearing the staging mark")
    });
    if let Err(primary) = finished {
        return Err(cleaned_up(primary, layout));
    }
    Ok(Staged::Ready { version, bytes })
}

pub(super) fn recorded(
    accepted: AcceptedRecord,
    result: UpdateResult,
    version: Version,
) -> UpdateState {
    UpdateState {
        accepted: Some(accepted),
        result: Some(result),
        version: Some(version),
    }
}

pub(super) fn accepted_record(statement: &VerifiedStatement) -> AcceptedRecord {
    AcceptedRecord {
        statement: statement.accepted(),
        expires: statement.statement().expires(),
    }
}

/// Verify `latest` against the pinned keys, the clock and the last accepted serial.
pub(super) fn read_statement(
    source: &dyn PackageSource,
    terms: &StageTerms<'_>,
    state: &UpdateState,
) -> Result<VerifiedStatement, InstallError> {
    let statement = source.read_signed(SignedFile::Statement, MAX_STATEMENT_BYTES)?;
    let signature = source.read_signed(SignedFile::StatementSignature, SIGNATURE_BYTES)?;
    let previous = state.previous();
    Ok(verify_statement(
        &statement,
        &signature,
        terms.keys,
        terms.now,
        previous.as_ref(),
    )?)
}

/// The stated version's manifest, verified, with the exact bytes and signature that were read.
pub(super) struct StatedManifest {
    pub(super) verified: VerifiedManifest,
    pub(super) bytes: Vec<u8>,
    pub(super) signature: Vec<u8>,
}

/// Read and verify the manifest the statement names, and require the program files.
pub(super) fn read_stated_manifest(
    source: &dyn PackageSource,
    statement: &VerifiedStatement,
    terms: &StageTerms<'_>,
) -> Result<StatedManifest, InstallError> {
    let version = statement.statement().version();
    let bytes = source.read_signed(SignedFile::Manifest(version), MAX_MANIFEST_BYTES)?;
    let signature = source.read_signed(SignedFile::ManifestSignature(version), SIGNATURE_BYTES)?;
    let verified = statement.verify_manifest(&bytes, &signature, terms.keys, terms.installed)?;
    require_program_files(verified.manifest())?;
    Ok(StatedManifest {
        verified,
        bytes,
        signature,
    })
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

pub(super) fn package_bytes(manifest: &Manifest) -> u64 {
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

/// The staging mark's text while a discard runs.
pub(crate) const DISCARD_MARK: &[u8] = b"discard\n";
/// The mark's text while start-up discards an update beside an unreadable `Updates\state`, so
/// the next start can finish that discard and still tell the owner (DW-69).
pub(crate) const UNREADABLE_STATE_DISCARD_MARK: &[u8] = b"discard unreadable-state\n";

/// Remove an earlier staged update and anything a stopped staging left.
pub(crate) fn discard_staged(layout: &UpdateLayout) -> Result<(), InstallError> {
    discard_marked(layout, DISCARD_MARK)
}

/// Remove a staged update because `Updates\state` cannot be read (DW-66).
pub(crate) fn discard_for_unreadable_state(layout: &UpdateLayout) -> Result<(), InstallError> {
    discard_marked(layout, UNREADABLE_STATE_DISCARD_MARK)
}

/// The mark goes before `pending`, so a discard stopped anywhere leaves a marked `Syrinx.new`
/// that the next start's `clear_unfinished` removes (DW-69); the mark itself goes last.
fn discard_marked(layout: &UpdateLayout, mark: &[u8]) -> Result<(), InstallError> {
    write_atomically(&layout.staging_marker(), mark, "marking the discard")?;
    remove_file_if_present(&layout.pending(), "removing the pending record")?;
    checkpoint(Step::DiscardBegun)?;
    remove_program_folder(&layout.staged(), "Syrinx.new")?;
    remove_update_folder(&layout.apply_folder(), "the apply folder")?;
    remove_update_folder(&layout.incoming(), "the incoming folder")?;
    remove_file_if_present(&layout.staging_marker(), "clearing the staging mark")
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
    source: &dyn PackageSource,
    manifest: &SignedManifest<'_>,
) -> Result<(), InstallError> {
    let version = manifest.verified.manifest().version();
    ensure_plain_directory(&layout.incoming(), "the incoming folder")?;
    let incoming = layout.incoming().join(version.to_string());
    fs::create_dir(&incoming).map_err(io_error("creating the incoming folder"))?;
    let files = manifest.verified.manifest().files();
    copy_to_incoming(source, version, &incoming, files)?;
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
    source: &dyn PackageSource,
    version: Version,
    incoming: &Path,
    files: &[FileEntry],
) -> Result<(), InstallError> {
    for entry in files {
        let mut reader = source.open_file(version, entry)?;
        let to = prepare_destination(incoming, entry.path(), "copying the update package")?;
        if !copy_exact(&mut reader, &to, entry.size(), "copying the update package")? {
            return Err(UpdateError::SizeMismatch {
                path: entry.path().as_str().to_owned(),
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
pub(super) fn record_failure(
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
        InstallError::Transfer(TransferProblem::Cancelled) => UpdateResult::Cancelled,
        InstallError::Transfer(_) => UpdateResult::NetworkFailed,
        // A failed cleanup does not hide why staging stopped.
        InstallError::Incomplete { primary, .. } => failure_category(primary),
        _ => UpdateResult::StagingFailed,
    }
}

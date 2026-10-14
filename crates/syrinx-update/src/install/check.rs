//! "Check for updates" (P1-39c): read and verify the statement and the manifest it names, so the
//! owner sees a version and size before anything larger is downloaded.

use super::lock::ApplyLock;
use super::records::{UpdateResult, UpdateState};
use super::source::PackageSource;
use super::stage::{
    StageTerms, accepted_record, package_bytes, read_stated_manifest, read_statement,
    record_failure, recorded,
};
use super::{InstallError, UpdateLayout};
use crate::version::Version;

/// What a check found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    /// The statement names the installed version.
    UpToDate(Version),
    /// A newer version, signed and stated, with the bytes its download needs.
    Available { version: Version, bytes: u64 },
}

/// Verify the statement and its manifest from `source`; nothing is staged or downloaded.
///
/// Up to date stores the statement's serial, as staging does. Available stores only the
/// category: the serial is stored once the owner's download is staged.
pub fn check_source(
    layout: &UpdateLayout,
    source: &dyn PackageSource,
    terms: &StageTerms<'_>,
) -> Result<Checked, InstallError> {
    let _lock = ApplyLock::try_acquire(layout)?.ok_or(InstallError::Busy)?;
    let mut state = UpdateState::read(layout)?;
    match check_locked(layout, source, terms, &mut state) {
        Ok(checked) => Ok(checked),
        Err(error) => Err(record_failure(layout, state, error)),
    }
}

fn check_locked(
    layout: &UpdateLayout,
    source: &dyn PackageSource,
    terms: &StageTerms<'_>,
    state: &mut UpdateState,
) -> Result<Checked, InstallError> {
    let statement = read_statement(source, terms, state)?;
    let version = statement.statement().version();
    if version == terms.installed {
        *state = recorded(accepted_record(&statement), UpdateResult::UpToDate, version);
        state.write(layout)?;
        return Ok(Checked::UpToDate(version));
    }

    // The manifest carries the size the owner is asked about; it is bound by the statement.
    let stated = read_stated_manifest(source, &statement, terms)?;
    let bytes = package_bytes(stated.verified.manifest());
    state.result = Some(UpdateResult::Available);
    state.version = Some(version);
    state.write(layout)?;
    Ok(Checked::Available { version, bytes })
}

//! What Syrinx does with update files when it starts, and when it hands over to the helper.

use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use super::error::io_error;
use super::files::{
    ensure_plain_directory, read_small, remove_file_if_present, remove_program_folder,
    remove_update_folder, write_atomically,
};
use super::layout::APPLY_HELPER;
use super::lock::ApplyLock;
use super::records::{Outcome, Pending, UpdateResult, UpdateState};
use super::stage::discard_staged;
use super::{InstallError, UpdateLayout};
use crate::error::UpdateError;
use crate::keys::KeySet;
use crate::manifest::MAX_MANIFEST_BYTES;
use crate::payload::hash_bounded;
use crate::signature::SIGNATURE_BYTES;
use crate::version::Version;
use crate::{cng, verify_manifest};

/// Share only reading, so the helper cannot be changed between its check and its start.
const FILE_SHARE_READ: u32 = 0x0000_0001;

/// What a starting Syrinx found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recovery {
    /// A verified copy waiting for Restart now or the next quit.
    pub staged: Option<Version>,
    /// What the helper did since the last start, to tell the owner once.
    pub outcome: Option<Outcome>,
    /// The helper is still at work, so nothing was touched.
    pub helper_running: bool,
}

/// Finish or clear what an interrupted staging or swap left, unless the helper is at work.
pub fn recover(layout: &UpdateLayout, running: Version) -> Result<Recovery, InstallError> {
    let Some(_lock) = ApplyLock::try_acquire(layout)? else {
        return Ok(Recovery {
            helper_running: true,
            ..Recovery::default()
        });
    };
    remove_update_folder(&layout.incoming(), "the incoming folder")?;
    let outcome = Outcome::take(layout)?;
    let staged = settle_pending(layout, running)?;
    Ok(Recovery {
        staged,
        outcome,
        helper_running: false,
    })
}

/// Keep a complete staged copy for this version; clear everything else.
fn settle_pending(
    layout: &UpdateLayout,
    running: Version,
) -> Result<Option<Version>, InstallError> {
    let Some(pending) = Pending::read(layout)? else {
        clear_unfinished(layout)?;
        return Ok(None);
    };
    if rollback_began(layout, &pending, running)? {
        // A rollback the helper could not finish: never offer or record the failed build.
        discard_staged(layout)?;
        return Ok(None);
    }
    if pending.version == running {
        // The swap finished but its records were not cleared.
        remove_file_if_present(&layout.pending(), "removing the pending record")?;
        remove_file_if_present(&layout.staging_marker(), "clearing the staging mark")?;
        let mut state = UpdateState::read(layout)?;
        state.result = Some(UpdateResult::Installed);
        state.version = Some(running);
        state.write(layout)?;
        return Ok(None);
    }
    let staged_present = layout.staged().is_dir();
    if pending.from != running || pending.version < running || !staged_present {
        // Staged by another version, older than this one, or incomplete.
        discard_staged(layout)?;
        return Ok(None);
    }
    Ok(Some(pending.version))
}

/// Whether the helper began rolling `pending` back: the state says so, or the version is the
/// running one while the staging mark is set (the helper clears a stale mark before swapping).
fn rollback_began(
    layout: &UpdateLayout,
    pending: &Pending,
    running: Version,
) -> Result<bool, InstallError> {
    if pending.version == running && layout.staging_marker().exists() {
        return Ok(true);
    }
    let state = UpdateState::read(layout)?;
    let rolled_back = matches!(
        state.result,
        Some(UpdateResult::ApplyRolledBack | UpdateResult::RollbackFailed)
    );
    Ok(rolled_back && state.version == Some(pending.version))
}

/// Remove a `Syrinx.new` that staging or a rollback left unfinished, recognised by the
/// staging mark. Without the mark it may be `install-syrinx.ps1`'s copy in progress, so it stays.
fn clear_unfinished(layout: &UpdateLayout) -> Result<(), InstallError> {
    if !layout.staging_marker().exists() {
        return Ok(());
    }
    remove_program_folder(&layout.staged(), "Syrinx.new")?;
    remove_file_if_present(&layout.staging_marker(), "clearing the staging mark")
}

/// Record that this Syrinx, started by the helper, restored its session and drew a frame.
pub fn record_started(layout: &UpdateLayout, running: Version) -> Result<(), InstallError> {
    ensure_plain_directory(&layout.updates(), "the Updates folder")?;
    write_atomically(
        &layout.started(),
        format!("{running}\n").as_bytes(),
        "recording a clean start",
    )
}

/// The version a relaunched Syrinx reported starting cleanly, if any yet.
pub(crate) fn started_version(layout: &UpdateLayout) -> Result<Option<Version>, InstallError> {
    let Some(bytes) = read_small(&layout.started(), 64, "reading the start record")? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| text.strip_suffix('\n'))
        .ok_or(InstallError::Record {
            what: "start record",
        })?;
    Ok(Version::parse(text))
}

/// What Syrinx keeps until it exits after starting the helper: the lock, and the checked
/// helper held so nothing can change it before it starts.
#[derive(Debug)]
pub struct Handoff {
    _lock: ApplyLock,
    _helper: File,
}

/// Take the lock and check the helper against the kept, signed manifest before starting it.
pub fn prepare_handoff(layout: &UpdateLayout, keys: &KeySet) -> Result<Handoff, InstallError> {
    let lock = ApplyLock::try_acquire(layout)?.ok_or(InstallError::Busy)?;
    let pending = Pending::read(layout)?.ok_or(InstallError::NothingStaged)?;
    const WHAT: &str = "reading the kept manifest";
    let manifest = read_small(&layout.apply_manifest(), MAX_MANIFEST_BYTES, WHAT)?
        .ok_or(InstallError::NothingStaged)?;
    let signature = read_small(&layout.apply_signature(), SIGNATURE_BYTES, WHAT)?
        .ok_or(InstallError::NothingStaged)?;
    if cng::sha256(&manifest)? != pending.manifest_sha256 {
        return Err(InstallError::StagedManifestChanged);
    }
    let verified = verify_manifest(&manifest, &signature, keys, pending.from)?;
    let entry = verified
        .manifest()
        .files()
        .iter()
        .find(|entry| entry.path().as_str() == APPLY_HELPER)
        .ok_or(InstallError::MissingProgramFile { name: APPLY_HELPER })?;
    let mut helper = open_unchangeable(&layout.helper())?;
    let (length, digest) = hash_bounded(&mut helper, entry.size(), APPLY_HELPER)?;
    if length != entry.size() || &digest != entry.sha256() {
        return Err(UpdateError::HashMismatch {
            path: APPLY_HELPER.to_owned(),
        }
        .into());
    }
    Ok(Handoff {
        _lock: lock,
        _helper: helper,
    })
}

/// Open a file so that, while the handle lives, nothing can write, rename or delete it, yet
/// Windows can still read it to start it.
pub(crate) fn open_unchangeable(path: &Path) -> Result<File, InstallError> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .map_err(io_error("checking the update helper"))
}

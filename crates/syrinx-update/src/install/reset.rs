//! "Reset update records" (DW-66): the owner's way out of an unreadable `Updates\state`,
//! which P1-39b's staging and P1-39c's check refuse to work past.

use std::fs;

use super::error::io_error;
use super::files::entry_present;
use super::lock::ApplyLock;
use super::records::{STATE_RECORD, UpdateState};
use super::{InstallError, UpdateLayout};

/// The name the unreadable record is kept under, beside `state`, as evidence.
pub const SET_ASIDE_STATE: &str = "state.unreadable";

/// How a reset ended when nothing was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateReset {
    /// The unreadable record was set aside; Syrinx now starts as a fresh install would.
    Reset,
    /// The record could be read after all, so nothing was changed.
    AlreadyReadable,
}

/// Whether `Updates\state` exists but cannot be read as a state record.
///
/// A missing record is a fresh install, not damage; an I/O failure is reported, not guessed at.
pub fn state_is_unreadable(layout: &UpdateLayout) -> Result<bool, InstallError> {
    match UpdateState::read(layout) {
        Ok(_) => Ok(false),
        Err(error) if is_unreadable_state(&error) => Ok(true),
        Err(error) => Err(error),
    }
}

/// Whether `error` is the refusal of an unreadable `Updates\state`, which a reset clears.
pub fn is_unreadable_state(error: &InstallError) -> bool {
    matches!(error, InstallError::Record { what } if *what == STATE_RECORD)
}

/// Set an unreadable `Updates\state` aside, forgetting the accepted statement serial.
///
/// Holds the update lock, so it cannot race staging, a check, a download or the helper, and
/// refuses while a staged update waits. Only `state` is renamed, in one step; nothing under
/// `Programs`, the database or the web view data is touched.
pub fn reset_unreadable_state(layout: &UpdateLayout) -> Result<StateReset, InstallError> {
    let _lock = ApplyLock::try_acquire(layout)?.ok_or(InstallError::Busy)?;
    // Rechecked under the lock: the owner's click may follow a repair or another reset.
    if !state_is_unreadable(layout)? {
        return Ok(StateReset::AlreadyReadable);
    }
    if entry_present(&layout.pending(), "checking for a staged update")? {
        return Err(InstallError::UpdateWaiting);
    }
    let set_aside = layout.updates().join(SET_ASIDE_STATE);
    // Replaces an older set-aside copy, so the evidence kept stays one small file.
    fs::rename(layout.state(), set_aside).map_err(io_error("setting the update record aside"))?;
    Ok(StateReset::Reset)
}

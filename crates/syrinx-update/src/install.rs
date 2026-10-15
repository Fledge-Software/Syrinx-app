//! Staging a verified package beside the installed program and swapping it in (P1-39b), from a
//! folder or from any `PackageSource` such as P1-39c's download.
//!
//! The folders, records and helper protocol are in `docs/p1-38-update-format.md`, "P1-39b".

mod apply;
mod check;
mod error;
mod fault;
mod files;
mod layout;
mod lock;
mod records;
mod reset;
mod source;
mod stage;
mod startup;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_apply;
#[cfg(test)]
mod tests_discard;
#[cfg(test)]
mod tests_reset;
#[cfg(test)]
mod tests_source;

pub use apply::{
    AFTER_UPDATE_FLAG, Applied, ApplyMode, ApplyRequest, ApplyTiming, LaunchedProgram, Launcher,
    ProcessLauncher, apply_update,
};
pub use check::{Checked, check_source};
pub use error::{InstallError, TransferProblem};
pub use fault::Step;
pub use layout::{
    APPLY_HELPER, PACKAGE_RECORD, PREVIOUS_FOLDER, PROGRAM_EXECUTABLE, PROGRAM_FOLDER,
    REQUIRED_PROGRAM_FILES, STAGED_FOLDER, UPDATES_FOLDER, UpdateLayout,
};
pub use lock::ApplyLock;
pub use records::{AcceptedRecord, Outcome, Pending, UpdateResult, UpdateState};
pub use reset::{
    SET_ASIDE_STATE, StateReset, is_unreadable_state, reset_unreadable_state, state_is_unreadable,
};
pub use source::{FolderSource, PackageSource, SignedFile};
pub use stage::{
    SPACE_MARGIN_BYTES, StageRequest, StageTerms, Staged, stage_from_folder, stage_from_source,
};
pub use startup::{Handoff, Recovery, prepare_handoff, record_started, recover};

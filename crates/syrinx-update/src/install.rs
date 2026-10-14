//! Staging a verified package beside the installed program and swapping it in (P1-39b).
//!
//! The folders, records and helper protocol are in `docs/p1-38-update-format.md`, "P1-39b".

mod apply;
mod error;
mod fault;
mod files;
mod layout;
mod lock;
mod records;
mod stage;
mod startup;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_apply;

pub use apply::{
    AFTER_UPDATE_FLAG, Applied, ApplyMode, ApplyRequest, ApplyTiming, LaunchedProgram, Launcher,
    ProcessLauncher, apply_update,
};
pub use error::InstallError;
pub use fault::Step;
pub use layout::{
    APPLY_HELPER, PACKAGE_RECORD, PREVIOUS_FOLDER, PROGRAM_EXECUTABLE, PROGRAM_FOLDER,
    REQUIRED_PROGRAM_FILES, STAGED_FOLDER, UPDATES_FOLDER, UpdateLayout,
};
pub use lock::ApplyLock;
pub use records::{AcceptedRecord, Outcome, Pending, UpdateResult, UpdateState};
pub use stage::{SPACE_MARGIN_BYTES, StageRequest, Staged, stage_from_folder};
pub use startup::{Handoff, Recovery, prepare_handoff, record_started, recover};

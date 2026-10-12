//! Confines Windows storage-location checks to Atlas's platform boundary.

pub mod browser;
pub mod file_dialog;
mod storage;

pub use storage::{
    RecoveryError, RecoveryOffer, RecoveryOutcome, StartupStorage, WindowsDirectoryLease,
    offer_recovery, prepare_startup,
};

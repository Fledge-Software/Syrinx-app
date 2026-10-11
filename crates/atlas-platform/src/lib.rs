//! Confines Windows storage-location checks to Atlas's platform boundary.

pub mod browser;
mod storage;

pub use storage::{
    RecoveryError, RecoveryOffer, RecoveryOutcome, WindowsDirectoryLease, offer_recovery,
};

//! Confines Windows storage, page, single-instance and Edge services to the platform boundary.

pub mod browser;
pub mod edge;
pub mod instance;
pub mod notice;
mod storage;

pub use storage::{
    RecoveryError, RecoveryOffer, RecoveryOutcome, StartupStorage, WindowsDirectoryLease,
    offer_recovery, prepare_startup,
};

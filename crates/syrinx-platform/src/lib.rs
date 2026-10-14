//! Confines Windows storage, page, single-instance, calendar, Edge, icon and disk services to the
//! platform boundary.

pub mod acceptance;
pub mod app_icon;
pub mod browser;
pub mod calendar;
pub mod disk;
pub mod edge;
pub mod file_dialog;
pub mod instance;
pub mod notice;
mod storage;

pub use storage::{
    DeletionJournal, JournalError, RecoveryError, RecoveryOffer, RecoveryOutcome, StartupStorage,
    WindowsDirectoryLease, offer_recovery, prepare_startup, read_deletion_log,
};

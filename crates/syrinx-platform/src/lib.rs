//! Confines Windows storage, page, single-instance, calendar, Edge, icon, disk, backdrop,
//! active-frame, caption and setup services to the platform boundary.

pub mod acceptance;
pub mod active_frame;
pub mod app_icon;
pub mod backdrop;
pub mod browser;
pub mod calendar;
pub mod caption;
pub mod disk;
pub mod edge;
pub mod file_dialog;
pub mod instance;
pub mod notice;
pub mod setup;
mod storage;
pub mod window_frame;

pub use storage::{
    CacheFolder, DeletionJournal, JournalError, RecoveryError, RecoveryOffer, RecoveryOutcome,
    StartupStorage, WindowsDirectoryLease, offer_recovery, prepare_startup, read_deletion_log,
};

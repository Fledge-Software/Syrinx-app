//! Confines the operating system's storage, page engine, single-instance, calendar, other-browser,
//! icon, disk, window-frame and setup services to the platform boundary: Windows, and Linux on X11.

#[cfg_attr(target_os = "linux", path = "linux/acceptance.rs")]
pub mod acceptance;
#[cfg_attr(target_os = "linux", path = "linux/active_frame.rs")]
pub mod active_frame;
#[cfg_attr(target_os = "linux", path = "linux/app_icon.rs")]
pub mod app_icon;
#[cfg_attr(target_os = "linux", path = "linux/backdrop.rs")]
pub mod backdrop;
#[cfg_attr(target_os = "linux", path = "browser_gtk/mod.rs")]
pub mod browser;
#[cfg_attr(unix, path = "unix/calendar.rs")]
pub mod calendar;
#[cfg_attr(target_os = "linux", path = "linux/caption.rs")]
pub mod caption;
#[cfg_attr(unix, path = "unix/disk.rs")]
pub mod disk;
#[cfg_attr(target_os = "linux", path = "linux/edge.rs")]
pub mod edge;
#[cfg_attr(target_os = "linux", path = "linux/file_dialog.rs")]
pub mod file_dialog;
#[cfg(target_os = "linux")]
#[path = "linux/gtk_thread.rs"]
mod gtk_thread;
pub mod instance;
#[cfg_attr(target_os = "linux", path = "linux/notice.rs")]
pub mod notice;
#[cfg(target_os = "linux")]
#[path = "linux/process.rs"]
mod process;
#[cfg(windows)]
pub mod setup;
mod storage;
#[cfg_attr(target_os = "linux", path = "linux/window_frame.rs")]
pub mod window_frame;

#[cfg(unix)]
pub use storage::local_app_data;
pub use storage::{
    CacheFolder, DeletionJournal, JournalError, RecoveryError, RecoveryOffer, RecoveryOutcome,
    StartupStorage, WindowsDirectoryLease, offer_recovery, prepare_startup, read_deletion_log,
};

/// Process-wide settings the platform needs before anything else runs; nothing on Windows.
///
/// # Safety
/// Call first in `main`, before any other thread exists: on Linux it may set an environment
/// variable the page engine reads.
pub unsafe fn prepare_process() {
    #[cfg(target_os = "linux")]
    // SAFETY: forwarded from this function's own contract.
    unsafe {
        process::prepare();
    }
}

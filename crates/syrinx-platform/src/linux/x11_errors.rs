//! Syrinx's place in Xlib's process-wide error handling, installed by GTK's thread as it starts.

use std::{
    ffi::c_int,
    ptr,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
};

use x11_dl::xlib;

/// Records X errors on Syrinx's private display instead of letting them end the process.
///
/// Xlib calls one process-wide handler for every connection's errors, and its default ends
/// the process. GDK installs its own handler while `gtk::init` runs, and swaps a handler in
/// and out around each of its error traps, saving and restoring whatever was installed before.
/// `install` runs once, on GTK's thread, just after `gtk::init` and before any GDK trap, so
/// GDK's traps then save and restore Syrinx's handler. That handler records errors on the
/// registered private display, and hands every other display's errors to the handler GDK
/// installed. Apart from GDK's own push and pop, nothing changes it again; while a GDK trap is
/// pushed, private errors reach GDK's handler, which ignores them (`docs/dev/trapstoknow.md`,
/// "Linux").
pub(crate) struct ErrorTrap;

/// The handler that was installed before Syrinx's (GDK's), set once by `ErrorTrap::install`.
static PREVIOUS_HANDLER: OnceLock<Option<ErrorHandler>> = OnceLock::new();
/// The private connection's display, whose errors are recorded rather than passed on.
static PRIVATE_DISPLAY: AtomicPtr<xlib::Display> = AtomicPtr::new(ptr::null_mut());
/// Whether an X error arrived on the private display since the last `clear`.
static RECORDED_ERROR: AtomicBool = AtomicBool::new(false);

type ErrorHandler = unsafe extern "C" fn(*mut xlib::Display, *mut xlib::XErrorEvent) -> c_int;

impl ErrorTrap {
    /// Installs Syrinx's handler once; later calls do nothing. GTK's thread calls it just
    /// after `gtk::init`, and nothing else may call `XSetErrorHandler` afterwards.
    pub(crate) fn install() {
        PREVIOUS_HANDLER.get_or_init(|| {
            let Ok(xlib) = xlib::Xlib::open() else {
                return None;
            };
            unsafe { (xlib.XSetErrorHandler)(Some(Self::handle)) }
        });
    }

    /// Names the one private display whose errors are recorded.
    pub(crate) fn register_private_display(display: *mut xlib::Display) {
        PRIVATE_DISPLAY.store(display, Ordering::Release);
    }

    /// Forgets any recorded error, at the start of a trapped call.
    pub(crate) fn clear() {
        RECORDED_ERROR.store(false, Ordering::Release);
    }

    /// Whether an error was recorded since `clear`, forgetting it.
    pub(crate) fn take_error() -> bool {
        RECORDED_ERROR.swap(false, Ordering::AcqRel)
    }

    unsafe extern "C" fn handle(
        display: *mut xlib::Display,
        event: *mut xlib::XErrorEvent,
    ) -> c_int {
        if display == PRIVATE_DISPLAY.load(Ordering::Acquire) {
            RECORDED_ERROR.store(true, Ordering::Release);
            return 0;
        }
        // Xlib always reports a previous handler, its own default at least. Without one there
        // is nothing to hand the error to, and ignoring it beats ending the process.
        let Some(previous) = PREVIOUS_HANDLER.get().copied().flatten() else {
            return 0;
        };
        unsafe { previous(display, event) }
    }
}

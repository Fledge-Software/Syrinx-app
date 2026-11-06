//! Measures palette acceptance against the real X11 display and process CPU counters.

use std::time::Duration;

#[path = "acceptance/window.rs"]
mod fixture_window;
#[path = "acceptance/probes.rs"]
mod probes;
#[path = "acceptance/x11.rs"]
mod x11;
pub use fixture_window::FixtureWindow;

#[cfg(test)]
#[path = "acceptance_tests.rs"]
mod tests;

/// Runs the real-focus fixture separately from parallel unit tests, on GTK's thread.
pub fn check_native_probes() -> Result<(), String> {
    crate::gtk_thread::run(probes::run).map_err(|error| error.to_string())?
}

/// Read the engine's actual Downloads destination on the one native-library thread.
pub fn fixture_download_directory() -> Result<Option<std::path::PathBuf>, String> {
    crate::gtk_thread::run(|| gtk::glib::user_special_dir(gtk::glib::UserDirectory::Downloads))
        .map_err(|error| error.to_string())
}

/// Linux dispatches committed text and key events through GPUI's own window dispatcher.
/// X11 has no per-window Unicode message equivalent to Win32 WM_CHAR; global keymap
/// remapping would alter the owner's keyboard. Native focus/stacking remain X11 probes.
pub const DISPATCH_INPUT_IN_WINDOW: bool = true;

/// A key the acceptance run presses in the palette.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Enter,
    Escape,
    Backspace,
}

/// Why a probe could not run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeError {
    /// The handle is not a window, or belongs to another process.
    NotSyrinxWindow,
    PostFailed,
    /// The server reported an X error other than a stale window ID.
    X11Error(i32),
    /// X11 did not report the monitor or its current display mode.
    DisplayUnreadable,
    /// X11 reported a refresh rate outside 24 to 1000 Hz.
    ImplausibleRefresh(u32),
    /// X11 did not report the interface thread's focus window.
    FocusUnreadable,
    /// X11 did not report this process's CPU time.
    CpuTimeUnreadable,
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSyrinxWindow => {
                formatter.write_str("The window is not one of this Syrinx process's windows.")
            }
            Self::PostFailed => formatter.write_str("X11 has no per-window Unicode input message; the Linux harness dispatches through GPUI."),
            Self::X11Error(code) => write!(formatter, "The X11 probe failed with server error {code}."),
            Self::DisplayUnreadable => {
                formatter.write_str("X11 did not report the display's current mode.")
            }
            Self::CpuTimeUnreadable => formatter.write_str("getrusage did not report Syrinx's CPU time."),
            Self::FocusUnreadable => {
                formatter.write_str("X11 did not report which window has keyboard focus.")
            }
            Self::ImplausibleRefresh(hertz) => {
                write!(
                    formatter,
                    "X11 reported an implausible {hertz} Hz refresh rate."
                )
            }
        }
    }
}

/// What X11 reports about an overlay popup's ownership and stacking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayStacking {
    pub owner: isize,
    pub topmost: bool,
    pub visible: bool,
    /// Whether the popup sits above `main` in the z-order.
    pub above_main: bool,
}

/// Which window holds X11 keyboard focus for Syrinx's interface thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum KeyboardFocus {
    /// No window on the thread's input queue has focus.
    Nobody,
    /// The main window itself, so keys go to GPUI rather than the page.
    MainWindow,
    /// A descendant of the main window: the page host or a WebKit child.
    InsideMainWindow,
    /// The overlay popup passed to the query.
    Popup,
    Elsewhere,
}

impl KeyboardFocus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nobody => "nobody",
            Self::MainWindow => "main window",
            Self::InsideMainWindow => "inside main window",
            Self::Popup => "popup",
            Self::Elsewhere => "elsewhere",
        }
    }
}

/// Reads the real transient parent, visibility and X11 stacking order.
pub fn overlay_stacking(popup: isize, main: isize) -> Result<OverlayStacking, ProbeError> {
    x11::stacking(popup, main)
}

/// Reads X11 focus and classifies a page child separately from the GPUI window.
pub fn keyboard_focus(main: isize, popup: Option<isize>) -> Result<KeyboardFocus, ProbeError> {
    x11::focus(main, popup)
}

/// The CPU time Syrinx's own process has used so far, user and system together.
pub fn process_cpu_time() -> Result<Duration, ProbeError> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: the pointer names a live local of the type the C library fills.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return Err(ProbeError::CpuTimeUnreadable);
    }
    let usage = unsafe { usage.assume_init() };
    let micros = |time: libc::timeval| {
        u64::try_from(time.tv_sec).unwrap_or(0) * 1_000_000
            + u64::try_from(time.tv_usec).unwrap_or(0)
    };
    Ok(Duration::from_micros(
        micros(usage.ru_utime) + micros(usage.ru_stime),
    ))
}

/// Whether the server still recognizes this window ID.
pub fn window_exists(window: isize) -> bool {
    x11::exists(window)
}

/// The nearest Syrinx ancestor of the real X11 focus window, or zero.
pub fn foreground_window() -> isize {
    x11::foreground()
}

/// Reads the actual monitor refresh rate through GDK; unknown rates remain errors.
pub fn refresh_interval(window: isize) -> Result<Duration, ProbeError> {
    x11::refresh(window)
}

/// Unsupported on Linux: the acceptance run posts Win32 keyboard messages.
pub fn post_text(_window: isize, _text: &str) -> Result<(), ProbeError> {
    Err(ProbeError::PostFailed)
}

/// Unsupported on Linux: the acceptance run posts Win32 keyboard messages.
pub fn post_key(_window: isize, _key: Key) -> Result<(), ProbeError> {
    Err(ProbeError::PostFailed)
}

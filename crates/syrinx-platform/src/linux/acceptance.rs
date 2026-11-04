//! P1-33's palette acceptance probes; the run drives Win32 input, so Linux answers only CPU time.

use std::time::Duration;

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
    /// Windows did not report the monitor or its current display mode.
    DisplayUnreadable,
    /// Windows reported a refresh rate outside 24 to 1000 Hz.
    ImplausibleRefresh(u32),
    /// Windows did not report the interface thread's focus window.
    FocusUnreadable,
    /// Windows did not report this process's CPU time.
    CpuTimeUnreadable,
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSyrinxWindow => {
                formatter.write_str("The window is not one of this Syrinx process's windows.")
            }
            Self::PostFailed => formatter.write_str("Windows refused the posted input message."),
            Self::DisplayUnreadable => {
                formatter.write_str("Windows did not report the display's current mode.")
            }
            Self::CpuTimeUnreadable => {
                formatter.write_str("Windows did not report Syrinx's CPU time.")
            }
            Self::FocusUnreadable => {
                formatter.write_str("Windows did not report which window has keyboard focus.")
            }
            Self::ImplausibleRefresh(hertz) => {
                write!(
                    formatter,
                    "Windows reported an implausible {hertz} Hz refresh rate."
                )
            }
        }
    }
}

/// What Windows reports about an overlay popup's ownership and stacking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayStacking {
    pub owner: isize,
    pub topmost: bool,
    pub visible: bool,
    /// Whether the popup sits above `main` in the z-order.
    pub above_main: bool,
}

/// Which window holds Windows keyboard focus for Syrinx's interface thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum KeyboardFocus {
    /// No window on the thread's input queue has focus.
    Nobody,
    /// The main window itself, so keys go to GPUI rather than the page.
    MainWindow,
    /// A descendant of the main window: the page host or a WebView2 child.
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

/// Unsupported on Linux: X11 stacking is the window manager's, not Syrinx's.
pub fn overlay_stacking(_popup: isize, _main: isize) -> Result<OverlayStacking, ProbeError> {
    Err(ProbeError::NotSyrinxWindow)
}

/// Unsupported on Linux: the acceptance run reads Win32 keyboard focus.
pub fn keyboard_focus(_main: isize, _popup: Option<isize>) -> Result<KeyboardFocus, ProbeError> {
    Err(ProbeError::FocusUnreadable)
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

/// Whether the handle names a window; X11 ids are not checked against the server here.
pub fn window_exists(window: isize) -> bool {
    window != 0
}

/// Unknown on Linux; the acceptance run uses this only for diagnostics.
pub fn foreground_window() -> isize {
    0
}

/// Unsupported on Linux: the acceptance run reads the Windows display mode.
pub fn refresh_interval(_window: isize) -> Result<Duration, ProbeError> {
    Err(ProbeError::DisplayUnreadable)
}

/// Unsupported on Linux: the acceptance run posts Win32 keyboard messages.
pub fn post_text(_window: isize, _text: &str) -> Result<(), ProbeError> {
    Err(ProbeError::PostFailed)
}

/// Unsupported on Linux: the acceptance run posts Win32 keyboard messages.
pub fn post_key(_window: isize, _key: Key) -> Result<(), ProbeError> {
    Err(ProbeError::PostFailed)
}

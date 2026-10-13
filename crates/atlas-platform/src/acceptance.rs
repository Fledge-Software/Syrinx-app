//! Win32 queries and posted input for P1-33's palette acceptance run, on Atlas's own windows only.

use std::time::Duration;

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, GetMonitorInfoW,
        MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromWindow,
    },
    UI::{
        Input::KeyboardAndMouse::{MAPVK_VK_TO_VSC, MapVirtualKeyW, VK_BACK, VK_ESCAPE, VK_RETURN},
        WindowsAndMessaging::{
            GUITHREADINFO, GW_HWNDPREV, GW_OWNER, GWL_EXSTYLE, GetForegroundWindow,
            GetGUIThreadInfo, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsChild,
            IsWindow, IsWindowVisible, PostMessageW, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WS_EX_TOPMOST,
        },
    },
};

/// Refresh rates outside this range are treated as unreadable rather than trusted.
const PLAUSIBLE_HZ: std::ops::RangeInclusive<u32> = 24..=1000;
/// A z-order walk stops here, so a changing window list cannot loop forever.
const Z_ORDER_LIMIT: usize = 4096;

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
    NotAtlasWindow,
    PostFailed,
    /// Windows did not report the monitor or its current display mode.
    DisplayUnreadable,
    /// Windows reported a refresh rate outside 24 to 1000 Hz.
    ImplausibleRefresh(u32),
    /// Windows did not report the interface thread's focus window.
    FocusUnreadable,
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAtlasWindow => {
                formatter.write_str("The window is not one of this Atlas process's windows.")
            }
            Self::PostFailed => formatter.write_str("Windows refused the posted input message."),
            Self::DisplayUnreadable => {
                formatter.write_str("Windows did not report the display's current mode.")
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

/// Reads the popup's owner, topmost style, visibility and position relative to `main`.
pub fn overlay_stacking(popup: isize, main: isize) -> Result<OverlayStacking, ProbeError> {
    own_window(popup)?;
    own_window(main)?;
    let handle = hwnd(popup);
    let owner = unsafe { GetWindow(handle, GW_OWNER) }.map_or(0, |owner| owner.0 as isize);
    let style = unsafe { GetWindowLongPtrW(handle, GWL_EXSTYLE) };
    Ok(OverlayStacking {
        owner,
        topmost: style as u32 & WS_EX_TOPMOST.0 != 0,
        visible: unsafe { IsWindowVisible(handle) }.as_bool(),
        above_main: is_above(popup, main),
    })
}

/// Which window holds Windows keyboard focus for Atlas's interface thread.
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

/// Reads the focus window of the main window's input queue and says where it is.
pub fn keyboard_focus(main: isize, popup: Option<isize>) -> Result<KeyboardFocus, ProbeError> {
    own_window(main)?;
    let thread = unsafe { GetWindowThreadProcessId(hwnd(main), None) };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread, &mut info) }.map_err(|_| ProbeError::FocusUnreadable)?;
    let focus = info.hwndFocus;
    Ok(match focus.0 as isize {
        0 => KeyboardFocus::Nobody,
        handle if handle == main => KeyboardFocus::MainWindow,
        handle if Some(handle) == popup => KeyboardFocus::Popup,
        _ if unsafe { IsChild(hwnd(main), focus) }.as_bool() => KeyboardFocus::InsideMainWindow,
        _ => KeyboardFocus::Elsewhere,
    })
}

/// Whether `upper` appears before `lower` when walking up the z-order from `lower`.
fn is_above(upper: isize, lower: isize) -> bool {
    let mut current = hwnd(lower);
    for _ in 0..Z_ORDER_LIMIT {
        let Ok(previous) = (unsafe { GetWindow(current, GW_HWNDPREV) }) else {
            return false;
        };
        if previous.0 as isize == upper {
            return true;
        }
        current = previous;
    }
    false
}

/// Whether the handle still names a live window.
pub fn window_exists(window: isize) -> bool {
    window != 0 && unsafe { IsWindow(Some(hwnd(window))) }.as_bool()
}

/// The current foreground window, for diagnostics; Windows may deny Atlas activation.
pub fn foreground_window() -> isize {
    unsafe { GetForegroundWindow() }.0 as isize
}

/// The display refresh interval of the monitor showing one of Atlas's windows.
pub fn refresh_interval(window: isize) -> Result<Duration, ProbeError> {
    own_window(window)?;
    let monitor = unsafe { MonitorFromWindow(hwnd(window), MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    let read = unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast::<MONITORINFO>()) };
    if !read.as_bool() {
        return Err(ProbeError::DisplayUnreadable);
    }
    let mut mode = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    let device = windows::core::PCWSTR(info.szDevice.as_ptr());
    if !unsafe { EnumDisplaySettingsW(device, ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool() {
        return Err(ProbeError::DisplayUnreadable);
    }
    let hertz = mode.dmDisplayFrequency;
    if !PLAUSIBLE_HZ.contains(&hertz) {
        return Err(ProbeError::ImplausibleRefresh(hertz));
    }
    Ok(Duration::from_secs_f64(1. / f64::from(hertz)))
}

/// Posts typed text to one Atlas window as `WM_CHAR` messages, one UTF-16 unit each.
pub fn post_text(window: isize, text: &str) -> Result<(), ProbeError> {
    own_window(window)?;
    for unit in text.encode_utf16() {
        post(window, WM_CHAR, usize::from(unit), 1)?;
    }
    Ok(())
}

/// Posts one key press and release to one Atlas window; GPUI reads it as a real key.
pub fn post_key(window: isize, key: Key) -> Result<(), ProbeError> {
    own_window(window)?;
    let virtual_key = match key {
        Key::Enter => VK_RETURN,
        Key::Escape => VK_ESCAPE,
        Key::Backspace => VK_BACK,
    };
    let scan = unsafe { MapVirtualKeyW(u32::from(virtual_key.0), MAPVK_VK_TO_VSC) };
    let down = 1 | (scan as isize) << 16;
    // Bits 30 and 31 mark a release of a key that was down.
    let up = down | 1 << 30 | 1 << 31;
    post(window, WM_KEYDOWN, usize::from(virtual_key.0), down)?;
    post(window, WM_KEYUP, usize::from(virtual_key.0), up)
}

fn post(window: isize, message: u32, wparam: usize, lparam: isize) -> Result<(), ProbeError> {
    unsafe { PostMessageW(Some(hwnd(window)), message, WPARAM(wparam), LPARAM(lparam)) }
        .map_err(|_| ProbeError::PostFailed)
}

/// Refuses any handle that is not a live window of this process, so input never leaves Atlas.
fn own_window(window: isize) -> Result<(), ProbeError> {
    if !window_exists(window) {
        return Err(ProbeError::NotAtlasWindow);
    }
    let mut process = 0;
    let thread = unsafe { GetWindowThreadProcessId(hwnd(window), Some(&raw mut process)) };
    if thread == 0 || process != std::process::id() {
        return Err(ProbeError::NotAtlasWindow);
    }
    Ok(())
}

fn hwnd(window: isize) -> HWND {
    HWND(window as *mut _)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_refused_for_windows_that_are_not_atlas_windows() {
        // The desktop shell's foreground window belongs to another process, or there is none.
        let foreground = foreground_window();
        assert_eq!(
            post_text(foreground, "x"),
            Err(ProbeError::NotAtlasWindow),
            "posted input must never reach another process"
        );
        assert_eq!(post_key(0, Key::Enter), Err(ProbeError::NotAtlasWindow));
        assert_eq!(overlay_stacking(0, 0), Err(ProbeError::NotAtlasWindow));
        assert_eq!(refresh_interval(0), Err(ProbeError::NotAtlasWindow));
        assert_eq!(keyboard_focus(0, None), Err(ProbeError::NotAtlasWindow));
    }
}

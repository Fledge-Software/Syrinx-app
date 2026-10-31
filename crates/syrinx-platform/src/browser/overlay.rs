//! Samples the pointer across the main window and its owned popup without touching page content.

use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    UI::{
        HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
        Input::KeyboardAndMouse::{GetActiveWindow, GetAsyncKeyState, SetActiveWindow, VK_LBUTTON},
        WindowsAndMessaging::{
            GA_ROOT, GetAncestor, GetCursorPos, GetForegroundWindow, GetWindowInfo, GetWindowRect,
            GetWindowThreadProcessId, HWND_TOP, IsIconic, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE,
            SWP_NOOWNERZORDER, SetWindowPos, ShowWindow, WINDOWINFO,
        },
    },
};

use super::{BrowserError, OverlayMonitor, OverlayPointer, OverlayRect, validate_window};

/// Native coordinates keep leave detection working over the page child and outside Syrinx.
pub fn overlay_pointer(parent: isize, popup: Option<isize>) -> Result<OverlayPointer, String> {
    super::validate_window(parent).map_err(|error| format!("Sidebar owner: {error}"))?;
    let parent = HWND(parent as *mut _);
    let mut pointer = POINT::default();
    let mut info = WINDOWINFO {
        cbSize: std::mem::size_of::<WINDOWINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetCursorPos(&mut pointer).map_err(|error| native_error("Read sidebar pointer", error))?;
        GetWindowInfo(parent, &mut info)
            .map_err(|error| native_error("Locate sidebar owner", error))?;
        let origin = POINT {
            x: info.rcClient.left,
            y: info.rcClient.top,
        };
        let foreground = GetAncestor(GetForegroundWindow(), GA_ROOT);
        let popup = popup.map(|handle| HWND(handle as *mut _));
        let mut in_popup = false;
        if let Some(popup) = popup {
            let mut bounds = RECT::default();
            GetWindowRect(popup, &mut bounds)
                .map_err(|error| native_error("Locate sidebar popup", error))?;
            in_popup = (bounds.left..bounds.right).contains(&pointer.x)
                && (bounds.top..bounds.bottom).contains(&pointer.y);
        }
        Ok(OverlayPointer {
            x: pointer.x - origin.x,
            y: pointer.y - origin.y,
            in_popup,
            active: !IsIconic(parent).as_bool()
                && (foreground == parent || Some(foreground) == popup),
            pressed: GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0,
        })
    }
}

pub fn overlay_monitor(x: i32, y: i32) -> Result<OverlayMonitor, String> {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        GetMonitorInfoW(monitor, &mut info)
            .ok()
            .map_err(|error| native_error("Locate sidebar display", error))?;
        let mut dpi_x = 0;
        let mut dpi_y = 0;
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y)
            .map_err(|error| native_error("Read sidebar display scale", error))?;
        Ok(OverlayMonitor {
            id: monitor.0 as u64,
            scale: dpi_x as f32 / 96.,
            left: info.rcMonitor.left,
            top: info.rcMonitor.top,
            right: info.rcMonitor.right,
            bottom: info.rcMonitor.bottom,
        })
    }
}

/// Moves a kept popup to `rect` and shows it above its owner without activating it.
///
/// Call this outside any GPUI update: the popup's resize and first frame are delivered from
/// these Win32 calls and GPUI drops them while its app state is borrowed.
pub fn show_overlay(popup: isize, rect: OverlayRect) -> Result<(), BrowserError> {
    validate_window(popup)?;
    let handle = HWND(popup as *mut _);
    let scale_before = unsafe { GetDpiForWindow(handle) };
    place_overlay(handle, rect)?;
    // Crossing to a monitor with another scale makes Windows resize the popup; restore `rect`.
    if unsafe { GetDpiForWindow(handle) } != scale_before {
        place_overlay(handle, rect)?;
    }
    unsafe {
        let _previous_visibility = ShowWindow(handle, SW_SHOWNA);
    }
    Ok(())
}

/// Hides a kept popup; when it is active, its owner is activated first, synchronously.
///
/// Hiding the active window would let Windows choose what to activate next, possibly after
/// the page was given focus.
pub fn hide_overlay(popup: isize, owner: isize) -> Result<(), BrowserError> {
    validate_window(popup)?;
    validate_window(owner)?;
    let popup = HWND(popup as *mut _);
    if unsafe { GetActiveWindow() } == popup {
        activate(HWND(owner as *mut _))?;
    }
    unsafe {
        let _previous_visibility = ShowWindow(popup, SW_HIDE);
    }
    Ok(())
}

/// Whether one of this process's windows has the foreground.
pub fn syrinx_in_foreground() -> bool {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() {
        return false;
    }
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(foreground, Some(&raw mut process)) };
    process == std::process::id()
}

/// Activates one of Syrinx's windows at once when Syrinx already has the foreground.
///
/// Returns `false`, changing nothing, when another application is in front: Syrinx never
/// takes the foreground from another application (owner decision, 2026-10-12).
pub fn activate_if_foreground(window: isize) -> Result<bool, BrowserError> {
    validate_window(window)?;
    if !syrinx_in_foreground() {
        return Ok(false);
    }
    activate(HWND(window as *mut _))?;
    Ok(true)
}

/// `SetActiveWindow` returns the previous active window, and the bindings report a NULL
/// previous window as an error; the call succeeded when `window` is now active.
fn activate(window: HWND) -> Result<(), BrowserError> {
    if unsafe { SetActiveWindow(window) }.is_ok() || unsafe { GetActiveWindow() } == window {
        return Ok(());
    }
    Err(BrowserError::Focus)
}

fn place_overlay(popup: HWND, rect: OverlayRect) -> Result<(), BrowserError> {
    unsafe {
        SetWindowPos(
            popup,
            Some(HWND_TOP),
            rect.left,
            rect.top,
            rect.width,
            rect.height,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        )
    }
    .map_err(|_| BrowserError::OverlayPlacement)
}

fn native_error(operation: &str, error: windows::core::Error) -> String {
    format!(
        "{operation}: HRESULT {:#010X}: {}",
        error.code().0,
        error.message()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use windows::{
        Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GWL_EXSTYLE, GWLP_HWNDPARENT, GetWindowLongPtrW,
            IsWindow, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP,
        },
        core::w,
    };

    #[test]
    fn kept_popup_calls_refuse_handles_that_are_not_windows() {
        let rect = OverlayRect {
            left: 0,
            top: 0,
            width: 10,
            height: 10,
        };
        assert_eq!(show_overlay(0, rect), Err(BrowserError::InvalidWindow));
        assert_eq!(hide_overlay(0, 0), Err(BrowserError::InvalidWindow));
        assert_eq!(activate_if_foreground(0), Err(BrowserError::InvalidWindow));
    }

    #[test]
    fn native_failures_keep_operation_hresult_and_message() {
        let error = windows::core::Error::new(
            windows::core::HRESULT(0x80070005u32 as i32),
            "Synthetic denial",
        );
        assert_eq!(
            native_error("Locate sidebar popup", error),
            "Locate sidebar popup: HRESULT 0x80070005: Synthetic denial"
        );
    }

    /// Test-owned, never shown; the owner may already have destroyed its popup.
    struct TestWindow(HWND);

    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe {
                if IsWindow(Some(self.0)).as_bool() {
                    DestroyWindow(self.0).expect("destroy synthetic window");
                }
            }
        }
    }

    #[test]
    fn an_owned_overlay_is_not_topmost_and_cannot_outlive_its_owner() {
        unsafe {
            let parent = TestWindow(
                CreateWindowExW(
                    Default::default(),
                    w!("STATIC"),
                    w!("Synthetic owner"),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    100,
                    100,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("create hidden synthetic owner"),
            );
            let popup = TestWindow(
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                    w!("STATIC"),
                    w!("Synthetic popup"),
                    WS_POPUP,
                    0,
                    0,
                    100,
                    100,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("create hidden synthetic popup"),
            );
            super::super::set_overlay_owner(popup.0.0 as isize, parent.0.0 as isize)
                .expect("apply the palette and peek ownership helper");
            assert_eq!(
                GetWindowLongPtrW(popup.0, GWLP_HWNDPARENT),
                parent.0.0 as isize
            );
            assert_eq!(
                GetWindowLongPtrW(popup.0, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize,
                0
            );
            drop(parent);
            assert!(
                !IsWindow(Some(popup.0)).as_bool(),
                "the owner destroys its popup"
            );
        }
    }
}

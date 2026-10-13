//! Gives Syrinx's windows the hawk icon compiled into `syrinx.exe`, at sizes matching their DPI.

use std::fmt;
use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    ICON_BIG, ICON_SMALL, IMAGE_ICON, IsWindow, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON,
    SM_CXSMICON, SendMessageW, WM_SETICON,
};
use windows::core::PCWSTR;

/// The icon resource `crates/syrinx-app/build.rs` compiles, and the registry's `syrinx.exe,0`.
const ICON_RESOURCE: u16 = 1;

/// Loaded icons by pixel size. They live as long as the process, so windows can share them.
static LOADED: Mutex<Vec<(i32, isize)>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconError {
    /// The handle is not a live window.
    InvalidWindow,
    /// The executable's module handle was unavailable.
    Module,
    /// The executable has no icon resource 1 at this size, as in test binaries.
    Load { size: i32 },
}

impl fmt::Display for IconError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWindow => write!(formatter, "The window for the Syrinx icon is gone."),
            Self::Module => write!(formatter, "Syrinx's executable module is unavailable."),
            Self::Load { size } => write!(formatter, "Cannot load the {size} px Syrinx icon."),
        }
    }
}

impl std::error::Error for IconError {}

/// Sets the title-bar (small) and taskbar/Alt Tab (big) icons for one window.
///
/// GPUI's window class already carries icon 1 at the system's default size; this picks the
/// sizes for the window's own DPI, so the taskbar does not scale a 32 px icon up.
pub fn apply(window: isize) -> Result<(), IconError> {
    let hwnd = HWND(window as *mut _);
    if window == 0 || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(IconError::InvalidWindow);
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let big = icon(unsafe { GetSystemMetricsForDpi(SM_CXICON, dpi) })?;
    let small = icon(unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) })?;
    // WM_SETICON returns the previous per-window icon: none, or one of these cached ones, so
    // there is nothing to free.
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(big)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(small)),
        );
    }
    Ok(())
}

/// Loads icon 1 at `size` pixels once, picking the closest entry in the `.ico`.
fn icon(size: i32) -> Result<isize, IconError> {
    // Each entry is pushed whole, so a panic elsewhere cannot leave the cache half-written.
    let mut loaded = LOADED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, handle)) = loaded.iter().find(|(loaded_size, _)| *loaded_size == size) {
        return Ok(*handle);
    }
    let module = unsafe { GetModuleHandleW(None) }.map_err(|_| IconError::Module)?;
    let handle = unsafe {
        LoadImageW(
            Some(module.into()),
            PCWSTR(ICON_RESOURCE as usize as *const u16),
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    }
    .map_err(|_| IconError::Load { size })?;
    loaded.push((size, handle.0 as isize));
    Ok(handle.0 as isize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_window_is_refused_before_loading() {
        assert_eq!(apply(0), Err(IconError::InvalidWindow));
    }

    #[test]
    fn a_binary_without_the_resource_reports_the_size() {
        // Test binaries are not linked with syrinx.res (build.rs links binaries only).
        assert_eq!(icon(32), Err(IconError::Load { size: 32 }));
    }
}

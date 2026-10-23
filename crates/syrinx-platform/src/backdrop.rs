//! Chooses the Windows 11 material drawn behind a Syrinx window: Mica, Mica Alt or Acrylic.
//! GPUI offers only Mica and Mica Alt, and its `Blurred` look is not Acrylic.

use windows::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{
        DWM_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW, DWMSBT_TABBEDWINDOW, DWMSBT_TRANSIENTWINDOW,
        DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
    },
};
use windows::core::BOOL;

/// The system backdrops Windows 11 (build 22621 and later) can draw behind a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// Mica: the wallpaper's colours, heavily blurred and muted.
    Mica,
    /// Mica Alt: Mica with more of the wallpaper's colour, as tabbed windows use.
    MicaAlt,
    /// Acrylic: a frosted blur of whatever is behind the window, as menus use.
    Acrylic,
}

impl Backdrop {
    fn system_type(self) -> DWM_SYSTEMBACKDROP_TYPE {
        match self {
            Self::Mica => DWMSBT_MAINWINDOW,
            Self::MicaAlt => DWMSBT_TABBEDWINDOW,
            Self::Acrylic => DWMSBT_TRANSIENTWINDOW,
        }
    }
}

/// Draw `backdrop` behind the window `hwnd`, in its dark form for a dark theme and its light
/// form for a light one; a light theme over dark Acrylic looks muddy (#93).
///
/// The window must leave the backdrop visible, as GPUI's Mica appearance does by clearing
/// its frame to transparent.
pub fn apply(hwnd: isize, backdrop: Backdrop, dark: bool) -> windows::core::Result<()> {
    let window = HWND(hwnd as *mut _);
    let dark = BOOL::from(dark);
    let kind = backdrop.system_type();
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const dark).cast(),
            size_of_val(&dark) as u32,
        )?;
        DwmSetWindowAttribute(
            window,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&raw const kind).cast(),
            size_of_val(&kind) as u32,
        )
    }
}

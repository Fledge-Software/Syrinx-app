//! Gives a borderless Syrinx popup Windows 11's rounded corners and system shadow.

use windows::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
        DwmSetWindowAttribute,
    },
    UI::Controls::MARGINS,
};

/// Round the window's corners (8 px, the closest system size to the design's 10 px menu
/// radius) and give it the system drop shadow.
///
/// DWM draws no shadow for a borderless window by itself; extending the frame by one pixel
/// into the client area is what asks it to, and it follows the rounded outline.
pub fn round_with_shadow(hwnd: isize) -> windows::core::Result<()> {
    let window = HWND(hwnd as *mut _);
    let corners = DWMWCP_ROUND;
    let frame = MARGINS {
        cxLeftWidth: 0,
        cxRightWidth: 0,
        cyTopHeight: 1,
        cyBottomHeight: 0,
    };
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corners).cast(),
            size_of_val(&corners) as u32,
        )?;
        DwmExtendFrameIntoClientArea(window, &frame)
    }
}

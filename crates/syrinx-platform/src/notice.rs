//! Shows a start-up problem in a message box, because a launch from Windows has no console.

use windows::{
    Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW},
    core::{HSTRING, w},
};

/// Shows `text` and waits until the owner closes the box.
pub fn show_warning(text: &str) {
    // The only answer is OK, so the returned button carries no information.
    let _closed = unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            w!("Syrinx"),
            MB_OK | MB_ICONWARNING,
        )
    };
}

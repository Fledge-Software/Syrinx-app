//! Samples the pointer across the main window and its owned popup without touching page content.

use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
        WindowsAndMessaging::{
            GA_ROOT, GetAncestor, GetCursorPos, GetForegroundWindow, GetWindowInfo, GetWindowRect,
            IsIconic, WINDOWINFO,
        },
    },
};

pub struct OverlayPointer {
    pub x: i32,
    pub y: i32,
    pub client_screen_x: i32,
    pub client_screen_y: i32,
    pub in_popup: bool,
    pub active: bool,
    pub pressed: bool,
}

/// Native coordinates keep leave detection working over the page child and outside Atlas.
pub fn overlay_pointer(
    parent: isize,
    popup: Option<isize>,
) -> Result<OverlayPointer, &'static str> {
    super::validate_window(parent).map_err(|_| "The sidebar owner window is unavailable.")?;
    let parent = HWND(parent as *mut _);
    let mut pointer = POINT::default();
    let mut info = WINDOWINFO {
        cbSize: std::mem::size_of::<WINDOWINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetCursorPos(&mut pointer).map_err(|_| "Cannot read the sidebar pointer position.")?;
        GetWindowInfo(parent, &mut info).map_err(|_| "Cannot locate the sidebar owner window.")?;
        let origin = POINT {
            x: info.rcClient.left,
            y: info.rcClient.top,
        };
        let foreground = GetAncestor(GetForegroundWindow(), GA_ROOT);
        let popup = popup.map(|handle| HWND(handle as *mut _));
        let mut in_popup = false;
        if let Some(popup) = popup {
            let mut bounds = RECT::default();
            GetWindowRect(popup, &mut bounds).map_err(|_| "Cannot locate the sidebar popup.")?;
            in_popup = (bounds.left..bounds.right).contains(&pointer.x)
                && (bounds.top..bounds.bottom).contains(&pointer.y);
        }
        Ok(OverlayPointer {
            x: pointer.x - origin.x,
            y: pointer.y - origin.y,
            client_screen_x: origin.x,
            client_screen_y: origin.y,
            in_popup,
            active: !IsIconic(parent).as_bool()
                && (foreground == parent || Some(foreground) == popup),
            pressed: GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0,
        })
    }
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

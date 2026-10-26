//! Checks the frame decisions, and that hidden synthetic windows follow them natively.

use super::*;

use windows::{
    Win32::{
        Foundation::{ERROR_SUCCESS, GetLastError, SetLastError},
        UI::{
            Input::KeyboardAndMouse::{GetActiveWindow, SetActiveWindow},
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, GWLP_HWNDPARENT, GetWindowInfo, IsWindow,
                SetWindowLongPtrW, WINDOWINFO, WS_ACTIVECAPTION, WS_EX_TOOLWINDOW,
                WS_OVERLAPPEDWINDOW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

#[test]
fn moving_between_syrinx_windows_keeps_every_frame_active() {
    let stays = Some(FrameChange {
        paint_active: true,
        tell_family: None,
    });
    assert_eq!(frame_change(false, Counterpart::Family), stays);
    assert_eq!(frame_change(true, Counterpart::Family), stays);
}

#[test]
fn another_application_taking_activation_dims_the_whole_family() {
    assert_eq!(
        frame_change(false, Counterpart::Outside),
        Some(FrameChange {
            paint_active: false,
            tell_family: Some(false),
        })
    );
}

#[test]
fn returning_from_another_application_lights_the_whole_family() {
    assert_eq!(
        frame_change(true, Counterpart::Outside),
        Some(FrameChange {
            paint_active: true,
            tell_family: Some(true),
        })
    );
}

#[test]
fn a_no_repaint_request_passes_through_unchanged() {
    assert_eq!(frame_change(false, Counterpart::NoRepaint), None);
    assert_eq!(frame_change(true, Counterpart::NoRepaint), None);
}

/// Test-owned and never shown; the owner may already have destroyed its popup.
struct TestWindow(HWND);

impl TestWindow {
    fn create(class: PCWSTR, title: PCWSTR, popup: bool) -> Self {
        let (extended, style) = if popup {
            (WS_EX_TOOLWINDOW, WS_POPUP)
        } else {
            (Default::default(), WS_OVERLAPPEDWINDOW)
        };
        let hwnd = unsafe {
            CreateWindowExW(
                extended, class, title, style, 0, 0, 100, 100, None, None, None, None,
            )
        }
        .expect("create a hidden synthetic window");
        Self(hwnd)
    }

    fn handle(&self) -> isize {
        self.0.0 as isize
    }

    /// Whether Windows currently draws this window's frame as active.
    fn frame_active(&self) -> bool {
        let mut info = WINDOWINFO {
            cbSize: size_of::<WINDOWINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetWindowInfo(self.0, &mut info) }.expect("read synthetic window info");
        info.dwWindowStatus == WS_ACTIVECAPTION.0
    }

    /// Make this the thread's active window, so Windows sends the activation messages itself.
    fn activate(&self) {
        // `SetActiveWindow` returns the previous active window, and NULL (an `Err` carrying
        // a success code) when there was none; only a real failure code is an error.
        if let Err(error) = unsafe { SetActiveWindow(self.0) } {
            assert!(error.code().is_ok(), "activate a synthetic window: {error}");
        }
        assert_eq!(
            unsafe { GetActiveWindow() },
            self.0,
            "the window became active"
        );
    }

    /// Deliver a `WM_NCACTIVATE` as Windows would, naming the other window involved.
    fn nc_activate(&self, active: bool, other: Option<&TestWindow>) {
        let other = other.map_or(0, TestWindow::handle);
        unsafe {
            SendMessageW(
                self.0,
                WM_NCACTIVATE,
                Some(WPARAM(usize::from(active))),
                Some(LPARAM(other)),
            );
        }
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(Some(self.0)).as_bool() {
                DestroyWindow(self.0).expect("destroy synthetic window");
            }
        }
    }
}

/// Make `popup` owned by `owner`, as Syrinx's popup helper does.
fn set_owner(popup: &TestWindow, owner: &TestWindow) {
    unsafe {
        SetLastError(ERROR_SUCCESS);
        let previous = SetWindowLongPtrW(popup.0, GWLP_HWNDPARENT, owner.handle());
        assert!(
            previous != 0 || GetLastError() == ERROR_SUCCESS,
            "set the synthetic owner"
        );
    }
}

/// A synthetic main window with two owned popups, each kept drawn with its family.
/// Bind `main` first when destructuring: locals drop in reverse, so the popups go first.
struct Family {
    palette: TestWindow,
    peek: TestWindow,
    main: TestWindow,
}

fn family() -> Family {
    let main = TestWindow::create(w!("STATIC"), w!("Synthetic main"), false);
    let palette = TestWindow::create(w!("STATIC"), w!("Synthetic palette"), true);
    let peek = TestWindow::create(w!("STATIC"), w!("Synthetic peek"), true);
    for popup in [&palette, &peek] {
        set_owner(popup, &main);
    }
    for window in [&main, &palette, &peek] {
        install(window.handle()).expect("keep the synthetic frame with its family");
    }
    Family {
        palette,
        peek,
        main,
    }
}

#[test]
fn the_main_window_stays_active_while_its_popup_takes_activation() {
    let Family { main, palette, .. } = family();
    main.nc_activate(true, None);
    assert!(main.frame_active());

    main.nc_activate(false, Some(&palette));
    palette.nc_activate(true, Some(&main));
    assert!(
        main.frame_active(),
        "the main window keeps its active frame"
    );
    assert!(palette.frame_active());
}

#[test]
fn another_application_dims_every_window_and_returning_lights_them() {
    let Family {
        main,
        palette,
        peek,
    } = family();
    main.nc_activate(true, None);
    assert!(
        peek.frame_active(),
        "a non-activating popup follows its family"
    );

    main.nc_activate(false, Some(&palette));
    palette.nc_activate(true, Some(&main));
    palette.nc_activate(false, None);
    assert!(!main.frame_active());
    assert!(!palette.frame_active());
    assert!(!peek.frame_active());

    main.nc_activate(true, None);
    assert!(main.frame_active());
    assert!(palette.frame_active());
    assert!(peek.frame_active());
}

#[test]
fn an_owned_window_without_the_subclass_is_left_to_windows() {
    let Family { main, peek, .. } = family();
    let dialog = TestWindow::create(w!("STATIC"), w!("Synthetic dialog"), true);
    set_owner(&dialog, &main);
    main.nc_activate(true, None);

    main.nc_activate(false, Some(&dialog));
    assert!(
        !main.frame_active(),
        "a file dialog or message box dims Syrinx"
    );
    assert!(!peek.frame_active());
    drop(dialog);
}

#[test]
fn an_unowned_window_is_left_to_windows() {
    let main = TestWindow::create(w!("STATIC"), w!("Synthetic main"), false);
    let stranger = TestWindow::create(w!("STATIC"), w!("Synthetic stranger"), false);
    install(main.handle()).expect("keep the synthetic frame with its family");
    main.nc_activate(true, None);
    stranger.nc_activate(true, None);

    main.nc_activate(false, Some(&stranger));
    assert!(!main.frame_active(), "a window outside the family dims it");
    assert!(
        stranger.frame_active(),
        "a window outside the family is not told"
    );
}

/// Windows itself, not the test, names the other window when activation moves inside this
/// thread. `SetActiveWindow` changes only this thread's active window, never the foreground.
#[test]
fn real_activation_of_a_popup_keeps_the_main_frame_active() {
    let Family { main, palette, .. } = family();
    main.activate();
    assert!(main.frame_active());

    palette.activate();
    assert!(
        main.frame_active(),
        "Windows named the palette as the window activated"
    );
    assert!(palette.frame_active());

    // The control: a window outside the family still dims both, through the same messages.
    let stranger = TestWindow::create(w!("STATIC"), w!("Synthetic stranger"), false);
    stranger.activate();
    assert!(!main.frame_active());
    assert!(!palette.frame_active());
}

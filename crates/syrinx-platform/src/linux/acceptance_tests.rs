//! Checks native X11 probe results against windows created only for this fixture.

use gtk::{gdk, prelude::*};
use x11_dl::xlib;

use super::*;

/// Two raw windows on GTK's connection, kept off the window manager's client list.
struct Windows {
    xlib: xlib::Xlib,
    display: *mut xlib::Display,
    live: Vec<xlib::Window>,
}

impl Windows {
    fn new() -> Result<Self, String> {
        let xlib = xlib::Xlib::open().map_err(|error| error.to_string())?;
        let display = gdk::Display::default()
            .ok_or("No GDK display")?
            .downcast::<gdkx11::X11Display>()
            .map_err(|_| "Not X11")?;
        let display = unsafe { gdkx11::ffi::gdk_x11_display_get_xdisplay(display.as_ptr()) }.cast();
        Ok(Self {
            xlib,
            display,
            live: Vec::new(),
        })
    }

    fn create(&mut self) -> Result<isize, String> {
        let root = unsafe { (self.xlib.XDefaultRootWindow)(self.display) };
        let window = unsafe {
            (self.xlib.XCreateSimpleWindow)(self.display, root, 10, 10, 100, 80, 0, 0, 0)
        };
        if window == 0 {
            return Err("No fixture window".into());
        }
        self.live.push(window);
        let mut attributes = unsafe { std::mem::zeroed::<xlib::XSetWindowAttributes>() };
        attributes.override_redirect = xlib::True;
        let process = std::ffi::c_ulong::from(std::process::id());
        unsafe {
            (self.xlib.XChangeWindowAttributes)(
                self.display,
                window,
                xlib::CWOverrideRedirect,
                &mut attributes,
            );
            let atom = (self.xlib.XInternAtom)(self.display, c"_NET_WM_PID".as_ptr(), xlib::False);
            (self.xlib.XChangeProperty)(
                self.display,
                window,
                atom,
                xlib::XA_CARDINAL,
                32,
                xlib::PropModeReplace,
                (&raw const process).cast(),
                1,
            );
            (self.xlib.XMapRaised)(self.display, window);
            (self.xlib.XSync)(self.display, xlib::False);
        }
        isize::try_from(window).map_err(|_| "Unrepresentable fixture window".into())
    }

    fn focus(&self, window: isize) {
        unsafe {
            (self.xlib.XSetInputFocus)(
                self.display,
                window as _,
                xlib::RevertToParent,
                xlib::CurrentTime,
            );
            (self.xlib.XSync)(self.display, xlib::False);
        }
    }

    fn destroy(&mut self, window: isize) {
        self.live.retain(|entry| *entry != window as xlib::Window);
        unsafe {
            (self.xlib.XDestroyWindow)(self.display, window as _);
            (self.xlib.XSync)(self.display, xlib::False);
        }
    }
}

impl Drop for Windows {
    fn drop(&mut self) {
        for window in &self.live {
            unsafe { (self.xlib.XDestroyWindow)(self.display, *window) };
        }
        unsafe { (self.xlib.XSync)(self.display, xlib::False) };
    }
}

#[test]
fn linux_probes_observe_real_focus_parent_stacking_and_window_destruction() {
    let result = crate::gtk_thread::run(|| -> Result<_, String> {
        let mut windows = Windows::new()?;
        let main = windows.create()?;
        let popup = windows.create()?;
        crate::browser::set_overlay_owner(popup, main).map_err(|error| error.to_string())?;
        windows.focus(main);
        let main_focus = keyboard_focus(main, Some(popup));
        windows.focus(popup);
        let popup_focus = keyboard_focus(main, Some(popup));
        let stacking = overlay_stacking(popup, main);
        let refresh = refresh_interval(main);
        windows.destroy(popup);
        let destroyed = window_exists(popup);
        Ok((main_focus, popup_focus, stacking, refresh, destroyed))
    })
    .expect("GTK thread")
    .expect("fixture windows");
    assert_eq!(result.0, Ok(KeyboardFocus::MainWindow));
    assert_eq!(result.1, Ok(KeyboardFocus::Popup));
    let stacking = result.2.expect("native stacking");
    assert_ne!(stacking.owner, 0);
    assert!(stacking.visible && stacking.above_main && !stacking.topmost);
    assert!(
        result.3.is_ok(),
        "native monitor refresh must be known on this machine"
    );
    assert!(!result.4, "a destroyed window must not be reported live");
}

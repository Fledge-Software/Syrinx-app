//! Exercises real desktop focus and visible stacking from the native page_close fixture.

use gtk::{gdk, prelude::*};
use x11_dl::xlib;

use super::*;
use crate::browser::{self, BrowserError};

/// Raw fixture windows on GTK's connection, kept off the window manager's client list.
pub(super) struct Windows {
    xlib: xlib::Xlib,
    display: *mut xlib::Display,
    live: Vec<xlib::Window>,
}

impl Windows {
    pub(super) fn new() -> Result<Self, String> {
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

    pub(super) fn create(&mut self, mapped: bool, ours: bool) -> Result<isize, String> {
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
            if ours {
                let atom =
                    (self.xlib.XInternAtom)(self.display, c"_NET_WM_PID".as_ptr(), xlib::False);
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
            }
            if mapped {
                (self.xlib.XMapRaised)(self.display, window);
            }
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

    fn has_keys(&self, window: isize) -> bool {
        let mut focused = 0;
        let mut revert = 0;
        unsafe { (self.xlib.XGetInputFocus)(self.display, &mut focused, &mut revert) };
        focused == window as xlib::Window
    }

    pub(super) fn destroy(&mut self, window: isize) {
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

/// Establish managed WM activation outside GTK, then check native transfers on GTK.
pub(super) fn run() -> Result<(), String> {
    let parent = FixtureWindow::new()?;
    parent.show()?;
    let main = parent.handle();
    crate::gtk_thread::run(move || positive_focus(main)).map_err(|error| error.to_string())??;

    // This owned synthetic window deliberately lacks Syrinx's PID marker.
    let foreign = FixtureWindow::unmarked()?;
    foreign.show()?;
    let foreign_id = foreign.handle();
    crate::gtk_thread::run(move || foreign_focus(main, foreign_id))
        .map_err(|error| error.to_string())??;
    println!(
        "Linux X11 probes: real focus guards, hidden/stale targets, ownership, stacking, refresh and destruction passed."
    );
    Ok(())
}

fn positive_focus(main: isize) -> Result<(), String> {
    let mut windows = Windows::new()?;
    let popup = windows.create(true, true)?;
    browser::set_overlay_owner(popup, main).map_err(|error| error.to_string())?;
    windows.focus(main);
    require(
        keyboard_focus(main, Some(popup)) == Ok(KeyboardFocus::MainWindow),
        "The real main window did not receive keyboard focus",
    )?;

    // A hidden or stale target must not disturb keys held by a live Syrinx window.
    let hidden = windows.create(false, true)?;
    browser::focus_chrome(hidden).map_err(|error| error.to_string())?;
    require(
        keyboard_focus(main, Some(popup)) == Ok(KeyboardFocus::MainWindow),
        "A hidden focus target disturbed keyboard ownership",
    )?;
    windows.destroy(hidden);
    require(
        browser::focus_chrome(hidden) == Err(BrowserError::InvalidWindow),
        "A destroyed focus target was not reported as invalid",
    )?;

    // Exercise the actual foreground query and focus transfer through both public paths.
    windows.focus(popup);
    require(
        keyboard_focus(main, Some(popup)) == Ok(KeyboardFocus::Popup),
        "The real popup did not receive keyboard focus",
    )?;
    browser::focus_chrome(main).map_err(|error| error.to_string())?;
    require(
        keyboard_focus(main, Some(popup)) == Ok(KeyboardFocus::MainWindow),
        "Chrome focus did not transfer keys out of the popup",
    )?;
    require(
        browser::activate_if_foreground(popup).map_err(|error| error.to_string())?,
        "A foreground popup refused activation",
    )?;
    require(
        keyboard_focus(main, Some(popup)) == Ok(KeyboardFocus::Popup),
        "Activation did not transfer keys into the popup",
    )?;
    let stacking = overlay_stacking(popup, main).map_err(|error| error.to_string())?;
    require(stacking.owner == main, "The popup has the wrong owner")?;
    require(
        stacking.visible && stacking.above_main && !stacking.topmost,
        "The visible popup must be above main without a topmost state",
    )?;
    refresh_interval(main).map_err(|error| error.to_string())?;

    windows.destroy(popup);
    require(!window_exists(popup), "A destroyed popup was reported live")?;
    Ok(())
}

fn foreign_focus(main: isize, foreign: isize) -> Result<(), String> {
    let windows = Windows::new()?;
    // Both focus paths must leave the WM-activated unmarked fixture's keys alone.
    require(
        !browser::syrinx_in_foreground(),
        "An unowned window counts as Syrinx",
    )?;
    browser::focus_chrome(main).map_err(|error| error.to_string())?;
    require(
        !browser::activate_if_foreground(main).map_err(|error| error.to_string())?,
        "Syrinx activated over an unrelated foreground window",
    )?;
    require(
        windows.has_keys(foreign) && keyboard_focus(main, None) == Ok(KeyboardFocus::Elsewhere),
        "A focus request took keys from an unrelated window",
    )?;
    Ok(())
}

fn require(condition: bool, reason: &str) -> Result<(), String> {
    if !condition {
        return Err(reason.to_owned());
    }
    Ok(())
}

//! Owns a disposable X11 parent for the Linux browser example fixtures.

use gtk::{gdk, prelude::*};
use x11_dl::xlib;

#[path = "window_input.rs"]
mod input;

/// A synthetic parent; drop it only after its BrowserHost has stopped.
pub struct FixtureWindow(isize);

impl FixtureWindow {
    pub fn new() -> Result<Self, String> {
        crate::gtk_thread::run(|| {
            with_display(|xlib, display| {
                let root = unsafe { (xlib.XDefaultRootWindow)(display) };
                let window =
                    unsafe { (xlib.XCreateSimpleWindow)(display, root, 20, 20, 900, 700, 0, 0, 0) };
                if window == 0 {
                    return Err("Cannot create fixture parent".into());
                }
                let pid = std::ffi::c_ulong::from(std::process::id());
                let mut attributes = xlib::XSetWindowAttributes {
                    override_redirect: xlib::True,
                    ..unsafe { std::mem::zeroed() }
                };
                unsafe {
                    (xlib.XChangeWindowAttributes)(
                        display,
                        window,
                        xlib::CWOverrideRedirect,
                        &mut attributes,
                    );
                    let atom = (xlib.XInternAtom)(display, c"_NET_WM_PID".as_ptr(), xlib::False);
                    (xlib.XChangeProperty)(
                        display,
                        window,
                        atom,
                        xlib::XA_CARDINAL,
                        32,
                        xlib::PropModeReplace,
                        (&raw const pid).cast(),
                        1,
                    );
                    (xlib.XStoreName)(display, window, c"Syrinx synthetic Linux fixture".as_ptr());
                    // Like the Windows fixture, keep the parent hidden and inspect child state.
                    (xlib.XSync)(display, xlib::False);
                }
                isize::try_from(window)
                    .map(Self)
                    .map_err(|_| "Unrepresentable X11 window ID".into())
            })
        })
        .map_err(|error| error.to_string())?
    }

    /// Explicitly map the unmanaged fixture parent for document-focus and media probes.
    pub fn show(&self) -> Result<(), String> {
        let window = self.0;
        crate::gtk_thread::run(move || {
            with_display(|xlib, display| {
                unsafe {
                    (xlib.XMapRaised)(display, window as _);
                    (xlib.XSetInputFocus)(
                        display,
                        window as _,
                        xlib::RevertToParent,
                        xlib::CurrentTime,
                    );
                    (xlib.XSync)(display, xlib::False);
                }
                Ok(())
            })
        })
        .map_err(|error| error.to_string())?
    }

    /// Focus only this owned chrome window, never another application.
    pub fn focus(&self) -> Result<(), String> {
        let window = self.0;
        crate::gtk_thread::run(move || {
            crate::browser::focus_chrome(window).map_err(|error| error.to_string())
        })
        .map_err(|error| error.to_string())?
    }

    /// Read physical child coordinates relative to this fixture parent.
    pub fn geometry(&self, child: isize) -> Result<(i32, i32, u32, u32), String> {
        self.require_child(child)?;
        crate::gtk_thread::run(move || {
            with_display(|xlib, display| {
                let mut attributes = unsafe { std::mem::zeroed::<xlib::XWindowAttributes>() };
                if unsafe { (xlib.XGetWindowAttributes)(display, child as _, &mut attributes) } == 0
                {
                    return Err("Cannot inspect fixture child geometry".into());
                }
                Ok((
                    attributes.x,
                    attributes.y,
                    attributes.width as u32,
                    attributes.height as u32,
                ))
            })
        })
        .map_err(|error| error.to_string())?
    }

    /// Inspect the child's own mapped state, including when its parent is hidden.
    pub fn visible(&self, child: isize) -> Result<bool, String> {
        self.require_child(child)?;
        crate::gtk_thread::run(move || {
            with_display(|xlib, display| {
                let mut attributes = unsafe { std::mem::zeroed::<xlib::XWindowAttributes>() };
                if unsafe { (xlib.XGetWindowAttributes)(display, child as _, &mut attributes) } == 0
                {
                    return Err("Cannot inspect fixture child visibility".into());
                }
                Ok(attributes.map_state != xlib::IsUnmapped)
            })
        })
        .map_err(|error| error.to_string())?
    }

    /// Dispatch a simple GDK key to the focused widget of this fixture's page child.
    pub fn key(&self, child: isize, keyval: u32) -> Result<(), String> {
        self.require_child(child)?;
        crate::gtk_thread::run(move || input::key(child, keyval))
            .map_err(|error| error.to_string())?
    }

    /// Inspect process-owned GTK print dialogs without dismissing an existing dialog.
    pub fn print_dialog_open(&self) -> Result<bool, String> {
        crate::gtk_thread::run(input::print_dialog_open).map_err(|error| error.to_string())?
    }

    /// Cancel the sole process-owned GTK print dialog; never submit a print job.
    pub fn cancel_print_dialog(&self) -> Result<bool, String> {
        crate::gtk_thread::run(input::cancel_print_dialog).map_err(|error| error.to_string())?
    }

    fn require_child(&self, child: isize) -> Result<(), String> {
        if !self.children()?.contains(&child) {
            return Err("Native probe target is outside its fixture parent".into());
        }
        Ok(())
    }

    pub fn handle(&self) -> isize {
        self.0
    }

    /// Read actual child IDs so closing a page proves native resources were released.
    pub fn children(&self) -> Result<Vec<isize>, String> {
        let window = self.0;
        crate::gtk_thread::run(move || {
            with_display(|xlib, display| {
                let (mut root, mut parent, mut count) = (0, 0, 0);
                let mut children = std::ptr::null_mut();
                let found = unsafe {
                    (xlib.XQueryTree)(
                        display,
                        window as _,
                        &mut root,
                        &mut parent,
                        &mut children,
                        &mut count,
                    )
                };
                let list = if children.is_null() {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(children, count as usize) }
                        .iter()
                        .map(|id| *id as isize)
                        .collect()
                };
                if !children.is_null() {
                    unsafe { (xlib.XFree)(children.cast()) };
                }
                if found == 0 {
                    return Err("Cannot read fixture children".into());
                }
                Ok(list)
            })
        })
        .map_err(|error| error.to_string())?
    }
}

impl Drop for FixtureWindow {
    fn drop(&mut self) {
        let window = self.0;
        let result = crate::gtk_thread::run(move || {
            with_display(|xlib, display| {
                unsafe {
                    (xlib.XDestroyWindow)(display, window as _);
                    (xlib.XSync)(display, xlib::False);
                }
                Ok(())
            })
        });
        if !matches!(result, Ok(Ok(()))) {
            eprintln!("Cannot destroy synthetic X11 parent: {result:?}");
        }
    }
}

/// Borrow GTK's existing connection; all callers run through gtk_thread::run.
fn with_display<T>(
    job: impl FnOnce(&xlib::Xlib, *mut xlib::Display) -> Result<T, String>,
) -> Result<T, String> {
    let xlib = xlib::Xlib::open().map_err(|error| error.to_string())?;
    let display = gdk::Display::default()
        .ok_or("No GDK display")?
        .downcast::<gdkx11::X11Display>()
        .map_err(|_| "Not an X11 display")?;
    let raw = unsafe { gdkx11::ffi::gdk_x11_display_get_xdisplay(display.as_ptr()) }.cast();
    display.error_trap_push();
    let result = job(&xlib, raw);
    let error = display.error_trap_pop();
    if error != 0 {
        return Err(format!(
            "Fixture X11 operation failed with server error {error}"
        ));
    }
    result
}

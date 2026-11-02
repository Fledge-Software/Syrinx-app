//! Reads native acceptance evidence on GTK's X11 connection with checked X errors.

use std::{ffi::c_ulong, ptr, time::Duration};

use gtk::{gdk, prelude::*};
use x11_dl::xlib;

use super::{KeyboardFocus, OverlayStacking, ProbeError};

/// The connection and error trap belong exclusively to the GTK thread.
struct Native {
    xlib: xlib::Xlib,
    display: gdkx11::X11Display,
    raw: *mut xlib::Display,
}

impl Native {
    fn new() -> Result<Self, ProbeError> {
        let xlib = xlib::Xlib::open().map_err(|_| ProbeError::DisplayUnreadable)?;
        let display = gdk::Display::default()
            .ok_or(ProbeError::DisplayUnreadable)?
            .downcast::<gdkx11::X11Display>()
            .map_err(|_| ProbeError::DisplayUnreadable)?;
        // SAFETY: GDK owns this X11 connection on the current GTK thread.
        let raw = unsafe { gdkx11::ffi::gdk_x11_display_get_xdisplay(display.as_ptr()) }.cast();
        Ok(Self { xlib, display, raw })
    }

    fn id(window: isize) -> Result<c_ulong, ProbeError> {
        c_ulong::try_from(window)
            .ok()
            .filter(|id| *id > 1)
            .ok_or(ProbeError::NotSyrinxWindow)
    }

    /// Synchronize the server before accepting a result; stale IDs must not abort the app.
    fn checked<T>(&self, operation: impl FnOnce() -> T) -> Result<T, ProbeError> {
        self.display.error_trap_push();
        let result = operation();
        match self.display.error_trap_pop() {
            0 => Ok(result),
            3 => Err(ProbeError::NotSyrinxWindow), // BadWindow: an ordinary stale-ID result.
            code => Err(ProbeError::X11Error(code)),
        }
    }

    fn attributes(&self, window: c_ulong) -> Result<xlib::XWindowAttributes, ProbeError> {
        self.checked(|| {
            let mut attributes = unsafe { std::mem::zeroed() };
            let found =
                unsafe { (self.xlib.XGetWindowAttributes)(self.raw, window, &mut attributes) };
            (found, attributes)
        })
        .and_then(|(found, attributes)| {
            if found == 0 {
                Err(ProbeError::NotSyrinxWindow)
            } else {
                Ok(attributes)
            }
        })
    }

    fn property(
        &self,
        window: c_ulong,
        name: &std::ffi::CStr,
        kind: c_ulong,
    ) -> Result<Vec<c_ulong>, ProbeError> {
        self.checked(|| {
            let (mut actual, mut format, mut count, mut remaining) = (0, 0, 0, 0);
            let mut data = ptr::null_mut();
            let atom = unsafe { (self.xlib.XInternAtom)(self.raw, name.as_ptr(), xlib::False) };
            let status = unsafe {
                (self.xlib.XGetWindowProperty)(
                    self.raw,
                    window,
                    atom,
                    0,
                    1024,
                    xlib::False,
                    kind,
                    &mut actual,
                    &mut format,
                    &mut count,
                    &mut remaining,
                    &mut data,
                )
            };
            let valid =
                status == 0 && actual == kind && format == 32 && remaining == 0 && !data.is_null();
            let result = if valid {
                // Format-32 properties are delivered as native longs by Xlib.
                unsafe { std::slice::from_raw_parts(data.cast::<c_ulong>(), count as usize) }
                    .to_vec()
            } else {
                Vec::new()
            };
            if !data.is_null() {
                unsafe { (self.xlib.XFree)(data.cast()) };
            }
            result
        })
    }

    fn owned(&self, window: c_ulong) -> Result<(), ProbeError> {
        self.attributes(window)?;
        if self.property(window, c"_NET_WM_PID", xlib::XA_CARDINAL)?
            != [c_ulong::from(std::process::id())]
        {
            return Err(ProbeError::NotSyrinxWindow);
        }
        Ok(())
    }

    fn tree(&self, window: c_ulong) -> Result<(c_ulong, Vec<c_ulong>), ProbeError> {
        self.checked(|| {
            let (mut root, mut parent, mut count) = (0, 0, 0);
            let mut children = ptr::null_mut();
            let status = unsafe {
                (self.xlib.XQueryTree)(
                    self.raw,
                    window,
                    &mut root,
                    &mut parent,
                    &mut children,
                    &mut count,
                )
            };
            let list = if children.is_null() {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(children, count as usize) }.to_vec()
            };
            if !children.is_null() {
                unsafe { (self.xlib.XFree)(children.cast()) };
            }
            (status, parent, list)
        })
        .and_then(|(status, parent, children)| {
            if status == 0 {
                Err(ProbeError::NotSyrinxWindow)
            } else {
                Ok((parent, children))
            }
        })
    }

    fn ancestor(&self, mut child: c_ulong, parent: c_ulong) -> Result<bool, ProbeError> {
        for _ in 0..64 {
            if child == parent {
                return Ok(true);
            }
            if child <= 1 {
                return Ok(false);
            }
            child = self.tree(child)?.0;
        }
        Err(ProbeError::FocusUnreadable)
    }

    fn root_child(&self, mut window: c_ulong) -> Result<c_ulong, ProbeError> {
        let root = unsafe { (self.xlib.XDefaultRootWindow)(self.raw) };
        for _ in 0..64 {
            let parent = self.tree(window)?.0;
            if parent == root {
                return Ok(window);
            }
            if parent == 0 {
                break;
            }
            window = parent;
        }
        Err(ProbeError::NotSyrinxWindow)
    }

    fn focus(&self) -> Result<c_ulong, ProbeError> {
        self.checked(|| {
            let (mut focus, mut revert) = (0, 0);
            unsafe { (self.xlib.XGetInputFocus)(self.raw, &mut focus, &mut revert) };
            focus
        })
    }
}

/// Run all toolkit work on syrinx-gtk and return only owned, thread-safe values.
fn run<T: Send + 'static>(
    job: impl FnOnce(&Native) -> Result<T, ProbeError> + Send + 'static,
) -> Result<T, ProbeError> {
    crate::gtk_thread::run(move || job(&Native::new()?))
        .map_err(|_| ProbeError::DisplayUnreadable)?
}

pub(super) fn exists(window: isize) -> bool {
    match run(move |native| native.attributes(Native::id(window)?).map(|_| true)) {
        Ok(found) => found,
        Err(ProbeError::NotSyrinxWindow) => false,
        Err(error) => {
            eprintln!("X11 window-existence probe failed: {error}");
            false
        }
    }
}

pub(super) fn focus(main: isize, popup: Option<isize>) -> Result<KeyboardFocus, ProbeError> {
    run(move |native| {
        let main = Native::id(main)?;
        native.owned(main)?;
        let popup = popup.map(Native::id).transpose()?;
        if let Some(popup) = popup {
            native.owned(popup)?;
        }
        let focus = native.focus()?;
        if focus <= 1 {
            return Ok(KeyboardFocus::Nobody);
        }
        if focus == main {
            return Ok(KeyboardFocus::MainWindow);
        }
        if Some(focus) == popup {
            return Ok(KeyboardFocus::Popup);
        }
        if native.ancestor(focus, main)? {
            return Ok(KeyboardFocus::InsideMainWindow);
        }
        Ok(KeyboardFocus::Elsewhere)
    })
}

pub(super) fn foreground() -> isize {
    run(|native| {
        let mut window = native.focus()?;
        for _ in 0..64 {
            if window <= 1 {
                return Ok(0);
            }
            match native.owned(window) {
                Ok(()) => return isize::try_from(window).map_err(|_| ProbeError::NotSyrinxWindow),
                Err(ProbeError::NotSyrinxWindow) => {}
                Err(error) => return Err(error),
            }
            window = native.tree(window)?.0;
        }
        Ok(0)
    })
    .unwrap_or_else(|error| {
        eprintln!("X11 foreground probe failed: {error}");
        0
    })
}

pub(super) fn stacking(popup: isize, main: isize) -> Result<OverlayStacking, ProbeError> {
    run(move |native| {
        let (popup, main) = (Native::id(popup)?, Native::id(main)?);
        native.owned(popup)?;
        native.owned(main)?;
        let mut owner = 0;
        native.checked(|| unsafe {
            (native.xlib.XGetTransientForHint)(native.raw, popup, &mut owner)
        })?;
        let states = native.property(popup, c"_NET_WM_STATE", xlib::XA_ATOM)?;
        let above = unsafe {
            (native.xlib.XInternAtom)(native.raw, c"_NET_WM_STATE_ABOVE".as_ptr(), xlib::False)
        };
        let root = unsafe { (native.xlib.XDefaultRootWindow)(native.raw) };
        let children = native.tree(root)?.1;
        let (popup_root, main_root) = (native.root_child(popup)?, native.root_child(main)?);
        let above_main = match (
            children.iter().position(|id| *id == popup_root),
            children.iter().position(|id| *id == main_root),
        ) {
            (Some(popup), Some(main)) => popup > main,
            _ => return Err(ProbeError::NotSyrinxWindow),
        };
        Ok(OverlayStacking {
            owner: isize::try_from(owner).map_err(|_| ProbeError::NotSyrinxWindow)?,
            topmost: states.contains(&above),
            visible: native.attributes(popup)?.map_state == xlib::IsViewable,
            above_main,
        })
    })
}

pub(super) fn refresh(window: isize) -> Result<Duration, ProbeError> {
    run(move |native| {
        let window = Native::id(window)?;
        native.owned(window)?;
        let wrapper = gdkx11::X11Window::foreign_new_for_display(&native.display, window);
        let monitor = native
            .display
            .upcast_ref::<gdk::Display>()
            .monitor_at_window(wrapper.upcast_ref())
            .ok_or(ProbeError::DisplayUnreadable)?;
        let millihertz =
            u32::try_from(monitor.refresh_rate()).map_err(|_| ProbeError::DisplayUnreadable)?;
        if !(24_000..=1_000_000).contains(&millihertz) {
            return Err(ProbeError::ImplausibleRefresh(millihertz / 1000));
        }
        Ok(Duration::from_secs_f64(1000. / f64::from(millihertz)))
    })
}

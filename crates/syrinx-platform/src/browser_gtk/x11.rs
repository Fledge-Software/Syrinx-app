//! Places, shows and focuses Syrinx's own X11 windows through one private Xlib connection.

use std::{
    ffi::{CStr, c_int, c_uint, c_ulong},
    ptr,
    sync::{Mutex, MutexGuard, OnceLock},
};

use x11_dl::{xlib, xrandr};

use super::{BrowserError, OverlayMonitor, OverlayPointer, OverlayRect};

/// One Xlib connection for the chrome thread's window queries, separate from GTK's and GPUI's.
struct Connection {
    xlib: xlib::Xlib,
    display: *mut xlib::Display,
    pid_atom: c_ulong,
}

// SAFETY: the display is only touched while the mutex below is held.
unsafe impl Send for Connection {}

static CONNECTION: OnceLock<Option<Mutex<Connection>>> = OnceLock::new();

fn connection() -> Option<MutexGuard<'static, Connection>> {
    CONNECTION
        .get_or_init(|| open().map(Mutex::new))
        .as_ref()?
        .lock()
        .ok()
}

fn open() -> Option<Connection> {
    let xlib = xlib::Xlib::open().ok()?;
    // Xlib is shared with GTK's thread, so it must be told it is used from several threads.
    unsafe { (xlib.XInitThreads)() };
    let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
    if display.is_null() {
        return None;
    }
    let pid_atom = unsafe { (xlib.XInternAtom)(display, c"_NET_WM_PID".as_ptr(), xlib::False) };
    Some(Connection {
        xlib,
        display,
        pid_atom,
    })
}

impl Connection {
    fn window(id: isize) -> Result<c_ulong, BrowserError> {
        c_ulong::try_from(id)
            .ok()
            .filter(|window| *window != 0)
            .ok_or(BrowserError::InvalidWindow)
    }

    fn exists(&self, window: c_ulong) -> bool {
        let mut attributes = unsafe { std::mem::zeroed::<xlib::XWindowAttributes>() };
        unsafe { (self.xlib.XGetWindowAttributes)(self.display, window, &mut attributes) != 0 }
    }

    fn flush(&self) {
        unsafe { (self.xlib.XFlush)(self.display) };
    }

    fn root(&self) -> c_ulong {
        unsafe { (self.xlib.XDefaultRootWindow)(self.display) }
    }

    fn focused(&self) -> c_ulong {
        let mut window = 0;
        let mut revert = 0;
        unsafe { (self.xlib.XGetInputFocus)(self.display, &mut window, &mut revert) };
        window
    }

    /// The top-level window holding `window`: the root's child on its ancestor chain.
    fn top_level(&self, mut window: c_ulong) -> Option<c_ulong> {
        let root = self.root();
        for _ in 0..64 {
            let mut found_root = 0;
            let mut parent = 0;
            let mut children = ptr::null_mut();
            let mut count: c_uint = 0;
            let status = unsafe {
                (self.xlib.XQueryTree)(
                    self.display,
                    window,
                    &mut found_root,
                    &mut parent,
                    &mut children,
                    &mut count,
                )
            };
            if !children.is_null() {
                unsafe { (self.xlib.XFree)(children.cast()) };
            }
            if status == 0 {
                return None;
            }
            if parent == root || parent == 0 {
                return Some(window);
            }
            window = parent;
        }
        None
    }

    /// Whether `window` belongs to this process, by the `_NET_WM_PID` its toolkit set.
    fn ours(&self, window: c_ulong) -> bool {
        let mut kind = 0;
        let mut format = 0;
        let mut items = 0;
        let mut remaining = 0;
        let mut data = ptr::null_mut();
        let status = unsafe {
            (self.xlib.XGetWindowProperty)(
                self.display,
                window,
                self.pid_atom,
                0,
                1,
                xlib::False,
                xlib::XA_CARDINAL,
                &mut kind,
                &mut format,
                &mut items,
                &mut remaining,
                &mut data,
            )
        };
        if status != 0 || data.is_null() {
            return false;
        }
        // Format 32 properties arrive as C longs.
        let pid = (items == 1 && format == 32).then(|| unsafe { *data.cast::<c_ulong>() });
        unsafe { (self.xlib.XFree)(data.cast()) };
        pid == Some(c_ulong::from(std::process::id()))
    }

    fn syrinx_focused(&self) -> bool {
        let focus = self.focused();
        if focus <= 1 {
            return false;
        }
        self.top_level(focus)
            .is_some_and(|top| self.ours(top) || self.ours(focus))
    }

    /// The window's outer rectangle in root coordinates.
    fn root_rect(&self, window: c_ulong) -> Option<(i32, i32, u32, u32)> {
        let (mut root, mut x, mut y, mut width, mut height, mut border, mut depth) =
            (0, 0, 0, 0, 0, 0, 0);
        let geometry = unsafe {
            (self.xlib.XGetGeometry)(
                self.display,
                window,
                &mut root,
                &mut x,
                &mut y,
                &mut width,
                &mut height,
                &mut border,
                &mut depth,
            )
        };
        if geometry == 0 {
            return None;
        }
        let (mut left, mut top, mut child) = (0, 0, 0);
        let translated = unsafe {
            (self.xlib.XTranslateCoordinates)(
                self.display,
                window,
                root,
                0,
                0,
                &mut left,
                &mut top,
                &mut child,
            )
        };
        (translated != 0).then_some((left, top, width, height))
    }
}

pub(super) fn validate_window(window: isize) -> Result<(), BrowserError> {
    let window = Connection::window(window)?;
    let connection = connection().ok_or(BrowserError::InvalidWindow)?;
    if !connection.exists(window) {
        return Err(BrowserError::InvalidWindow);
    }
    Ok(())
}

/// Gives the Syrinx chrome window keyboard focus, taking it from a page's child window.
pub fn focus_chrome(parent: isize) -> Result<(), BrowserError> {
    let window = Connection::window(parent)?;
    let connection = connection().ok_or(BrowserError::Focus)?;
    unsafe {
        (connection.xlib.XSetInputFocus)(
            connection.display,
            window,
            xlib::RevertToParent,
            xlib::CurrentTime,
        )
    };
    connection.flush();
    Ok(())
}

/// Whether X11 keyboard focus is on the Syrinx chrome window itself, not a page child.
pub fn chrome_has_keyboard_focus(parent: isize) -> bool {
    holds_keyboard_focus(parent)
}

/// Whether X11 keyboard focus is on `window` itself, such as the palette's popup.
pub fn holds_keyboard_focus(window: isize) -> bool {
    let Ok(window) = Connection::window(window) else {
        return false;
    };
    connection().is_some_and(|connection| connection.focused() == window)
}

/// Popups are override-redirect windows, so no owner is needed to keep them above Syrinx.
pub fn set_overlay_owner(popup: isize, parent: isize) -> Result<(), BrowserError> {
    validate_window(popup)?;
    validate_window(parent)
}

/// Whether one of this process's windows holds keyboard focus.
pub fn syrinx_in_foreground() -> bool {
    connection().is_some_and(|connection| connection.syrinx_focused())
}

/// Focuses `window` only while Syrinx is already in front, so it never steals the screen.
pub fn activate_if_foreground(window: isize) -> Result<bool, BrowserError> {
    let target = Connection::window(window)?;
    let connection = connection().ok_or(BrowserError::Focus)?;
    if !connection.syrinx_focused() {
        return Ok(false);
    }
    unsafe {
        (connection.xlib.XRaiseWindow)(connection.display, target);
        (connection.xlib.XSetInputFocus)(
            connection.display,
            target,
            xlib::RevertToParent,
            xlib::CurrentTime,
        );
    }
    connection.flush();
    Ok(true)
}

/// Moves a kept popup to `rect` and shows it above Syrinx without asking the window manager.
pub fn show_overlay(popup: isize, rect: OverlayRect) -> Result<(), BrowserError> {
    let window = Connection::window(popup)?;
    let width = u32::try_from(rect.width).map_err(|_| BrowserError::OverlayPlacement)?;
    let height = u32::try_from(rect.height).map_err(|_| BrowserError::OverlayPlacement)?;
    let connection = connection().ok_or(BrowserError::OverlayPlacement)?;
    unsafe {
        (connection.xlib.XMoveResizeWindow)(
            connection.display,
            window,
            rect.left,
            rect.top,
            width.max(1),
            height.max(1),
        );
        (connection.xlib.XMapRaised)(connection.display, window);
    }
    connection.flush();
    Ok(())
}

/// Hides a kept popup; `owner` keeps focus where it was.
pub fn hide_overlay(popup: isize, _owner: isize) -> Result<(), BrowserError> {
    let window = Connection::window(popup)?;
    let connection = connection().ok_or(BrowserError::OverlayPlacement)?;
    unsafe { (connection.xlib.XUnmapWindow)(connection.display, window) };
    connection.flush();
    Ok(())
}

/// The pointer in root coordinates, so leaving is seen over the page child and outside Syrinx.
pub fn overlay_pointer(parent: isize, popup: Option<isize>) -> Result<OverlayPointer, String> {
    Connection::window(parent).map_err(|error| format!("Sidebar owner: {error}"))?;
    let connection = connection().ok_or("Syrinx cannot reach the X11 display.")?;
    let root = connection.root();
    let (mut found_root, mut child, mut x, mut y, mut window_x, mut window_y, mut mask) =
        (0, 0, 0, 0, 0, 0, 0);
    let read = unsafe {
        (connection.xlib.XQueryPointer)(
            connection.display,
            root,
            &mut found_root,
            &mut child,
            &mut x,
            &mut y,
            &mut window_x,
            &mut window_y,
            &mut mask,
        )
    };
    if read == 0 {
        return Err("The pointer is on another screen.".to_owned());
    }
    let in_popup = popup
        .and_then(|popup| Connection::window(popup).ok())
        .and_then(|popup| connection.root_rect(popup))
        .is_some_and(|(left, top, width, height)| {
            x >= left && y >= top && x < left + width as i32 && y < top + height as i32
        });
    Ok(OverlayPointer {
        x,
        y,
        in_popup,
        active: connection.syrinx_focused(),
        pressed: mask & xlib::Button1Mask != 0,
    })
}

/// The monitor under a physical point, with GPUI's X11 scale (`Xft.dpi` over 96).
pub fn overlay_monitor(x: i32, y: i32) -> Result<OverlayMonitor, String> {
    let connection = connection().ok_or("Syrinx cannot reach the X11 display.")?;
    let scale = xft_scale(&connection);
    let monitors = monitors(&connection);
    let index = monitors
        .iter()
        .position(|monitor| {
            x >= monitor.left && x < monitor.right && y >= monitor.top && y < monitor.bottom
        })
        .unwrap_or(0);
    let chosen = monitors
        .into_iter()
        .nth(index)
        .ok_or("X11 reported no monitors.")?;
    Ok(OverlayMonitor { scale, ..chosen })
}

fn monitors(connection: &Connection) -> Vec<OverlayMonitor> {
    let Ok(randr) = xrandr::Xrandr::open() else {
        return Vec::new();
    };
    let mut count: c_int = 0;
    let list = unsafe {
        (randr.XRRGetMonitors)(
            connection.display,
            connection.root(),
            xlib::True,
            &mut count,
        )
    };
    if list.is_null() {
        return Vec::new();
    }
    let entries = unsafe { std::slice::from_raw_parts(list, usize::try_from(count).unwrap_or(0)) };
    let found = entries
        .iter()
        .enumerate()
        .map(|(index, monitor)| OverlayMonitor {
            id: index as u64,
            scale: 1.,
            left: monitor.x,
            top: monitor.y,
            right: monitor.x + monitor.width,
            bottom: monitor.y + monitor.height,
        })
        .collect();
    unsafe { (randr.XRRFreeMonitors)(list) };
    found
}

/// GPUI's X11 backend scales by `Xft.dpi / 96`, defaulting to 1 when the resource is unset.
fn xft_scale(connection: &Connection) -> f32 {
    let resources = unsafe { (connection.xlib.XResourceManagerString)(connection.display) };
    if resources.is_null() {
        return 1.;
    }
    let text = unsafe { CStr::from_ptr(resources) }.to_string_lossy();
    text.lines()
        .find_map(|line| line.strip_prefix("Xft.dpi:"))
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|dpi| *dpi > 0.)
        .map_or(1., |dpi| dpi / 96.)
}

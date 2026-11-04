//! Places, shows and focuses Syrinx's own X11 windows through one private Xlib connection.

use std::{
    ffi::{CStr, c_int, c_uint, c_ulong},
    ptr,
    sync::{Mutex, MutexGuard, OnceLock},
};

use x11_dl::{xlib, xrandr};

use crate::x11_errors::ErrorTrap;

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

/// The shared connection: `NoDisplay` when no X11 display could be opened, and `DisplayLock`
/// when a panic on another thread poisoned it.
fn connection() -> Result<MutexGuard<'static, Connection>, BrowserError> {
    CONNECTION
        .get_or_init(|| open().map(Mutex::new))
        .as_ref()
        .ok_or(BrowserError::NoDisplay)?
        .lock()
        .map_err(|_| BrowserError::DisplayLock)
}

/// The same connection for callers that report in words.
fn described_connection() -> Result<MutexGuard<'static, Connection>, String> {
    connection().map_err(|error| {
        match error {
            BrowserError::DisplayLock => "Syrinx's X11 connection failed after an earlier error.",
            _ => "Syrinx cannot reach the X11 display.",
        }
        .to_owned()
    })
}

fn open() -> Option<Connection> {
    // GTK's thread installs `ErrorTrap` as it starts; without it this connection's errors
    // would end the process. No X11 display means GTK cannot start either.
    crate::gtk_thread::ensure_started().ok()?;
    let xlib = xlib::Xlib::open().ok()?;
    // Xlib is shared with GTK's thread, so it must be told it is used from several threads.
    unsafe { (xlib.XInitThreads)() };
    let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
    if display.is_null() {
        return None;
    }
    let pid_atom = unsafe { (xlib.XInternAtom)(display, c"_NET_WM_PID".as_ptr(), xlib::False) };
    ErrorTrap::register_private_display(display);
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

    /// Runs `call`'s requests with X errors on this connection recorded by `ErrorTrap`
    /// rather than fatal; `None` when any of them failed, for example on a window destroyed
    /// meanwhile. Holding `&self` means the caller holds the connection's lock, which
    /// `ErrorTrap::run` requires.
    fn trapped<T>(&self, call: impl FnOnce() -> T) -> Option<T> {
        ErrorTrap::run(&self.xlib, call)
    }

    fn exists(&self, window: c_ulong) -> bool {
        self.trapped(|| self.attributes(window).is_some())
            .unwrap_or(false)
    }

    /// The window's attributes; untrapped, so callers run it inside `trapped`.
    fn attributes(&self, window: c_ulong) -> Option<xlib::XWindowAttributes> {
        let mut attributes = unsafe { std::mem::zeroed::<xlib::XWindowAttributes>() };
        let read =
            unsafe { (self.xlib.XGetWindowAttributes)(self.display, window, &mut attributes) };
        (read != 0).then_some(attributes)
    }

    /// Whether `window` and every ancestor are mapped; focusing any other window raises
    /// BadMatch. Untrapped, so callers run it inside `trapped`.
    fn viewable(&self, window: c_ulong) -> bool {
        self.attributes(window)
            .is_some_and(|attributes| attributes.map_state == xlib::IsViewable)
    }

    /// Gives `window` the keys only when it can take them; `false` when it was not viewable
    /// or the server refused the request.
    fn set_focus(&self, window: c_ulong) -> bool {
        self.trapped(|| {
            if !self.viewable(window) {
                return false;
            }
            unsafe {
                (self.xlib.XSetInputFocus)(
                    self.display,
                    window,
                    xlib::RevertToParent,
                    xlib::CurrentTime,
                )
            };
            true
        })
        .unwrap_or(false)
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

    /// `window`'s parent, or `None` when the server reports none. Untrapped: a destroyed
    /// window raises BadWindow, so callers run it inside `trapped`.
    fn parent(&self, window: c_ulong) -> Option<c_ulong> {
        let (mut found_root, mut parent) = (0, 0);
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
        (status != 0 && parent != 0).then_some(parent)
    }

    /// Whether `window`, or an ancestor below the root, carries this process's `_NET_WM_PID`.
    /// A reparenting window manager puts its own frame between Syrinx's window and the root,
    /// and the page container carries no PID, so the chain is searched up to the first match.
    /// A window destroyed during the walk counts as "not ours".
    fn inside_ours(&self, window: c_ulong) -> bool {
        let root = self.root();
        self.trapped(|| {
            std::iter::successors(Some(window), |current| self.parent(*current))
                .take_while(|current| *current != root)
                .take(64)
                .any(|current| self.ours(current))
        })
        .unwrap_or(false)
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

    /// Whether the focused window is, or lies inside, a window this process marked as its own.
    fn syrinx_focused(&self) -> bool {
        let focus = self.focused();
        // None (0) and PointerRoot (1) are not windows.
        if focus <= 1 {
            return false;
        }
        self.inside_ours(focus)
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
    let connection = connection()?;
    if !connection.exists(window) {
        return Err(BrowserError::InvalidWindow);
    }
    Ok(())
}

/// Gives the Syrinx chrome window keyboard focus, taking it from a page's child window.
/// Nothing moves while another application holds the keys (owner, 2026-10-12), or while the
/// window is minimised or otherwise not shown (never mapped, or inside an unmapped ancestor)
/// and cannot take them. A window that is gone is `InvalidWindow`, and a shown one that
/// refused the keys is `Focus`.
pub fn focus_chrome(parent: isize) -> Result<(), BrowserError> {
    let window = Connection::window(parent)?;
    let connection = connection()?;
    if !connection.syrinx_focused() || connection.set_focus(window) {
        return Ok(());
    }
    let attributes = connection
        .trapped(|| connection.attributes(window))
        .flatten()
        .ok_or(BrowserError::InvalidWindow)?;
    if attributes.map_state != xlib::IsViewable {
        return Ok(());
    }
    // Covered by code reading only: X refuses `XSetInputFocus` on a viewable window only in
    // a race (it is unmapped or destroyed in between), which no test can produce reliably.
    Err(BrowserError::Focus)
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
    connection().is_ok_and(|connection| connection.focused() == window)
}

/// Popups are override-redirect windows, so no owner is needed to keep them above Syrinx.
pub fn set_overlay_owner(popup: isize, parent: isize) -> Result<(), BrowserError> {
    validate_window(popup)?;
    validate_window(parent)
}

/// Whether one of this process's windows holds keyboard focus.
pub fn syrinx_in_foreground() -> bool {
    connection().is_ok_and(|connection| connection.syrinx_focused())
}

/// Focuses `window` only while Syrinx is already in front, so it never steals the screen.
/// `false` when Syrinx is behind another application or `window` cannot take the keys.
pub fn activate_if_foreground(window: isize) -> Result<bool, BrowserError> {
    let target = Connection::window(window)?;
    let connection = connection()?;
    if !connection.syrinx_focused() {
        return Ok(false);
    }
    // A window that is gone cannot be raised; it is reported like any other bad window.
    connection
        .trapped(|| unsafe { (connection.xlib.XRaiseWindow)(connection.display, target) })
        .ok_or(BrowserError::InvalidWindow)?;
    Ok(connection.set_focus(target))
}

/// Moves a kept popup to `rect` and shows it above Syrinx without asking the window manager.
pub fn show_overlay(popup: isize, rect: OverlayRect) -> Result<(), BrowserError> {
    let window = Connection::window(popup)?;
    let width = u32::try_from(rect.width).map_err(|_| BrowserError::OverlayPlacement)?;
    let height = u32::try_from(rect.height).map_err(|_| BrowserError::OverlayPlacement)?;
    let connection = connection()?;
    connection
        .trapped(|| unsafe {
            (connection.xlib.XMoveResizeWindow)(
                connection.display,
                window,
                rect.left,
                rect.top,
                width.max(1),
                height.max(1),
            );
            (connection.xlib.XMapRaised)(connection.display, window);
        })
        .ok_or(BrowserError::OverlayPlacement)
}

/// Hides a kept popup. A popup holding the keys hands them to `owner` first, as on Windows:
/// unmapping a focused override-redirect window reverts X focus to the root, outside Syrinx.
/// An owner that is minimised, gone or refuses cannot take them, so focus then reverts as X
/// decides. That is harmless here: the popup is hidden either way, which is all the caller
/// asked for.
pub fn hide_overlay(popup: isize, owner: isize) -> Result<(), BrowserError> {
    let window = Connection::window(popup)?;
    let owner = Connection::window(owner)?;
    let connection = connection()?;
    // The popup holding the keys proves Syrinx is in front, so this takes nothing from others.
    if connection.focused() == window {
        connection.set_focus(owner);
    }
    connection
        .trapped(|| unsafe { (connection.xlib.XUnmapWindow)(connection.display, window) })
        .ok_or(BrowserError::OverlayPlacement)?;
    Ok(())
}

/// The window's top-left in physical root coordinates, read from the X server. GPUI's own
/// X11 bounds keep a stale origin after a window manager moves and resizes in one step.
pub fn window_origin(window: isize) -> Option<(i32, i32)> {
    let window = Connection::window(window).ok()?;
    let connection = connection().ok()?;
    connection
        .root_rect(window)
        .map(|(left, top, _, _)| (left, top))
}

/// The pointer in root coordinates, so leaving is seen over the page child and outside Syrinx.
pub fn overlay_pointer(parent: isize, popup: Option<isize>) -> Result<OverlayPointer, String> {
    Connection::window(parent).map_err(|error| format!("Sidebar owner: {error}"))?;
    let connection = described_connection()?;
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
    let connection = described_connection()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain window on the test's display, mapped or not, that no Syrinx window owns.
    fn test_window(mapped: bool) -> c_ulong {
        let connection = connection().expect("an X11 display for the test");
        let root = connection.root();
        let window = unsafe {
            (connection.xlib.XCreateSimpleWindow)(connection.display, root, 0, 0, 16, 16, 0, 0, 0)
        };
        if mapped {
            unsafe { (connection.xlib.XMapWindow)(connection.display, window) };
        }
        unsafe { (connection.xlib.XSync)(connection.display, xlib::False) };
        window
    }

    fn destroy(window: c_ulong) {
        let connection = connection().expect("an X11 display for the test");
        unsafe {
            (connection.xlib.XDestroyWindow)(connection.display, window);
            (connection.xlib.XSync)(connection.display, xlib::False);
        }
    }

    #[test]
    fn a_minimised_window_is_never_given_the_keys() {
        // An unmapped window stands in for a minimised one: neither is viewable.
        let window = test_window(false);
        {
            let connection = connection().expect("an X11 display for the test");
            assert_eq!(
                connection.trapped(|| connection.viewable(window)),
                Some(false)
            );
            assert!(
                !connection.set_focus(window),
                "BadMatch would end the process"
            );
            unsafe { (connection.xlib.XSync)(connection.display, xlib::False) };
        }
        destroy(window);
    }

    /// The display's keyboard focus is shared by every test in the process.
    static FOCUS: Mutex<()> = Mutex::new(());

    /// Holds the focus lock and, when dropped, gives the keys back to the pointer root, so a
    /// failing focus test neither poisons the lock nor leaves focus where the next test looks.
    struct FocusGuard {
        _lock: MutexGuard<'static, ()>,
    }

    impl FocusGuard {
        fn take() -> Self {
            Self {
                _lock: FOCUS
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            }
        }
    }

    impl Drop for FocusGuard {
        fn drop(&mut self) {
            let Ok(connection) = connection() else {
                return;
            };
            unsafe {
                (connection.xlib.XSetInputFocus)(
                    connection.display,
                    xlib::PointerRoot as c_ulong,
                    xlib::RevertToPointerRoot,
                    xlib::CurrentTime,
                );
                (connection.xlib.XSync)(connection.display, xlib::False);
            }
        }
    }

    /// A mapped window inside `parent`, marked as this process's when `ours` is true.
    fn child_window(parent: c_ulong, ours: bool) -> c_ulong {
        let connection = connection().expect("an X11 display for the test");
        let window = unsafe {
            (connection.xlib.XCreateSimpleWindow)(connection.display, parent, 0, 0, 16, 16, 0, 0, 0)
        };
        if ours {
            let pid = c_ulong::from(std::process::id());
            unsafe {
                (connection.xlib.XChangeProperty)(
                    connection.display,
                    window,
                    connection.pid_atom,
                    xlib::XA_CARDINAL,
                    32,
                    xlib::PropModeReplace,
                    (&raw const pid).cast(),
                    1,
                )
            };
        }
        unsafe {
            (connection.xlib.XMapWindow)(connection.display, window);
            (connection.xlib.XSync)(connection.display, xlib::False);
        }
        window
    }

    #[test]
    fn a_page_inside_a_window_managers_frame_counts_as_syrinx_in_front() {
        let _focus = FocusGuard::take();
        // A reparenting manager's frame (no PID) holds Syrinx's window, which holds the page's
        // container (no PID either), and the container has the keys.
        let frame = test_window(true);
        let syrinx = child_window(frame, true);
        let container = child_window(syrinx, false);
        assert!(connection().expect("an X11 display").set_focus(container));
        assert!(syrinx_in_foreground());
        destroy(frame);
    }

    #[test]
    fn a_window_destroyed_mid_query_is_not_ours_and_not_fatal() {
        let _focus = FocusGuard::take();
        // Destroying the frame destroys the whole chain, so every request below names windows
        // that no longer exist: each raises BadWindow, which untrapped would end the process.
        let frame = test_window(true);
        let syrinx = child_window(frame, true);
        let container = child_window(syrinx, false);
        destroy(frame);
        let connection = connection().expect("an X11 display");
        assert!(
            !connection.inside_ours(container),
            "a gone window is not Syrinx's"
        );
        assert_eq!(connection.trapped(|| connection.viewable(syrinx)), None);
        assert!(!connection.set_focus(container));
        assert!(!connection.exists(frame));
        // The trap reports the error and is then clear for the next call.
        assert_eq!(connection.trapped(|| connection.parent(container)), None);
        assert_eq!(connection.trapped(|| 7), Some(7));
    }

    #[test]
    fn focus_chrome_ignores_a_minimised_window_and_reports_a_gone_one() {
        let _focus = FocusGuard::take();
        // Syrinx is in front: one of its windows has the keys.
        let frame = test_window(true);
        let syrinx = child_window(frame, true);
        assert!(connection().expect("an X11 display").set_focus(syrinx));
        let minimised = test_window(false);
        assert_eq!(focus_chrome(minimised as isize), Ok(()));
        destroy(minimised);
        assert_eq!(
            focus_chrome(minimised as isize),
            Err(BrowserError::InvalidWindow)
        );
        destroy(frame);
    }

    #[test]
    fn focus_moves_nowhere_while_another_application_holds_the_keys() {
        let _focus = FocusGuard::take();
        // In a test process no window carries Syrinx's `_NET_WM_PID`, so Syrinx is behind.
        let window = test_window(true);
        let before = connection().expect("an X11 display").focused();
        assert!(!syrinx_in_foreground());
        assert_eq!(focus_chrome(window as isize), Ok(()));
        assert_eq!(activate_if_foreground(window as isize), Ok(false));
        assert_eq!(connection().expect("an X11 display").focused(), before);
        destroy(window);
    }
}

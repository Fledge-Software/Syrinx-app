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

    /// Applies the caller's foreground observation before requesting chrome focus.
    fn focus_chrome(&self, window: c_ulong, in_front: bool) -> Result<(), BrowserError> {
        if !in_front || self.set_focus(window) {
            return Ok(());
        }
        let attributes = self
            .trapped(|| self.attributes(window))
            .flatten()
            .ok_or(BrowserError::InvalidWindow)?;
        if attributes.map_state != xlib::IsViewable {
            return Ok(());
        }
        // A viewable window can refuse only if it is unmapped/destroyed during the request.
        Err(BrowserError::Focus)
    }

    /// Raises and focuses the target only while the caller observed Syrinx in front.
    fn activate(&self, target: c_ulong, in_front: bool) -> Result<bool, BrowserError> {
        if !in_front {
            return Ok(false);
        }
        self.trapped(|| unsafe { (self.xlib.XRaiseWindow)(self.display, target) })
            .ok_or(BrowserError::InvalidWindow)?;
        Ok(self.set_focus(target))
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
    connection.focus_chrome(window, connection.syrinx_focused())
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

/// Records the real transient parent; override-redirect popups still place themselves.
pub fn set_overlay_owner(popup: isize, parent: isize) -> Result<(), BrowserError> {
    validate_window(popup)?;
    validate_window(parent)?;
    let popup = Connection::window(popup)?;
    let parent = Connection::window(parent)?;
    let connection = connection()?;
    connection
        .trapped(|| unsafe {
            (connection.xlib.XSetTransientForHint)(connection.display, popup, parent)
        })
        .ok_or(BrowserError::InvalidWindow)?;
    Ok(())
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
    connection.activate(target, connection.syrinx_focused())
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

    /// An unmapped root child: unit tests never ask the window manager to show it.
    fn test_window() -> c_ulong {
        let connection = connection().expect("an X11 display for the test");
        let root = connection.root();
        let window = unsafe {
            (connection.xlib.XCreateSimpleWindow)(connection.display, root, 0, 0, 16, 16, 0, 0, 0)
        };
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

    /// Serializes intentional X errors with GTK's process-wide GDK error traps.
    fn on_display<T: Send + 'static>(probe: impl FnOnce() -> T + Send + 'static) -> T {
        crate::gtk_thread::run(probe).expect("GTK thread")
    }

    #[test]
    fn a_minimised_window_is_never_given_the_keys() {
        let observed = on_display(|| {
            let ancestor = test_window();
            let child = child_window(ancestor, false);
            let observed = {
                let connection = connection().expect("an X11 display");
                // A mapped child remains unviewable beneath an unmapped ancestor.
                [ancestor, child].map(|window| {
                    (
                        connection.trapped(|| connection.viewable(window)),
                        connection.set_focus(window),
                    )
                })
            };
            destroy(ancestor);
            observed
        });
        assert_eq!(
            observed,
            [(Some(false), false); 2],
            "neither hidden target may take keys or raise an unhandled BadMatch"
        );
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
    fn a_page_inside_a_window_managers_frame_is_owned_through_its_ancestor() {
        let owned = on_display(|| {
            // Keep the frame unmapped: ownership does not require the WM to grant focus.
            let frame = test_window();
            let syrinx = child_window(frame, true);
            let container = child_window(syrinx, false);
            let sibling = child_window(frame, false);
            let owned = {
                let connection = connection().expect("an X11 display");
                [container, syrinx, frame, sibling].map(|window| connection.inside_ours(window))
            };
            destroy(frame);
            owned
        });
        assert_eq!(owned, [true, true, false, false]);
    }

    #[test]
    fn a_window_destroyed_mid_query_is_not_ours_and_not_fatal() {
        let observed = on_display(|| {
            // Destroy the whole chain: every following query names a stale ID.
            let frame = test_window();
            let syrinx = child_window(frame, true);
            let container = child_window(syrinx, false);
            destroy(frame);
            let connection = connection().expect("an X11 display");
            (
                connection.inside_ours(container),
                connection.trapped(|| connection.viewable(syrinx)),
                connection.set_focus(container),
                connection.exists(frame),
                connection.trapped(|| connection.parent(container)),
                connection.trapped(|| 7),
            )
        });
        // Each BadWindow is contained, and the last trap is clear for a successful call.
        assert_eq!(observed, (false, None, false, false, None, Some(7)));
    }

    #[test]
    fn focus_chrome_ignores_a_minimised_window_and_reports_a_gone_one() {
        let observed = on_display(|| {
            let minimised = test_window();
            // Supply foreground ownership; the target's real X11 state stays unmapped.
            let hidden = connection()
                .expect("an X11 display")
                .focus_chrome(minimised, true);
            destroy(minimised);
            let connection = connection().expect("an X11 display");
            (
                hidden,
                connection.focus_chrome(minimised, true),
                connection.activate(minimised, true),
            )
        });
        assert_eq!(
            observed,
            (
                Ok(()),
                Err(BrowserError::InvalidWindow),
                Err(BrowserError::InvalidWindow)
            )
        );
    }

    #[test]
    fn focus_moves_nowhere_while_another_application_holds_the_keys() {
        let observed = on_display(|| {
            let stale = test_window();
            destroy(stale);
            let connection = connection().expect("an X11 display");
            // A foreign foreground must decline before inspecting even a stale target.
            (
                connection.focus_chrome(stale, false),
                connection.activate(stale, false),
            )
        });
        assert_eq!(observed, (Ok(()), Ok(false)));
    }
}

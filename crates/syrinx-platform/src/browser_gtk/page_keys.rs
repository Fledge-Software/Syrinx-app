//! Lets a page's X11 windows take key presses only while the page holds X11 focus.
//!
//! X11 gives a key to the window under the pointer when that window lies inside the focused
//! one. The page sits inside Syrinx's window, so with the chrome focused and the pointer over
//! the page, the page took the owner's typing for the find bar or the address field. With no
//! page window selecting keys, the server passes them up to Syrinx's window as real events
//! (a forwarded copy is not enough: input methods such as fcitx drop synthetic keys).

use std::{
    ffi::{c_uint, c_ulong},
    ptr,
    rc::Rc,
};

use gtk::{gdk, prelude::*};
use x11_dl::xlib;

/// How far up the window tree a focus owner is looked for before giving up.
const MAX_DEPTH: usize = 32;

/// Watches the container's focus changes and keeps its key selection in step with them.
pub(super) struct KeyRouting {
    state: Rc<RouteState>,
}

/// Gives the page X11 focus when the owner clicks it, as a click focuses WebView2 on Windows.
#[derive(Clone)]
pub(super) struct ClickFocus(Rc<RouteState>);

impl ClickFocus {
    pub(super) fn focus(&self) {
        self.0.focus();
    }
}

struct RouteState {
    xlib: Rc<xlib::Xlib>,
    display: *mut xlib::Display,
    container: c_ulong,
    window: gdk::Window,
}

impl KeyRouting {
    /// `window` is the GDK window adopting `container`, on GTK's own X connection `display`.
    pub(super) fn install(
        xlib: Rc<xlib::Xlib>,
        display: *mut xlib::Display,
        container: c_ulong,
        window: gdk::Window,
    ) -> Self {
        let state = Rc::new(RouteState {
            xlib,
            display,
            container,
            window,
        });
        state.update();
        // Display-wide, as focus can move between any of Syrinx's windows.
        // SAFETY: `Drop` removes this filter while the routing still holds the state.
        unsafe {
            gdk::ffi::gdk_window_add_filter(
                ptr::null_mut(),
                Some(focus_changed),
                Rc::as_ptr(&state).cast_mut().cast(),
            );
        }
        Self { state }
    }

    /// Gives the container X11 focus and selects keys at once, so none are typed past it.
    pub(super) fn focus(&self) {
        self.state.focus();
    }

    pub(super) fn click_focus(&self) -> ClickFocus {
        ClickFocus(self.state.clone())
    }
}

impl Drop for KeyRouting {
    fn drop(&mut self) {
        // SAFETY: the same window, function and data `install` registered.
        unsafe {
            gdk::ffi::gdk_window_remove_filter(
                ptr::null_mut(),
                Some(focus_changed),
                Rc::as_ptr(&self.state).cast_mut().cast(),
            );
        }
    }
}

impl RouteState {
    fn focus(&self) {
        unsafe {
            (self.xlib.XSetInputFocus)(
                self.display,
                self.container,
                xlib::RevertToParent,
                xlib::CurrentTime,
            );
            (self.xlib.XFlush)(self.display);
        }
        self.update();
    }

    fn update(&self) {
        let page_has_keys = self.focus_inside();
        select_keys(&self.window, page_has_keys);
        // Native windows inside (the view's own) never select keys: GTK routes every key
        // through the toplevel window, and one selecting them would take the chrome's.
        self.deselect_keys_inside(self.container, MAX_DEPTH);
    }

    fn deselect_keys_inside(&self, window: c_ulong, depth: usize) {
        let Ok(display) = self.window.display().downcast::<gdkx11::X11Display>() else {
            return;
        };
        let Some(depth) = depth.checked_sub(1) else {
            return;
        };
        let Some(tree) = self.tree(window) else {
            return;
        };
        for child in tree.children {
            if let Some(native) = gdkx11::X11Window::lookup_for_display(&display, child) {
                select_keys(native.upcast_ref(), false);
            }
            self.deselect_keys_inside(child, depth);
        }
    }

    /// Whether X11 focus is on the container or a window inside it.
    fn focus_inside(&self) -> bool {
        let mut focus = 0;
        let mut revert = 0;
        unsafe { (self.xlib.XGetInputFocus)(self.display, &mut focus, &mut revert) };
        let mut window = focus;
        for _ in 0..MAX_DEPTH {
            if window == self.container {
                return true;
            }
            // None (0) and PointerRoot (1) are not windows.
            let Some(tree) = (window > 1).then(|| self.tree(window)).flatten() else {
                return false;
            };
            if tree.parent == 0 || tree.parent == tree.root {
                return false;
            }
            window = tree.parent;
        }
        false
    }

    fn tree(&self, window: c_ulong) -> Option<Tree> {
        let (mut root, mut parent, mut children) = (0, 0, ptr::null_mut());
        let mut count: c_uint = 0;
        let status = unsafe {
            (self.xlib.XQueryTree)(
                self.display,
                window,
                &mut root,
                &mut parent,
                &mut children,
                &mut count,
            )
        };
        let found = if children.is_null() {
            Vec::new()
        } else {
            // SAFETY: Xlib returned `count` window ids at `children`, freed right after copying.
            let length = usize::try_from(count).ok()?;
            let copied = unsafe { std::slice::from_raw_parts(children, length) }.to_vec();
            unsafe { (self.xlib.XFree)(children.cast()) };
            copied
        };
        (status != 0).then_some(Tree {
            root,
            parent,
            children: found,
        })
    }
}

/// One window's place in the X11 tree.
struct Tree {
    root: c_ulong,
    parent: c_ulong,
    children: Vec<c_ulong>,
}

/// GDK calls this for each X event it reads; a focus change reruns the routing.
unsafe extern "C" fn focus_changed(
    xevent: *mut gdk::ffi::GdkXEvent,
    _event: *mut gdk::ffi::GdkEvent,
    data: gtk::glib::ffi::gpointer,
) -> gdk::ffi::GdkFilterReturn {
    // SAFETY: GDK passes the raw X event, and `data` is the state `install` gave it.
    let event = unsafe { &*xevent.cast::<xlib::XEvent>() };
    if is_focus_change(event) {
        let state = unsafe { &*data.cast::<RouteState>() };
        state.update();
    }
    gdk::ffi::GDK_FILTER_CONTINUE
}

/// Core focus events, or XInput2's, which GDK selects instead of the core ones.
fn is_focus_change(event: &xlib::XEvent) -> bool {
    let kind = event.get_type();
    if kind == xlib::GenericEvent {
        // SAFETY: a GenericEvent's leading fields are a cookie; only its header is read.
        let evtype = unsafe { event.generic_event_cookie.evtype };
        return is_xi2_focus_change(evtype);
    }
    kind == xlib::FocusIn || kind == xlib::FocusOut
}

/// XInput2's `XI_FocusIn` and `XI_FocusOut`.
fn is_xi2_focus_change(evtype: i32) -> bool {
    matches!(evtype, XI_FOCUS_IN | XI_FOCUS_OUT)
}

const XI_FOCUS_IN: i32 = 9;
const XI_FOCUS_OUT: i32 = 10;

fn select_keys(window: &gdk::Window, selected: bool) {
    let events = window.events();
    let next = key_events(events, selected);
    if next != events {
        window.set_events(next);
    }
}

/// `events` with the key press and release masks set or cleared.
fn key_events(events: gdk::EventMask, selected: bool) -> gdk::EventMask {
    let keys = gdk::EventMask::KEY_PRESS_MASK | gdk::EventMask::KEY_RELEASE_MASK;
    if selected {
        events | keys
    } else {
        events - keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_xi2_focus_events_count_as_focus_changes() {
        assert!(is_xi2_focus_change(9));
        assert!(is_xi2_focus_change(10));
        for other in [2, 3, 4, 7, 8, 11] {
            assert!(!is_xi2_focus_change(other));
        }
    }

    #[test]
    fn key_selection_changes_only_the_key_masks() {
        let other = gdk::EventMask::FOCUS_CHANGE_MASK | gdk::EventMask::STRUCTURE_MASK;
        let keys = gdk::EventMask::KEY_PRESS_MASK | gdk::EventMask::KEY_RELEASE_MASK;
        assert_eq!(key_events(other, true), other | keys);
        assert_eq!(key_events(other | keys, false), other);
        assert_eq!(key_events(other, false), other);
        assert_eq!(key_events(other | keys, true), other | keys);
    }
}

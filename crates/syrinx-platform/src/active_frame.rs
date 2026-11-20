//! Keeps Syrinx's main window and its owned popups drawn as one active window, so Windows
//! does not flatten the main window's Mica or Acrylic while Syrinx's own palette or menu is open.

use std::cell::{Cell, RefCell};

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GW_OWNER, GetForegroundWindow, GetWindow, IsIconic, SendMessageW, WM_NCACTIVATE,
            WM_NCDESTROY,
        },
    },
};

/// Identifies this subclass among others on the window, such as the caption hit test's.
const SUBCLASS_ID: usize = 0x5359_4146;

/// The `WM_NCACTIVATE` `lparam` that asks `DefWindowProc` not to repaint the frame.
const NO_REPAINT: isize = -1;

thread_local! {
    /// The windows on this thread that carry this subclass. Only they form a family; any
    /// other owned window (a file dialog, a message box) keeps Windows' own behaviour.
    static INSTALLED: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };

    /// Set while this module tells the other windows their frame state, so their own
    /// subclass passes those messages through instead of telling everyone again.
    static TELLING_FAMILY: Cell<bool> = const { Cell::new(false) };
}

/// The window on the other side of a `WM_NCACTIVATE`, relative to Syrinx's windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Counterpart {
    /// The main window or another installed popup it owns.
    Family,
    /// Another application's window, an uninstalled window, or none.
    Outside,
    /// The sender asked for no repaint; leave the message as it is.
    NoRepaint,
}

/// How one window answers a `WM_NCACTIVATE`, and what the rest of the family must be told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameChange {
    /// Whether this window's frame, and so its system backdrop, is drawn active.
    paint_active: bool,
    /// The state to send every other family window, when activation entered or left Syrinx.
    tell_family: Option<bool>,
}

/// Draw `hwnd`'s frame active whenever any window in its family (the main window and the
/// popups it owns that also call this) is active, and inactive once activation leaves them.
///
/// Windows draws a system backdrop's flat fallback on a window whose frame is inactive.
/// Only the drawn frame changes: activation, focus and `WM_ACTIVATE` are untouched, so GPUI's
/// activation observers still see the real state. Call this on the window's own thread,
/// after the window's owner is set.
pub fn install(hwnd: isize) -> Result<(), &'static str> {
    let window = HWND(hwnd as *mut _);
    if !unsafe { SetWindowSubclass(window, Some(subclass), SUBCLASS_ID, 0) }.as_bool() {
        return Err("Syrinx could not keep its window drawn active while its popups are open.");
    }
    INSTALLED.with_borrow_mut(|installed| installed.push(hwnd));
    // A popup that never takes activation, like the sidebar peek, starts with its family's state.
    if let Some(owner) = owner_of(window) {
        let active = in_family(owner, unsafe { GetForegroundWindow() });
        send_frame_state(window, active);
    }
    Ok(())
}

/// Decide a window's frame state from Windows' request and the window on the other side.
fn frame_change(becoming_active: bool, counterpart: Counterpart) -> Option<FrameChange> {
    let change = match (becoming_active, counterpart) {
        (_, Counterpart::NoRepaint) => return None,
        // Activation moved between Syrinx's own windows: every one of them stays active.
        (_, Counterpart::Family) => FrameChange {
            paint_active: true,
            tell_family: None,
        },
        // Activation entered Syrinx from outside, or left it for another window.
        (active, Counterpart::Outside) => FrameChange {
            paint_active: active,
            tell_family: Some(active),
        },
    };
    Some(change)
}

/// The window procedure layer that rewrites `WM_NCACTIVATE` before GPUI passes it on.
unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        forget(hwnd);
    }
    let Some(change) = family_frame_change(hwnd, message, wparam, lparam) else {
        return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    };
    let painted = WPARAM(usize::from(change.paint_active));
    let answer = unsafe { DefSubclassProc(hwnd, message, painted, lparam) };
    if let Some(active) = change.tell_family {
        tell_family(hwnd, active);
    }
    answer
}

/// The frame change for a `WM_NCACTIVATE` this window should rewrite; `None` passes it through.
fn family_frame_change(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<FrameChange> {
    // Windows asks for the default answer on a minimized window.
    if message != WM_NCACTIVATE || TELLING_FAMILY.get() || unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let root = family_root(hwnd);
    frame_change(wparam.0 != 0, counterpart(root, lparam))
}

/// Classify the `lparam` of a `WM_NCACTIVATE`: the window being activated or the one left,
/// or NULL when that window belongs to another application.
fn counterpart(root: HWND, lparam: LPARAM) -> Counterpart {
    if lparam.0 == NO_REPAINT {
        return Counterpart::NoRepaint;
    }
    if in_family(root, HWND(lparam.0 as *mut _)) {
        Counterpart::Family
    } else {
        Counterpart::Outside
    }
}

/// The main window a popup belongs to, or the window itself when nothing owns it.
fn family_root(hwnd: HWND) -> HWND {
    owner_of(hwnd).unwrap_or(hwnd)
}

/// The window that owns `hwnd`, if any.
fn owner_of(hwnd: HWND) -> Option<HWND> {
    // No owner, or a window already gone, both mean the window heads no one's family.
    let Ok(owner) = (unsafe { GetWindow(hwnd, GW_OWNER) }) else {
        return None;
    };
    (!owner.is_invalid()).then_some(owner)
}

/// Whether `other` carries this subclass and is the family's main window or a popup it owns.
fn in_family(root: HWND, other: HWND) -> bool {
    if other.is_invalid() || !is_installed(other) {
        return false;
    }
    is_root_or_owned_by(root, other)
}

/// Whether `install` was called for `hwnd` on this thread and it has not been destroyed.
fn is_installed(hwnd: HWND) -> bool {
    INSTALLED.with_borrow(|installed| installed.contains(&(hwnd.0 as isize)))
}

/// Send every other window of `hwnd`'s family its new frame state.
fn tell_family(hwnd: HWND, active: bool) {
    let root = family_root(hwnd);
    let members: Vec<HWND> = INSTALLED.with_borrow(|installed| {
        installed
            .iter()
            .map(|&member| HWND(member as *mut _))
            .filter(|&member| member != hwnd && is_root_or_owned_by(root, member))
            .collect()
    });
    for member in members {
        send_frame_state(member, active);
    }
}

/// Whether `member` is `root` itself or a window `root` owns.
fn is_root_or_owned_by(root: HWND, member: HWND) -> bool {
    member == root || owner_of(member) == Some(root)
}

/// Ask `DefWindowProc` to draw `hwnd`'s frame active or inactive, without this module's
/// subclass reinterpreting the message.
fn send_frame_state(hwnd: HWND, active: bool) {
    let was_telling = TELLING_FAMILY.replace(true);
    // `WM_NCACTIVATE`'s answer only matters to Windows' own activation, which this is not.
    unsafe {
        SendMessageW(
            hwnd,
            WM_NCACTIVATE,
            Some(WPARAM(usize::from(active))),
            Some(LPARAM(0)),
        );
    }
    TELLING_FAMILY.set(was_telling);
}

/// Drop a destroyed window from its family and remove its subclass, as Comctl32 asks.
fn forget(hwnd: HWND) {
    let handle = hwnd.0 as isize;
    INSTALLED.with_borrow_mut(|installed| installed.retain(|&member| member != handle));
    if !unsafe { RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID) }.as_bool() {
        eprintln!("Syrinx could not stop keeping a closed window drawn active.");
    }
}

#[cfg(test)]
mod tests;

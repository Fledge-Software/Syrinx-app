//! Opens the owned GPUI popups (palette, sidebar peek, menus) that may sit over the page card.

use gpui::{AnyView, Pixels};
use syrinx_platform::browser::{
    BrowserError, OverlayRect, activate_if_foreground, hide_overlay, show_overlay,
    syrinx_in_foreground, window_origin,
};

use super::*;

/// An open popup window and its native handle.
pub(super) struct OwnedPopup {
    pub(super) window: WindowHandle<Root>,
    pub(super) hwnd: isize,
}

/// Why a popup could not be opened.
pub(super) enum OwnedPopupError {
    /// Shown to the owner; a later attempt can try again.
    Notice(String),
    /// A popup without a usable native handle could not be closed.
    Orphaned,
}

/// What to open: the window's title, its name in error messages, and its options.
pub(super) struct PopupSpec {
    pub(super) title: &'static str,
    pub(super) noun: &'static str,
    pub(super) owner: isize,
    pub(super) options: WindowOptions,
}

/// Opens a popup owned by Syrinx's main window and not topmost, so it stays above Syrinx
/// (and its page) without floating over other applications. `build` receives the native
/// handle and returns the popup's content.
///
/// Call it outside every entity update: GPUI draws a new window before `open_window`
/// returns, and a content view that reads the browser would find it already leased.
pub(super) fn open_owned_popup(
    spec: PopupSpec,
    cx: &mut App,
    build: impl FnOnce(isize, &mut Window, &mut App) -> AnyView + 'static,
) -> Result<OwnedPopup, OwnedPopupError> {
    let native = Rc::new(RefCell::new(Err(format!(
        "The {} was not created.",
        spec.noun
    ))));
    let created = native.clone();
    let PopupSpec {
        title,
        noun,
        owner,
        options,
    } = spec;
    let starts_hidden = !options.show;
    let opened = cx.open_window(options, move |popup, cx| {
        popup.set_window_title(title);
        let owned = own_popup(popup, owner);
        let content = match &owned {
            Ok(hwnd) => build(*hwnd, popup, cx),
            // The window is removed as soon as `open_window` returns.
            Err(_) => cx.new(|_| gpui::EmptyView).into(),
        };
        *created.borrow_mut() = owned;
        cx.new(|cx| Root::new(content, popup, cx))
    });
    let Ok(window) = opened else {
        return Err(OwnedPopupError::Notice(format!("Cannot open the {noun}.")));
    };
    let native = native.borrow().clone();
    match native {
        Ok(hwnd) => {
            if starts_hidden {
                keep_hidden(hwnd, owner, noun);
            }
            Ok(OwnedPopup { window, hwnd })
        }
        Err(error) => match window.update(cx, |_, popup, _| popup.remove_window()) {
            Ok(()) => Err(OwnedPopupError::Notice(error)),
            Err(_) => Err(OwnedPopupError::Orphaned),
        },
    }
}

/// Hides a popup opened with `show: false`. GPUI's X11 backend maps every window it opens,
/// so without this the kept palette sat visible at its creation spot; on Windows it is a no-op.
fn keep_hidden(hwnd: isize, owner: isize, noun: &str) {
    if let Err(error) = hide_overlay(hwnd, owner) {
        eprintln!("Cannot hide the new {noun}: {error}");
    }
}

/// The main window's bounds with its origin read from the platform where GPUI's is stale
/// (X11 after a move and resize in one step), so popups line up with where the window is.
pub(super) fn main_window_bounds(window: &Window, parent: isize) -> Bounds<Pixels> {
    corrected_bounds(
        window.bounds(),
        window_origin(parent),
        window.scale_factor(),
    )
}

/// `bounds` moved to `origin`, a physical point, at `scale`; unchanged without an origin.
fn corrected_bounds(
    bounds: Bounds<Pixels>,
    origin: Option<(i32, i32)>,
    scale: f32,
) -> Bounds<Pixels> {
    let Some((left, top)) = origin.filter(|_| scale > 0.) else {
        return bounds;
    };
    Bounds::new(
        point(px(left as f32 / scale), px(top as f32 / scale)),
        bounds.size,
    )
}

/// Makes the popup an owned, non-topmost window with Syrinx's icon, whose frame is drawn
/// active with the main window's; its handle on success.
fn own_popup(popup: &Window, owner: isize) -> Result<isize, String> {
    let hwnd = native_handle(popup).map_err(str::to_owned)?;
    set_overlay_owner(hwnd, owner).map_err(|error| error.to_string())?;
    show_app_icon(hwnd);
    keep_frame_with_family(hwnd);
    Ok(hwnd)
}

/// Keep a Syrinx window's frame, and so its Mica or Acrylic, drawn active while the main
/// window or one of these popups has activation. A failure only loses that, so it is reported.
pub(super) fn keep_frame_with_family(hwnd: isize) {
    if let Err(error) = syrinx_platform::active_frame::install(hwnd) {
        eprintln!("{error}");
    }
}

/// Gives an owned popup Windows 11's rounded corners and system shadow (#72, #85).
/// A failure leaves a square, shadowless popup that still works, so it is only reported.
pub(super) fn round_popup_corners(hwnd: isize, noun: &str) {
    if let Err(error) = syrinx_platform::window_frame::round_with_shadow(hwnd) {
        eprintln!("Cannot round the {noun}'s corners: {error}");
    }
}

/// Places, shows and activates a kept popup while Syrinx is in front; `Ok(false)` when another
/// application is, because Syrinx never takes the foreground from it (owner, 2026-10-12).
/// Call it from a spawned task, outside any GPUI update: Windows delivers the popup's resize,
/// first frame and activation from these calls, and GPUI drops them while an update holds the app.
pub(super) fn show_and_activate(hwnd: isize, rect: OverlayRect) -> Result<bool, BrowserError> {
    if !syrinx_in_foreground() {
        return Ok(false);
    }
    show_overlay(hwnd, rect).and_then(|()| activate_if_foreground(hwnd))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stale() -> Bounds<Pixels> {
        Bounds::new(point(px(1536.), px(26.)), size(px(1024.), px(656.)))
    }

    #[test]
    fn a_platform_origin_replaces_a_stale_one_and_keeps_the_size() {
        let corrected = corrected_bounds(stale(), Some((2560, 326)), 1.25);
        assert_eq!(corrected.origin, point(px(2048.), px(260.8)));
        assert_eq!(corrected.size, stale().size);
    }

    #[test]
    fn without_a_platform_origin_gpui_bounds_stand() {
        assert_eq!(corrected_bounds(stale(), None, 1.25), stale());
        assert_eq!(corrected_bounds(stale(), Some((10, 10)), 0.), stale());
    }
}

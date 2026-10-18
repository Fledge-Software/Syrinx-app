//! Opens the owned GPUI popups (palette, sidebar peek, menus) that may sit over the page card.

use gpui::AnyView;

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
        Ok(hwnd) => Ok(OwnedPopup { window, hwnd }),
        Err(error) => match window.update(cx, |_, popup, _| popup.remove_window()) {
            Ok(()) => Err(OwnedPopupError::Notice(error)),
            Err(_) => Err(OwnedPopupError::Orphaned),
        },
    }
}

/// Makes the popup an owned, non-topmost window with Syrinx's icon; its handle on success.
fn own_popup(popup: &Window, owner: isize) -> Result<isize, String> {
    let hwnd = native_handle(popup).map_err(str::to_owned)?;
    set_overlay_owner(hwnd, owner).map_err(|error| error.to_string())?;
    show_app_icon(hwnd);
    Ok(hwnd)
}

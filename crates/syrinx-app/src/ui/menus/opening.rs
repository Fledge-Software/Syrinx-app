//! Opens a menu's popup outside every update, applies what the popup asks in the main
//! window, and closes it, giving the keys back where the menu took them from.

use super::*;

/// Opens a requested menu outside every entity update, then hands the popup to the browser.
pub(super) fn open_requested_menu(
    browser: WeakEntity<BrowserView>,
    request: MenuRequest,
    cx: &mut App,
) {
    let Some(entity) = browser.upgrade() else {
        return; // The owner has gone, so nothing awaits this popup.
    };
    if !entity.read(cx).awaits_menu(request.generation) {
        return;
    }
    let MenuRequest {
        generation,
        kind,
        rows,
        zoom,
        rect,
        parent,
        select_first,
    } = request;
    let spec = PopupSpec {
        title: "Syrinx menu",
        noun: "menu",
        owner: parent,
        options: WindowOptions {
            kind: WindowKind::PopUp,
            titlebar: None,
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.), px(0.)),
                kind_size(kind, &rows),
            ))),
            focus: false,
            show: false,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        },
    };
    let created = Rc::new(RefCell::new(None));
    let retained = created.clone();
    let opened = open_owned_popup(spec, cx, move |hwnd, popup, cx| {
        round_popup_corners(hwnd, "menu");
        let closing = browser.clone();
        popup.on_window_should_close(cx, move |_, cx| {
            let queued = closing.update(cx, |browser, cx| {
                browser.queue_menu_action(generation, MenuAction::Dismiss, cx);
            });
            queued.is_err() // With no owner entity, nothing else will close this window.
        });
        let selected = if select_first {
            first_selectable(&rows)
        } else {
            None
        };
        let menu = MenuContent {
            kind,
            rows,
            zoom,
            selected,
        };
        let view = cx.new(|cx| MenuView::new(browser, generation, menu, popup, cx));
        *retained.borrow_mut() = Some(view.downgrade());
        view.into()
    });
    let opened = opened.map(|popup| {
        let view = created.borrow_mut().take().expect("created menu view");
        (popup, view)
    });
    entity.update(cx, |browser, cx| {
        browser.install_menu(generation, rect, opened, cx);
    });
}

/// Hides an active popup through its owner, then removes it.
pub(super) fn remove_menu_window(popup: OwnedPopup, owner: isize, cx: &mut App) {
    if let Err(error) = hide_overlay(popup.hwnd, owner) {
        eprintln!("Cannot hide the menu: {error}");
    }
    let handle: gpui::AnyWindowHandle = popup.window.into();
    if cx.windows().contains(&handle)
        && handle
            .update(cx, |_, window, _| window.remove_window())
            .is_err()
    {
        eprintln!("Cannot close the menu popup.");
    }
}

/// Whether a press on `kind`'s button closes its menu instead of opening it: the menu is
/// still open, or the same press closed it a moment ago by deactivating its popup.
pub(super) fn closes_on_reclick(
    open: Option<MenuKind>,
    dismissed: Option<MenuDismissal>,
    kind: MenuKind,
    now: Instant,
) -> bool {
    open == Some(kind)
        || dismissed.is_some_and(|dismissal| {
            dismissal.kind == kind && now.saturating_duration_since(dismissal.at) < RECLICK_WINDOW
        })
}

impl BrowserView {
    /// A press on an open menu's button closes it. Depending on when Windows reports the
    /// popup's deactivation, the menu is either still open or was closed by this press.
    pub(in crate::ui) fn toggled_closed(&mut self, kind: MenuKind, cx: &mut Context<Self>) -> bool {
        let open = self.menu.as_ref().map(|menu| menu.kind);
        let dismissed = self.menu_dismissal.take();
        if !closes_on_reclick(open, dismissed, kind, Instant::now()) {
            return false;
        }
        self.retire_menu(cx);
        cx.notify();
        true
    }

    /// Records the menu and opens its popup once this update ends (see `open_requested_menu`).
    pub(super) fn show_menu_rows(
        &mut self,
        kind: MenuKind,
        rows: Vec<MenuRow>,
        opening: MenuOpening,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The peek's copy of the sidebar has no menu.
        if !self.is_main_window(window) || self.closing {
            return;
        }
        // A menu replaces the palette and the peek; a live palette's late close would refocus
        // the page and close the menu again (#75).
        self.retire_palette(cx);
        self.retire_peek(cx);
        self.retire_menu(cx);
        // A Back or Forward menu still waiting for its rows must not replace this one.
        self.cancel_history_request();
        let MenuOpening {
            anchor,
            returns,
            select_first,
            asked,
        } = opening;
        let size = kind_size(kind, &rows);
        let rect = menu_rect(
            main_window_bounds(window, self.parent_hwnd),
            window.scale_factor(),
            anchor,
            size,
        );
        self.menu_generation = self
            .menu_generation
            .checked_add(1)
            .expect("menu generations exhausted");
        let generation = self.menu_generation;
        self.menu = Some(OpenMenu {
            kind,
            generation,
            popup: None,
            view: None,
            anchor,
            returns,
            active: self.model.active_id,
            asked,
        });
        let request = MenuRequest {
            generation,
            kind,
            rows,
            zoom: self.shown_zoom(),
            rect,
            parent: self.parent_hwnd,
            select_first,
        };
        let browser = cx.weak_entity();
        cx.defer(move |cx| open_requested_menu(browser, request, cx));
        cx.notify();
    }

    /// Closes the open menu, if any, as `action` from its popup would. `MenuAction::Dismiss`
    /// closes it as Escape does: the keys go back where it took them from, if Syrinx is in
    /// front, as every other close does.
    pub(super) fn close_menu_by(
        &mut self,
        action: MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(generation) = self.menu.as_ref().map(|menu| menu.generation) else {
            return;
        };
        self.apply_menu_action(generation, action, window, cx);
    }

    /// Which menu is open or opening, if any.
    pub(in crate::ui) fn open_menu_kind(&self) -> Option<MenuKind> {
        self.menu.as_ref().map(|menu| menu.kind)
    }

    /// Whether a popup opened for `generation` is still wanted.
    pub(super) fn awaits_menu(&self, generation: u64) -> bool {
        !self.closing
            && self
                .menu
                .as_ref()
                .is_some_and(|menu| menu.generation == generation && menu.popup.is_none())
    }

    /// Adopts the opened popup and shows it, or closes it when the menu was closed meanwhile.
    pub(super) fn install_menu(
        &mut self,
        generation: u64,
        rect: OverlayRect,
        opened: Result<(OwnedPopup, WeakEntity<MenuView>), OwnedPopupError>,
        cx: &mut Context<Self>,
    ) {
        let wanted = self.awaits_menu(generation);
        match opened {
            Ok((popup, view)) if wanted => {
                let hwnd = popup.hwnd;
                if let Some(menu) = &mut self.menu {
                    menu.popup = Some(popup);
                    menu.view = Some(view);
                }
                self.show_menu_popup(generation, hwnd, rect, cx);
            }
            Ok((stale, _)) => remove_menu_window(stale, self.parent_hwnd, cx),
            Err(error) => {
                if wanted {
                    self.menu = None;
                }
                match error {
                    OwnedPopupError::Notice(message) if wanted => self.message = message,
                    OwnedPopupError::Notice(_) => {}
                    OwnedPopupError::Orphaned => self.fail("Cannot close the failed menu popup."),
                }
            }
        }
        cx.notify();
    }

    /// Places, shows and activates the popup from a task, outside any GPUI update: Windows
    /// delivers its resize, first frame and activation from these calls.
    pub(super) fn show_menu_popup(
        &mut self,
        generation: u64,
        hwnd: isize,
        rect: OverlayRect,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |view, cx| {
            let Ok(true) = view.update(cx, |view, _| view.is_current_menu(generation)) else {
                return; // The menu closed before it was shown, or the browser is gone.
            };
            let shown = show_and_activate(hwnd, rect);
            if view
                .update(cx, |view, cx| view.menu_shown(generation, shown, cx))
                .is_err()
            {
                eprintln!("The menu was shown after its browser was removed.");
            }
        })
        .detach();
    }

    /// A menu that could not take the keys closes rather than float without them.
    pub(super) fn menu_shown(
        &mut self,
        generation: u64,
        shown: Result<bool, BrowserError>,
        cx: &mut Context<Self>,
    ) {
        if !self.is_current_menu(generation) {
            return;
        }
        match shown {
            Ok(true) => {
                self.menu_activated(cx);
                return;
            }
            Ok(false) => {
                self.message = "Syrinx was not in front, so the menu stayed closed.".into()
            }
            Err(error) => self.message = format!("Cannot show the menu: {error}"),
        }
        self.retire_menu(cx);
        cx.notify();
    }

    /// The popup now has the keys, so any later deactivation is the owner leaving it, even
    /// one GPUI delivers before the popup's own activation callback has run.
    pub(super) fn menu_activated(&self, cx: &mut Context<Self>) {
        let Some(view) = self.menu.as_ref().and_then(|menu| menu.view.clone()) else {
            return;
        };
        if view.update(cx, |menu, _| menu.was_active = true).is_err() {
            eprintln!("The menu popup closed before it took the keys.");
        }
    }

    /// The page in view reported a new zoom level; an open page menu's Zoom row shows it.
    pub(in crate::ui) fn menu_zoom_changed(&self, factor: f64, cx: &mut Context<Self>) {
        let Some(view) = self.menu.as_ref().and_then(|menu| menu.view.clone()) else {
            return;
        };
        let updated = view.update(cx, |menu, cx| {
            menu.zoom = factor;
            cx.notify();
        });
        if updated.is_err() {
            eprintln!("The menu popup closed before its zoom level changed.");
        }
    }

    pub(super) fn is_current_menu(&self, generation: u64) -> bool {
        self.menu
            .as_ref()
            .is_some_and(|menu| menu.generation == generation)
    }

    /// Popup input is applied in the main window, after the popup's own update ends.
    pub(super) fn queue_menu_action(
        &self,
        generation: u64,
        action: MenuAction,
        cx: &mut Context<Self>,
    ) {
        let main = self.main_window;
        let view = cx.weak_entity();
        cx.defer(move |cx| {
            if !cx.windows().contains(&main) {
                return;
            }
            let delivered = main.update(cx, |_, window, cx| {
                view.update(cx, |view, cx| {
                    view.apply_menu_action(generation, action, window, cx);
                })
            });
            if !matches!(delivered, Ok(Ok(()))) {
                eprintln!("Menu input arrived after its browser was removed.");
            }
        });
    }

    pub(super) fn apply_menu_action(
        &mut self,
        generation: u64,
        action: MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A closed or replaced menu's late input must not act on the current one.
        if self.closing || !self.is_current_menu(generation) {
            return;
        }
        // A zoom step keeps the menu open, so the owner can step again.
        if let MenuAction::Zoom(shortcut) = action {
            self.essential_shortcut(shortcut);
            return;
        }
        if let MenuAction::Download(download, control) = action {
            self.control_download(download, control);
            return;
        }
        // A queued key or click must not reactivate Syrinx after Alt Tab.
        let in_front = syrinx_in_foreground();
        let Some(menu) = self.menu.as_ref() else {
            return;
        };
        let kind = menu.kind;
        let origin = MenuOrigin {
            anchor: menu.anchor,
            returns: menu.returns.clone(),
            active: menu.active,
            asked: menu.asked.clone(),
        };
        self.retire_menu(cx);
        match action.after_close(in_front) {
            AfterClose::RecordDismissal => {
                self.menu_dismissal = Some(MenuDismissal {
                    kind,
                    at: Instant::now(),
                });
            }
            AfterClose::ReturnKeys => self.return_menu_focus(origin.returns, window, cx),
            AfterClose::Run(item) => self.run_menu_item(item, origin, window, cx),
            AfterClose::Nothing => {}
        }
        cx.notify();
    }

    /// Closes the open menu, if any. An active popup hands activation to the main window
    /// first, so Windows does not pick the next window itself.
    pub(in crate::ui) fn retire_menu(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        if let Some(popup) = menu.popup {
            remove_menu_window(popup, self.parent_hwnd, cx);
        }
    }

    /// Gives the keys back after a menu closes: to the page, the row a key opened it from, or
    /// the control a click found them on. A row or control gone since falls back to the page.
    pub(super) fn return_menu_focus(
        &mut self,
        returns: MenuReturn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let returned = match returns {
            MenuReturn::Page | MenuReturn::Was(ReturnFocus::Page) => false,
            MenuReturn::Row(row) => {
                self.move_keys_to_chrome() && self.sidebar_trees.focus_main_row(row, window, cx)
            }
            MenuReturn::Was(ReturnFocus::Control(handle)) => {
                self.return_keys_to_control(&handle, window, cx)
            }
        };
        if !returned {
            self.focus_page(window, cx);
        }
    }

    /// Gives the keys to a chrome control the main window still draws; returns whether it did.
    pub(super) fn return_keys_to_control(
        &mut self,
        handle: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // The root's own handle contains every control drawn in the last frame, and only those.
        if !self.focus.contains(handle, window) || !self.move_keys_to_chrome() {
            return false;
        }
        window.focus(handle, cx);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_on_the_open_menus_button_closes_it_whenever_deactivation_arrives() {
        let now = Instant::now();
        let just = |kind| {
            Some(MenuDismissal {
                kind,
                at: now - Duration::from_millis(50),
            })
        };
        // Deactivation not yet delivered: the menu is still open.
        assert!(closes_on_reclick(
            Some(MenuKind::Page),
            None,
            MenuKind::Page,
            now
        ));
        // Deactivation delivered first: the same press closed it a moment ago.
        assert!(closes_on_reclick(
            None,
            just(MenuKind::Page),
            MenuKind::Page,
            now
        ));
        // Another menu's button, or a press long after, opens the menu.
        assert!(!closes_on_reclick(
            None,
            just(MenuKind::Sidebar),
            MenuKind::Page,
            now
        ));
        assert!(!closes_on_reclick(
            Some(MenuKind::Sidebar),
            None,
            MenuKind::Page,
            now
        ));
        let earlier = Some(MenuDismissal {
            kind: MenuKind::Page,
            at: now - Duration::from_secs(2),
        });
        assert!(!closes_on_reclick(None, earlier, MenuKind::Page, now));
    }
}

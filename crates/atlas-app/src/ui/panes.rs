//! Moves keyboard focus between the address field, bars, sidebar and page with F6 (P1-36).
//!
//! The order itself is `atlas_ui::focus`; this file finds the focused pane and lands on one.

use atlas_ui::focus::{Direction, OpenPanes, Pane, next_pane};
use gpui::Role;

use super::*;

actions!(
    panes,
    [
        FocusNextPane,
        FocusPreviousPane,
        FocusNextControl,
        FocusPreviousControl
    ]
);

/// F6 cycles panes; Tab moves between the controls of whichever Atlas window has focus.
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("f6", FocusNextPane, None),
        KeyBinding::new("shift-f6", FocusPreviousPane, None),
        KeyBinding::new("tab", FocusNextControl, None),
        KeyBinding::new("shift-tab", FocusPreviousControl, None),
    ]);
}

/// One focus handle per pane region; F6 asks each whether it holds focus. The address pane
/// is the whole toolbar, but F6 lands on its field.
pub(super) struct PaneFocus {
    address_field: FocusHandle,
    address: FocusHandle,
    runtime_notice: FocusHandle,
    permission: FocusHandle,
    find: FocusHandle,
    sidebar: FocusHandle,
    page: FocusHandle,
}

impl PaneFocus {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            address_field: cx.focus_handle().tab_stop(true),
            address: cx.focus_handle(),
            runtime_notice: cx.focus_handle(),
            permission: cx.focus_handle(),
            find: cx.focus_handle(),
            sidebar: cx.focus_handle(),
            page: cx.focus_handle(),
        }
    }

    fn handle(&self, pane: Pane) -> &FocusHandle {
        match pane {
            Pane::Address => &self.address,
            Pane::RuntimeNotice => &self.runtime_notice,
            Pane::Permission => &self.permission,
            Pane::Find => &self.find,
            Pane::Sidebar => &self.sidebar,
            Pane::Page => &self.page,
        }
    }
}

impl BrowserView {
    /// The panes this frame draws.
    pub(super) fn open_panes(&self) -> OpenPanes {
        OpenPanes {
            runtime_notice: self.runtime.change().is_some(),
            permission: self.active_permission().is_some(),
            find: self.find.is_some(),
            sidebar: atlas_ui::sidebar::draws_content(self.sidebar_sizing.width()),
        }
    }

    /// Moves to the next or previous pane. `from_page` says the key came from a focused web
    /// page, which GPUI cannot see.
    pub(super) fn cycle_pane(
        &mut self,
        direction: Direction,
        from_page: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing || self.fullscreen.is_some() {
            return;
        }
        let current = if from_page {
            Some(Pane::Page)
        } else {
            self.focused_pane(window, cx)
        };
        let target = next_pane(current, self.open_panes(), direction);
        self.land_on(target, window, cx);
        cx.notify();
    }

    /// The pane whose region holds GPUI focus, if any.
    fn focused_pane(&self, window: &Window, cx: &App) -> Option<Pane> {
        Pane::ORDER
            .into_iter()
            .find(|pane| self.panes.handle(*pane).contains_focused(window, cx))
    }

    /// Focuses the pane: a web page natively, the address field directly, anything else at
    /// its first control.
    fn land_on(&mut self, pane: Pane, window: &mut Window, cx: &mut Context<Self>) {
        if pane == Pane::Page && self.page_card_shows_web_page() {
            self.focus_page(window, cx);
            return;
        }
        if let Err(error) = atlas_platform::browser::focus_chrome(self.parent_hwnd) {
            self.message = error.to_string();
            return;
        }
        window.activate_window();
        if pane == Pane::Address {
            let field = self.panes.address_field.clone();
            window.focus(&field, cx);
            return;
        }
        let region = self.panes.handle(pane).clone();
        window.focus(&region, cx);
        window.focus_next(cx);
    }

    /// Whether the page card shows the native web page rather than Atlas's own content.
    fn page_card_shows_web_page(&self) -> bool {
        let crashed = self
            .shown
            .is_some_and(|id| self.crashes.is_crashed(id.get()));
        self.shown.is_some() && !self.history.is_open() && !self.clear_data.is_open() && !crashed
    }

    /// A page that took focus by click or Tab clears GPUI's focus, so no chrome control
    /// keeps showing a ring while keys go to the page.
    pub(super) fn page_took_focus(&mut self, id: u64, window: &mut Window, cx: &mut App) {
        if self.shown.map(TabId::get) == Some(id) {
            window.focus(&self.focus, cx);
        }
    }

    /// Marks a region as an F6 pane: screen readers hear its name, and F6 can find it.
    pub(super) fn as_pane(&self, region: Div, pane: Pane, role: Role) -> Stateful<Div> {
        region
            .id(pane.name())
            .role(role)
            .aria_label(pane.name())
            .track_focus(self.panes.handle(pane))
    }

    /// Wraps a bar that may be absent; an empty bar is not a pane.
    pub(super) fn bar_pane(
        &self,
        bar: Option<Div>,
        pane: Pane,
        role: Role,
    ) -> Option<Stateful<Div>> {
        let bar = bar?;
        Some(self.as_pane(div().flex().flex_col().flex_none().child(bar), pane, role))
    }

    /// The address field's own focus handle, where F6 lands in the toolbar.
    pub(super) fn address_field_focus(&self) -> &FocusHandle {
        &self.panes.address_field
    }
}

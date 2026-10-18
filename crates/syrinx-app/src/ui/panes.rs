//! Moves keyboard focus between the address field, bars, sidebar and page with F6 (P1-36).
//!
//! The order itself is `syrinx_ui::focus`; this file finds the focused pane and lands on one.

use gpui::Role;
use syrinx_ui::focus::{
    Direction, Entry, Landing, OpenPanes, Pane, Settle, landing, next_pane, settle,
};

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

/// F6 cycles panes; Tab moves between the controls of whichever Syrinx window has focus.
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

/// An upper bound on the Tab steps taken to find a pane's last control, so a Tab order that
/// never leaves the pane cannot loop forever.
const MAX_PANE_CONTROLS: usize = 4096;

/// Steps from the region to its first Tab stop, or stays on the region when it has none.
fn focus_first_control(region: &FocusHandle, window: &mut Window, cx: &mut App) {
    window.focus(region, cx);
    window.focus_next(cx);
    if settle(focus_inside(region, window, cx)) == Settle::Region {
        window.focus(region, cx);
    }
}

/// Walks the region's Tab stops to its last one, or stays on the region when it has none.
fn focus_last_control(region: &FocusHandle, window: &mut Window, cx: &mut App) {
    focus_first_control(region, window, cx);
    for _ in 0..MAX_PANE_CONTROLS {
        let Some(current) = window.focused(cx) else {
            return;
        };
        if region.is_focused(window) {
            return;
        }
        window.focus_next(cx);
        if settle(focus_inside(region, window, cx)) == Settle::Region {
            window.focus(&current, cx);
            return;
        }
    }
}

/// Whether focus is on a control inside the region, not on the region itself.
fn focus_inside(region: &FocusHandle, window: &Window, cx: &App) -> bool {
    region.contains_focused(window, cx) && !region.is_focused(window)
}

impl BrowserView {
    /// The panes this frame draws.
    pub(super) fn open_panes(&self) -> OpenPanes {
        OpenPanes {
            runtime_notice: self.runtime.change().is_some(),
            permission: self.active_permission().is_some(),
            find: self.find.is_some(),
            sidebar: syrinx_ui::sidebar::draws_content(self.sidebar_sizing.width()),
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
        self.land_on(target, Entry::First, window, cx);
        cx.notify();
    }

    /// Shift Tab past a page's first element: the previous pane, entered at its last control,
    /// as Tab order would have it.
    pub(super) fn leave_page_backward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing || self.fullscreen.is_some() {
            return;
        }
        let target = next_pane(Some(Pane::Page), self.open_panes(), Direction::Backward);
        self.land_on(target, Entry::Last, window, cx);
        cx.notify();
    }

    /// The pane whose region holds GPUI focus, if any.
    fn focused_pane(&self, window: &Window, cx: &App) -> Option<Pane> {
        Pane::ORDER
            .into_iter()
            .find(|pane| self.panes.handle(*pane).contains_focused(window, cx))
    }

    /// Focuses the pane: a web page natively, the address field directly, anything else at
    /// the control `entry` names, or at the region itself when it has no controls.
    fn land_on(&mut self, pane: Pane, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        let landing = landing(pane, entry, self.page_card_shows_web_page());
        if landing == Landing::WebPage {
            self.focus_page(window, cx);
            return;
        }
        if let Err(error) = syrinx_platform::browser::focus_chrome(self.parent_hwnd) {
            self.message = error.to_string();
            return;
        }
        // Native activation: GPUI's `activate_window` sends a synthetic Alt and forces the
        // foreground (owner decision, 2026-10-12).
        self.activate_main_now();
        let Landing::Control(entry) = landing else {
            let field = self.panes.address_field.clone();
            window.focus(&field, cx);
            return;
        };
        // The sidebar tree has one Tab stop; F6 lands on its active row (DW-61).
        if pane == Pane::Sidebar && entry == Entry::First && self.focus_active_row(window, cx) {
            return;
        }
        let region = self.panes.handle(pane).clone();
        match entry {
            Entry::First => focus_first_control(&region, window, cx),
            Entry::Last => focus_last_control(&region, window, cx),
        }
    }

    /// Whether the page card shows the native web page rather than Syrinx's own content.
    pub(super) fn page_card_shows_web_page(&self) -> bool {
        let crashed = self
            .shown
            .is_some_and(|id| self.crashes.is_crashed(id.get()));
        self.shown.is_some() && !self.history.is_open() && !self.clear_data.is_open() && !crashed
    }

    /// A page that took focus by click or Tab clears GPUI's focus, so no chrome control
    /// keeps showing a ring while keys go to the page.
    pub(super) fn page_took_focus(&mut self, id: u64, window: &mut Window, cx: &mut App) {
        let chrome_kept_keys =
            syrinx_platform::browser::chrome_has_keyboard_focus(self.parent_hwnd);
        if page_focus_clears_chrome(self.shown.map(TabId::get), id, chrome_kept_keys) {
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

/// A shown page's `GotFocus` clears chrome focus only when the keys really left the chrome.
fn page_focus_clears_chrome(shown: Option<u64>, id: u64, chrome_kept_keys: bool) -> bool {
    shown == Some(id) && !chrome_kept_keys
}

#[cfg(test)]
mod tests {
    use super::page_focus_clears_chrome;

    #[test]
    fn a_find_highlight_focus_event_leaves_the_find_field_focused() {
        // WebView2 raised GotFocus during native find while Windows focus stayed on the chrome (#57).
        assert!(!page_focus_clears_chrome(Some(7), 7, true));
    }

    #[test]
    fn a_real_click_into_the_shown_page_still_clears_the_focus_ring() {
        assert!(page_focus_clears_chrome(Some(7), 7, false));
        assert!(!page_focus_clears_chrome(Some(8), 7, false));
        assert!(!page_focus_clears_chrome(None, 7, false));
    }
}

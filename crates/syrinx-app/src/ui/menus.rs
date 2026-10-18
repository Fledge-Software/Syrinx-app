//! The chrome's two drop-down menus: the page menu behind ☰ and the sidebar's ⋯ menu.
//! Each also opens where the owner right-clicks, and every item shows its shortcut.

use gpui::{Anchor, KeyDownEvent, MouseDownEvent, Pixels, Point, anchored, deferred};
use syrinx_core::{DestinationSurface, Shortcut};

use super::*;

/// The menu's width, narrow enough that the sidebar menu stays inside a wide sidebar.
const MENU_WIDTH: f32 = 220.;
const ITEM_HEIGHT: f32 = 30.;

/// Which menu is open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MenuKind {
    Page,
    Sidebar,
}

/// One menu entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuItem {
    Find,
    Downloads,
    Print,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    SavePage,
    History,
    NewTab,
    NewFolder,
    EditFavorites,
}

const PAGE_ITEMS: [MenuItem; 8] = [
    MenuItem::Find,
    MenuItem::Downloads,
    MenuItem::Print,
    MenuItem::ZoomIn,
    MenuItem::ZoomOut,
    MenuItem::ResetZoom,
    MenuItem::SavePage,
    MenuItem::History,
];
const SIDEBAR_ITEMS: [MenuItem; 3] = [
    MenuItem::NewTab,
    MenuItem::NewFolder,
    MenuItem::EditFavorites,
];

impl MenuKind {
    fn items(self) -> &'static [MenuItem] {
        match self {
            MenuKind::Page => &PAGE_ITEMS,
            MenuKind::Sidebar => &SIDEBAR_ITEMS,
        }
    }
}

impl MenuItem {
    fn label(self) -> &'static str {
        match self {
            MenuItem::Find => "Find in page",
            MenuItem::Downloads => "Downloads",
            MenuItem::Print => "Print",
            MenuItem::ZoomIn => "Zoom in",
            MenuItem::ZoomOut => "Zoom out",
            MenuItem::ResetZoom => "Reset zoom",
            MenuItem::SavePage => "Save page",
            MenuItem::History => "History",
            MenuItem::NewTab => "New tab",
            MenuItem::NewFolder => "New folder",
            MenuItem::EditFavorites => "Edit favorites",
        }
    }

    /// The item's shortcut as the command registry spells it; zoom keys are bound in
    /// `essentials` because the registry cannot spell `+` or `-`.
    fn shortcut(self) -> Option<String> {
        let keys = match self {
            MenuItem::Find => "ctrl-f",
            MenuItem::Downloads => "ctrl-j",
            MenuItem::Print => "ctrl-p",
            MenuItem::ZoomIn => return Some("Ctrl +".to_owned()),
            MenuItem::ZoomOut => return Some("Ctrl −".to_owned()),
            MenuItem::ResetZoom => "ctrl-0",
            MenuItem::SavePage => "ctrl-d",
            MenuItem::History => "ctrl-h",
            MenuItem::NewTab => "ctrl-t",
            MenuItem::NewFolder | MenuItem::EditFavorites => return None,
        };
        Shortcut::new(keys).ok().map(|shortcut| shortcut.label())
    }
}

/// The open menu and where its corner sits, in window coordinates.
pub(super) struct OpenMenu {
    kind: MenuKind,
    position: Point<Pixels>,
    corner: Anchor,
}

impl BrowserView {
    /// Opens `kind` with its top-left corner at the pointer, as a right-click does.
    pub(super) fn open_menu_at(
        &mut self,
        kind: MenuKind,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut position = event.position;
        // The sidebar menu stays inside the sidebar, clear of the page's web view.
        if kind == MenuKind::Sidebar {
            let limit = (self.sidebar_sizing.width() - MENU_WIDTH - 8.).max(8.);
            position.x = position.x.min(px(limit));
        }
        self.show_menu(kind, position, Anchor::TopLeft, window, cx);
    }

    /// Opens the page menu under the ☰ button in the title bar's right corner.
    pub(super) fn open_page_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let right = f32::from(window.viewport_size().width) - 3. * TITLE_BAR_HEIGHT - 4.;
        let position = point(px(right), px(TITLE_BAR_HEIGHT));
        self.show_menu(MenuKind::Page, position, Anchor::TopRight, window, cx);
    }

    /// Opens the sidebar menu under its ⋯ button.
    pub(super) fn open_sidebar_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_menu(MenuKind::Sidebar, position, Anchor::TopLeft, window, cx);
    }

    fn show_menu(
        &mut self,
        kind: MenuKind,
        position: Point<Pixels>,
        corner: Anchor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The menu draws in the main window; the peek's copy of the sidebar has no menu.
        if !self.is_main_window(window) {
            return;
        }
        // Windows focus moves from the page to the chrome, so Escape reaches the menu.
        if let Err(error) = syrinx_platform::browser::focus_chrome(self.parent_hwnd) {
            self.message = error.to_string();
        }
        self.menu = Some(OpenMenu {
            kind,
            position,
            corner,
        });
        window.focus(&self.menu_focus, cx);
        cx.notify();
    }

    /// Escape: closes an open menu and gives the keys back to the page; false if none was open.
    pub(super) fn close_menu_to_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.menu.is_none() {
            return false;
        }
        self.close_menu(cx);
        self.focus_page(window, cx);
        true
    }

    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Does what the chosen item names, then closes the menu.
    fn run_menu_item(&mut self, item: MenuItem, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        match item {
            MenuItem::Find => self.open_find(window, cx),
            MenuItem::Downloads => self.essential_shortcut(BrowserShortcut::Downloads),
            MenuItem::Print => self.essential_shortcut(BrowserShortcut::Print),
            MenuItem::ZoomIn => self.essential_shortcut(BrowserShortcut::ZoomIn),
            MenuItem::ZoomOut => self.essential_shortcut(BrowserShortcut::ZoomOut),
            MenuItem::ResetZoom => self.essential_shortcut(BrowserShortcut::ZoomReset),
            MenuItem::SavePage => self.command(SessionCommand::SaveActive),
            MenuItem::History => {
                self.toggle_history(super::history_list::ReturnFocus::Page, window, cx);
            }
            MenuItem::NewTab => self.command(SessionCommand::NewTab {
                address: "about:blank".to_owned(),
            }),
            MenuItem::NewFolder => self.start_new_folder(window, cx),
            MenuItem::EditFavorites => {
                let favorites = Some(DestinationSurface::Favorites);
                self.home.editing = if self.home.editing == favorites {
                    None
                } else {
                    favorites
                };
            }
        }
        cx.notify();
    }

    /// The open menu, drawn above everything in the window; a press outside it closes it.
    pub(super) fn menu_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let menu = self.menu.as_ref()?;
        let palette = theme::current();
        let zoom = self
            .shown
            .and_then(|id| self.zoom_levels.get(&id.get()).copied())
            .unwrap_or(1.);
        let mut card = div()
            .id("chrome-menu")
            .role(gpui::Role::Menu)
            .aria_label(match menu.kind {
                MenuKind::Page => "Page menu",
                MenuKind::Sidebar => "Sidebar menu",
            })
            .track_focus(&self.menu_focus)
            .occlude()
            .w(px(MENU_WIDTH))
            .p(px(4.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(theme::rgba(palette.panel_line))
            .bg(theme::rgba(palette.panel))
            .shadow_lg()
            .text_size(px(syrinx_ui::metrics::text::BODY))
            .text_color(theme::text())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.close_menu_to_page(window, cx) {
                    cx.stop_propagation();
                }
            }));
        for (index, item) in menu.kind.items().iter().copied().enumerate() {
            let editing = self.home.editing == Some(DestinationSurface::Favorites);
            let label = match item {
                MenuItem::ResetZoom => format!("{} · {:.0}%", item.label(), zoom * 100.),
                MenuItem::EditFavorites if editing => "Done editing favorites".to_owned(),
                _ => item.label().to_owned(),
            };
            card = card.child(menu_row(index, label, item.shortcut(), &palette).on_click(
                cx.listener(move |this, _, window, cx| this.run_menu_item(item, window, cx)),
            ));
        }
        Some(deferred(
            anchored()
                .anchor(menu.corner)
                .position(menu.position)
                .snap_to_window_with_margin(px(8.))
                .child(card),
        ))
    }
}

/// One item: its name on the left and its shortcut, dimmed, on the right.
fn menu_row(
    index: usize,
    label: String,
    shortcut: Option<String>,
    palette: &syrinx_ui::Palette,
) -> Stateful<Div> {
    let selected = palette.row_selected;
    crate::controls::control(
        div().id(("menu-item", index)),
        gpui::Role::MenuItem,
        label.clone(),
    )
    .h(px(ITEM_HEIGHT))
    .px(px(10.))
    .flex()
    .items_center()
    .gap_3()
    .rounded(px(6.))
    .cursor_pointer()
    .hover(move |style| style.bg(theme::rgba(selected)))
    .child(div().flex_1().child(label))
    .children(shortcut.map(|keys| {
        div()
            .text_size(px(syrinx_ui::metrics::text::SMALL))
            .text_color(theme::rgba(palette.text_muted))
            .child(keys)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_item_shows_its_shortcut() {
        for item in PAGE_ITEMS {
            assert!(item.shortcut().is_some(), "{item:?}");
        }
        assert_eq!(MenuItem::Find.shortcut().as_deref(), Some("Ctrl F"));
    }

    #[test]
    fn the_sidebar_menu_holds_new_tab_new_folder_and_favorites_editing() {
        assert_eq!(MenuKind::Sidebar.items(), &SIDEBAR_ITEMS);
        assert_eq!(MenuItem::NewTab.shortcut().as_deref(), Some("Ctrl T"));
    }
}

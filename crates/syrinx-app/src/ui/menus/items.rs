//! The page menu behind ☰ and the sidebar's ⋯ menu: which items each holds, how they open,
//! and what choosing an item does (#95).

use super::*;

pub(super) const PAGE_ITEMS: [MenuItem; 7] = [
    MenuItem::Find,
    MenuItem::Downloads,
    MenuItem::Print,
    MenuItem::Zoom,
    MenuItem::ResetZoom,
    MenuItem::SavePage,
    MenuItem::History,
];

pub(super) const SIDEBAR_ITEMS: [MenuItem; 5] = [
    MenuItem::NewTab,
    MenuItem::NewFolder,
    MenuItem::EditFavorites,
    MenuItem::Separator,
    MenuItem::CloseActiveTab,
];

/// The ⋯ menu's Close tab closes the tab that was active when the menu opened, never one
/// selected since; with no tab active then, or that tab closed meanwhile, it closes nothing.
pub(super) fn close_opened_on(
    active: Option<TabId>,
    model: &syrinx_core::TabModelSnapshot,
) -> Option<SessionCommand> {
    let tab = active?;
    model
        .tabs
        .iter()
        .any(|open| open.id == tab)
        .then_some(SessionCommand::CloseTab(tab))
}

/// A row with its item's own label and no shortcut.
pub(super) fn fixed_row(item: MenuItem) -> MenuRow {
    MenuRow {
        item,
        label: item.label().to_owned(),
        shortcut: None,
        controls: &[],
        icon: None,
    }
}

impl BrowserView {
    /// Opens `kind` with its top-left corner at the pointer, as a right-click does.
    pub(in crate::ui) fn open_menu_at(
        &mut self,
        kind: MenuKind,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_menu(kind, event.position, Corner::TopLeft, window, cx);
    }

    /// Opens the page menu under the ☰ button in the title bar's right corner, or closes it
    /// when the press is on the button of the menu that is open.
    pub(in crate::ui) fn open_page_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggled_closed(MenuKind::Page, cx) {
            return;
        }
        let position = title_bar_corner(window);
        self.show_menu(MenuKind::Page, position, Corner::TopRight, window, cx);
    }

    /// Opens the sidebar menu under its ⋯ button, or closes it as `open_page_menu` does.
    pub(in crate::ui) fn open_sidebar_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.toggled_closed(MenuKind::Sidebar, cx) {
            return;
        }
        self.show_menu(MenuKind::Sidebar, position, Corner::TopLeft, window, cx);
    }

    /// Opens one of the fixed menus; the keys go back to the page when it closes.
    pub(super) fn show_menu(
        &mut self,
        kind: MenuKind,
        position: Point<Pixels>,
        corner: Corner,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.menu_rows(kind);
        let opening = MenuOpening {
            anchor: MenuAnchor { position, corner },
            returns: MenuReturn::Page,
            select_first: false,
            asked: Vec::new(),
        };
        self.show_menu_rows(kind, rows, opening, window, cx);
    }

    /// The rows of `kind` as they read now, favorites editing included; the Zoom row reads
    /// its level from the open menu (`menu_zoom_changed`).
    pub(super) fn menu_rows(&self, kind: MenuKind) -> Vec<MenuRow> {
        if kind == MenuKind::Downloads {
            return download_rows(&self.downloads, syrinx_platform::browser::PAUSES_DOWNLOADS);
        }
        let editing = self.home.editing == Some(DestinationSurface::Favorites);
        kind.items()
            .iter()
            .map(|&item| MenuRow {
                item,
                label: match item {
                    MenuItem::EditFavorites if editing => "Done editing favorites".to_owned(),
                    _ => item.label().to_owned(),
                },
                shortcut: match item {
                    // The window's own close-tab key, read from the registry that binds it.
                    MenuItem::CloseActiveTab => {
                        browser_key_label(&self.commands, BrowserAction::CloseTab)
                    }
                    _ => item.shortcut(),
                },
                controls: &[],
                icon: None,
            })
            .collect()
    }

    /// Does what the chosen item names; the keys go back to the page unless it places them.
    pub(super) fn run_menu_item(
        &mut self,
        item: MenuItem,
        origin: MenuOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if item.takes_keys() {
            self.move_keys_to_chrome();
        }
        match item {
            MenuItem::Find => self.open_find(window, cx),
            MenuItem::Downloads => self.toggle_downloads(window, cx),
            MenuItem::Print => self.essential_shortcut(BrowserShortcut::Print),
            // The Zoom row acts through its stepper (`MenuAction::Zoom`), never as a choice.
            MenuItem::Zoom => {}
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
            MenuItem::ClearDownloads => self.downloads.clear_finished(),
            MenuItem::CloseActiveTab => {
                if let Some(command) = close_opened_on(origin.active, &self.model) {
                    self.command(command);
                }
            }
            // A row menu's or confirmation's item puts the keys back itself.
            MenuItem::Tab(_) | MenuItem::ConfirmBulk { .. } | MenuItem::Cancel => {
                self.run_row_item(item, origin, window, cx);
                return;
            }
            MenuItem::HistoryPage {
                tab,
                direction,
                steps,
            } => self.jump_to_history_row(tab, direction, steps),
            // Shown for their state only, which `is_choosable` keeps from being chosen.
            MenuItem::Download(_)
            | MenuItem::NoDownloads
            | MenuItem::Separator
            | MenuItem::ConfirmPrompt
            | MenuItem::HistoryUnlisted => {}
        }
        if item.returns_keys_to_page() {
            self.focus_page(window, cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_item_shows_its_shortcut_except_the_zoom_steppers_row() {
        for item in PAGE_ITEMS {
            let expected = item != MenuItem::Zoom;
            assert_eq!(item.shortcut().is_some(), expected, "{item:?}");
        }
        assert_eq!(MenuItem::ResetZoom.shortcut().as_deref(), Some("Ctrl 0"));
        assert_eq!(MenuItem::Find.shortcut().as_deref(), Some("Ctrl F"));
    }

    #[test]
    fn the_sidebar_menu_holds_new_tab_new_folder_and_favorites_editing() {
        assert_eq!(MenuKind::Sidebar.items(), &SIDEBAR_ITEMS);
        assert_eq!(MenuItem::NewTab.shortcut().as_deref(), Some("Ctrl T"));
    }

    #[test]
    fn only_find_new_folder_and_history_keep_the_keys_from_the_page() {
        let taking: Vec<_> = PAGE_ITEMS
            .iter()
            .chain(&SIDEBAR_ITEMS)
            .copied()
            .filter(|item| item.takes_keys())
            .collect();
        assert_eq!(
            taking,
            [MenuItem::Find, MenuItem::History, MenuItem::NewFolder]
        );
    }

    #[test]
    fn new_tab_leaves_the_keys_for_its_palette_not_the_page() {
        assert!(!MenuItem::NewTab.returns_keys_to_page());
        assert!(
            !MenuItem::Downloads.returns_keys_to_page(),
            "the downloads flyout takes them"
        );
        assert!(MenuItem::Print.returns_keys_to_page());
        assert!(!MenuItem::Find.returns_keys_to_page());
    }

    #[test]
    fn the_sidebar_menus_close_tab_closes_the_tab_active_when_it_opened() {
        let mut tabs = syrinx_core::TabModel::new();
        let mut open = || {
            let metadata = syrinx_core::TabMetadata {
                title: "Synthetic".to_owned(),
                address: "https://example.test/".to_owned(),
                favicon: None,
                badge: None,
            };
            tabs.open(
                syrinx_core::TabKind::Web,
                metadata,
                syrinx_core::Activation::Foreground,
            )
            .expect("synthetic tab opens")
        };
        let opened_on = open();
        // Another tab became active after the menu opened; Close tab still closes the first.
        let selected_since = open();
        let model = tabs.snapshot();
        assert_eq!(model.active_id, Some(selected_since));
        assert!(matches!(
            close_opened_on(Some(opened_on), &model),
            Some(SessionCommand::CloseTab(tab)) if tab == opened_on
        ));
        assert!(close_opened_on(None, &model).is_none());
        // A tab that closed while the menu was open is not closed again.
        let gone = syrinx_core::TabId::new(999).expect("synthetic tab id");
        assert!(close_opened_on(Some(gone), &model).is_none());
    }

    #[test]
    fn the_sidebar_menu_ends_with_close_tab_after_a_separator() {
        assert_eq!(
            SIDEBAR_ITEMS[3..],
            [MenuItem::Separator, MenuItem::CloseActiveTab]
        );
        assert!(MenuItem::CloseActiveTab.returns_keys_to_page());
        assert!(!MenuItem::Separator.is_choosable());
    }
}

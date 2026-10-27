//! The chrome's drop-down menus: the page menu behind ☰, the sidebar's ⋯ menu, a sidebar
//! row's menu and its bulk-close confirmation (#95), and the Downloads flyout behind ↓ (#143).
//! A menu is its own owned popup window: GPUI content drawn over the page card in the
//! main window never receives the page's clicks or keys (#12).

use std::time::{Duration, Instant};

use gpui::{KeyDownEvent, MouseDownEvent, Pixels, Point, WeakEntity, prelude::FluentBuilder as _};
use syrinx_core::{BrowserAction, DestinationSurface, DownloadControl, Shortcut};
use syrinx_platform::browser::{BrowserError, OverlayRect, hide_overlay, syrinx_in_foreground};

use super::row_menu::{
    ActionFocus, BulkClose, MenuOrigin, RowEntry, TabAction, browser_key_label, bulk_confirm_label,
    bulk_question,
};
use super::sidebar_tree::TreeId;
use super::title_bar::{CAPTION_GAP, caption_buttons_width};
use super::{
    history_list::ReturnFocus,
    owned_popup::{
        OwnedPopup, OwnedPopupError, PopupSpec, open_owned_popup, round_popup_corners,
        show_and_activate,
    },
    *,
};

/// The menu's width, narrow enough that the sidebar menu stays inside a wide sidebar.
const MENU_WIDTH: f32 = 220.;
/// The Downloads flyout is wider, so a file name, its progress and its buttons fit on one row.
const DOWNLOADS_WIDTH: f32 = 380.;
/// The flyout lists this many downloads, newest first; the session's list keeps more.
const MAX_DOWNLOAD_ROWS: usize = 8;
const ITEM_HEIGHT: f32 = 30.;
/// A separator between a menu's groups: a 1 px line centred in this height.
const SEPARATOR_HEIGHT: f32 = 9.;
/// The card's padding around its rows and its border, in logical pixels.
const MENU_PADDING: f32 = 4.;
const MENU_BORDER: f32 = 1.;
/// The menu keeps this far inside the main window's edges.
const WINDOW_MARGIN: f32 = 8.;
/// A press on the menu's own button this soon after the press closed it is that same
/// press, so it closes the menu rather than opening it again.
const RECLICK_WINDOW: Duration = Duration::from_millis(400);

/// Which menu is open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MenuKind {
    Page,
    Sidebar,
    Downloads,
    /// A sidebar row's actions; its rows come from `row_menu::row_actions`.
    Row,
    /// Asks before Close other tabs or Close tabs below.
    Confirm,
}

/// One menu entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuItem {
    Find,
    Downloads,
    Print,
    /// One row with the − / percentage / + stepper, which zooms without closing the menu.
    Zoom,
    ResetZoom,
    SavePage,
    History,
    NewTab,
    NewFolder,
    EditFavorites,
    /// One download in the Downloads flyout, with its Pause, Resume and Cancel buttons.
    /// Syrinx has no safe platform call yet to open the file or its folder.
    Download(u64),
    /// The flyout's only row when nothing has downloaded this session.
    NoDownloads,
    ClearDownloads,
    /// A line between groups; never selected or chosen.
    Separator,
    /// The ⋯ menu's Close tab, on the active tab.
    CloseActiveTab,
    /// A sidebar row's action (#95).
    Tab(TabAction),
    /// The confirmation's question; shown, never chosen.
    ConfirmPrompt,
    /// Closes the tabs the confirmation named; `remember` stops it asking again.
    ConfirmBulk {
        bulk: BulkClose,
        remember: bool,
    },
    Cancel,
}

const PAGE_ITEMS: [MenuItem; 7] = [
    MenuItem::Find,
    MenuItem::Downloads,
    MenuItem::Print,
    MenuItem::Zoom,
    MenuItem::ResetZoom,
    MenuItem::SavePage,
    MenuItem::History,
];
const SIDEBAR_ITEMS: [MenuItem; 5] = [
    MenuItem::NewTab,
    MenuItem::NewFolder,
    MenuItem::EditFavorites,
    MenuItem::Separator,
    MenuItem::CloseActiveTab,
];

impl MenuKind {
    /// The fixed items; the Downloads flyout's rows come from the session's list instead.
    fn items(self) -> &'static [MenuItem] {
        match self {
            MenuKind::Page => &PAGE_ITEMS,
            MenuKind::Sidebar => &SIDEBAR_ITEMS,
            MenuKind::Downloads | MenuKind::Row | MenuKind::Confirm => &[],
        }
    }

    fn name(self) -> &'static str {
        match self {
            MenuKind::Page => "Page menu",
            MenuKind::Sidebar => "Sidebar menu",
            MenuKind::Downloads => "Downloads",
            MenuKind::Row => "Tab menu",
            MenuKind::Confirm => "Close tabs",
        }
    }

    fn width(self) -> f32 {
        match self {
            MenuKind::Downloads => DOWNLOADS_WIDTH,
            MenuKind::Page | MenuKind::Sidebar | MenuKind::Row | MenuKind::Confirm => MENU_WIDTH,
        }
    }
}

impl MenuItem {
    fn label(self) -> &'static str {
        match self {
            MenuItem::Find => "Find in page",
            MenuItem::Downloads => "Downloads",
            MenuItem::Print => "Print",
            MenuItem::Zoom => "Zoom",
            MenuItem::ResetZoom => "Reset zoom",
            MenuItem::SavePage => "Save page",
            MenuItem::History => "History",
            MenuItem::NewTab => "New tab",
            MenuItem::NewFolder => "New folder",
            MenuItem::EditFavorites => "Edit favorites",
            MenuItem::Download(_) => "Download",
            MenuItem::NoDownloads => "No downloads this session",
            MenuItem::ClearDownloads => "Clear finished downloads",
            MenuItem::Separator => "",
            MenuItem::CloseActiveTab => "Close tab",
            MenuItem::Tab(action) => action.label(),
            MenuItem::ConfirmPrompt => "Close tabs?",
            MenuItem::ConfirmBulk {
                remember: false, ..
            } => "Close tabs",
            MenuItem::ConfirmBulk { remember: true, .. } => "Close and don't ask again",
            MenuItem::Cancel => "Cancel",
        }
    }

    /// The item's shortcut as the command registry spells it. The Zoom row shows its
    /// stepper instead; its buttons name Ctrl − and Ctrl +, which `essentials` binds.
    fn shortcut(self) -> Option<String> {
        let keys = match self {
            MenuItem::Find => "ctrl-f",
            MenuItem::Downloads => "ctrl-j",
            MenuItem::Print => "ctrl-p",
            MenuItem::Zoom => return None,
            MenuItem::ResetZoom => "ctrl-0",
            MenuItem::SavePage => "ctrl-d",
            MenuItem::History => "ctrl-h",
            MenuItem::NewTab => "ctrl-t",
            // The ⋯ menu's Close tab and the row items read the registry (`menu_rows`).
            MenuItem::NewFolder
            | MenuItem::EditFavorites
            | MenuItem::Download(_)
            | MenuItem::NoDownloads
            | MenuItem::ClearDownloads
            | MenuItem::Separator
            | MenuItem::CloseActiveTab
            | MenuItem::Tab(_)
            | MenuItem::ConfirmPrompt
            | MenuItem::ConfirmBulk { .. }
            | MenuItem::Cancel => return None,
        };
        Shortcut::new(keys).ok().map(|shortcut| shortcut.label())
    }

    /// Whether the item puts the keys in a chrome field or list (History places them itself);
    /// every other item returns them to the page, where they were before the menu opened.
    fn takes_keys(self) -> bool {
        matches!(
            self,
            MenuItem::Find | MenuItem::NewFolder | MenuItem::History
        )
    }

    /// Whether the keys go back to the page after the item runs. New tab gives them to its
    /// Home palette instead: focusing the outgoing page would take them from it (#14).
    /// Downloads gives them to its flyout.
    fn returns_keys_to_page(self) -> bool {
        !self.takes_keys() && !matches!(self, MenuItem::NewTab | MenuItem::Downloads)
    }

    /// Whether Enter, Space or a click runs the item; a download row only shows its state.
    fn is_choosable(self) -> bool {
        self.is_selectable()
            && !matches!(
                self,
                MenuItem::Zoom | MenuItem::Download(_) | MenuItem::NoDownloads
            )
    }

    /// Whether arrows and the pointer can select the row; separators and the confirmation's
    /// question are drawn only.
    fn is_selectable(self) -> bool {
        !matches!(self, MenuItem::Separator | MenuItem::ConfirmPrompt)
    }
}

/// One row as the popup draws it, captured when the menu opens.
#[derive(Clone, Debug)]
struct MenuRow {
    item: MenuItem,
    label: String,
    shortcut: Option<String>,
    /// A download row's buttons; empty for every other row.
    controls: &'static [DownloadControl],
}

/// The open menu: its popup once the window exists, `None` while it is opening.
pub(super) struct OpenMenu {
    kind: MenuKind,
    generation: u64,
    popup: Option<OwnedPopup>,
    view: Option<WeakEntity<MenuView>>,
    /// Where it hangs, so a confirmation can open in its place.
    anchor: MenuAnchor,
    returns: MenuReturn,
}

/// Where the keys go when a menu closes by Escape or after an item that does not place them.
#[derive(Clone)]
pub(super) enum MenuReturn {
    /// The page, as for the title bar's menus.
    Page,
    /// The sidebar row a key opened the menu from.
    Row(TreeId),
    /// Where they were when a click opened the menu: a chrome control, or the page.
    Was(ReturnFocus),
}

/// The menu a press on the main window closed, and when, so that press does not reopen it.
#[derive(Clone, Copy)]
pub(super) struct MenuDismissal {
    kind: MenuKind,
    at: Instant,
}

/// What the popup asks of the browser; it is applied in the main window.
#[derive(Clone, Copy, Debug)]
enum MenuAction {
    Choose(MenuItem),
    /// A step on the Zoom row's stepper; the menu stays open, as in Chrome and Edge.
    Zoom(BrowserShortcut),
    /// Escape or the window's close request: the keys go back to the page.
    Dismiss,
    /// The owner clicked elsewhere or switched away; focus stays where that put it.
    Deactivated,
    /// A download row's Pause, Resume or Cancel; the flyout stays open to show the result.
    Download(u64, DownloadControl),
}

/// Where a requested menu opens.
struct MenuRequest {
    generation: u64,
    kind: MenuKind,
    rows: Vec<MenuRow>,
    zoom: f64,
    rect: OverlayRect,
    parent: isize,
    /// A menu opened by a key starts with its first item selected, as Windows menus do.
    select_first: bool,
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
        self.show_menu(kind, event.position, Corner::TopLeft, window, cx);
    }

    /// Opens the page menu under the ☰ button in the title bar's right corner, or closes it
    /// when the press is on the button of the menu that is open.
    pub(super) fn open_page_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggled_closed(MenuKind::Page, cx) {
            return;
        }
        let position = title_bar_corner(window);
        self.show_menu(MenuKind::Page, position, Corner::TopRight, window, cx);
    }

    /// Opens the Downloads flyout under the title bar's ↓ corner, or closes it as
    /// `open_page_menu` does. It works from any tab, Home included.
    pub(super) fn toggle_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggled_closed(MenuKind::Downloads, cx) {
            return;
        }
        self.show_downloads(window, cx);
    }

    /// Opens the Downloads flyout at its corner, replacing any open menu.
    fn show_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let position = title_bar_corner(window);
        self.show_menu(MenuKind::Downloads, position, Corner::TopRight, window, cx);
    }

    /// The session's downloads changed: an open flyout shows the new rows, and reopens in
    /// place when their number changed, since its window is sized to its rows.
    pub(super) fn menu_downloads_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &self.menu else {
            return;
        };
        if menu.kind != MenuKind::Downloads {
            return;
        }
        let Some(view) = menu.view.clone() else {
            return; // Still opening; its rows were read when it was requested.
        };
        let rows = self.menu_rows(MenuKind::Downloads);
        let reopen = view.update(cx, |menu, cx| {
            if menu.rows.len() != rows.len() {
                return true;
            }
            menu.rows = rows;
            cx.notify();
            false
        });
        match reopen {
            Ok(false) => {}
            // Opening afresh gives the window its new size.
            Ok(true) => self.show_downloads(window, cx),
            Err(_) => eprintln!("The downloads flyout closed before its rows changed."),
        }
    }

    /// Opens the sidebar menu under its ⋯ button, or closes it as `open_page_menu` does.
    pub(super) fn open_sidebar_menu(
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

    /// A press on an open menu's button closes it. Depending on when Windows reports the
    /// popup's deactivation, the menu is either still open or was closed by this press.
    fn toggled_closed(&mut self, kind: MenuKind, cx: &mut Context<Self>) -> bool {
        let open = self.menu.as_ref().map(|menu| menu.kind);
        let dismissed = self.menu_dismissal.take();
        if !closes_on_reclick(open, dismissed, kind, Instant::now()) {
            return false;
        }
        self.retire_menu(cx);
        cx.notify();
        true
    }

    /// Opens one of the fixed menus; the keys go back to the page when it closes.
    fn show_menu(
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
        };
        self.show_menu_rows(kind, rows, opening, window, cx);
    }

    /// Opens a sidebar row's menu, or the ⋯ menu when the row offers nothing here (a folder
    /// in the favicon-only sidebar). A row gone from the model opens nothing.
    pub(super) fn show_row_menu(
        &mut self,
        row: TreeId,
        opening: MenuOpening,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let wide = self.sidebar_sizing.labels_visible();
        let rows = row_menu_rows(self.row_entries(row, wide));
        if rows.is_empty() {
            let rows = self.menu_rows(MenuKind::Sidebar);
            self.show_menu_rows(MenuKind::Sidebar, rows, opening, window, cx);
            return;
        }
        self.show_menu_rows(MenuKind::Row, rows, opening, window, cx);
    }

    /// Asks before a bulk close, in the place of the menu it was chosen from.
    pub(super) fn show_bulk_confirmation(
        &mut self,
        bulk: BulkClose,
        count: usize,
        origin: MenuOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A question reached by keys starts on its first choice, as the row menu did.
        let select_first = matches!(origin.returns, MenuReturn::Row(_));
        let opening = MenuOpening {
            anchor: origin.anchor,
            returns: origin.returns,
            select_first,
        };
        let rows = confirm_rows(bulk, count);
        self.show_menu_rows(MenuKind::Confirm, rows, opening, window, cx);
    }

    /// Records the menu and opens its popup once this update ends (see `open_requested_menu`).
    fn show_menu_rows(
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
        let MenuOpening {
            anchor,
            returns,
            select_first,
        } = opening;
        let size = kind_size(kind, &rows);
        let rect = menu_rect(window.bounds(), window.scale_factor(), anchor, size);
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

    /// The rows of `kind` as they read now, favorites editing included; the Zoom row reads
    /// its level from the open menu (`menu_zoom_changed`).
    fn menu_rows(&self, kind: MenuKind) -> Vec<MenuRow> {
        if kind == MenuKind::Downloads {
            return download_rows(&self.downloads);
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
            })
            .collect()
    }

    /// Whether a popup opened for `generation` is still wanted.
    fn awaits_menu(&self, generation: u64) -> bool {
        !self.closing
            && self
                .menu
                .as_ref()
                .is_some_and(|menu| menu.generation == generation && menu.popup.is_none())
    }

    /// Adopts the opened popup and shows it, or closes it when the menu was closed meanwhile.
    fn install_menu(
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
    fn show_menu_popup(
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
    fn menu_shown(
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
    fn menu_activated(&self, cx: &mut Context<Self>) {
        let Some(view) = self.menu.as_ref().and_then(|menu| menu.view.clone()) else {
            return;
        };
        if view.update(cx, |menu, _| menu.was_active = true).is_err() {
            eprintln!("The menu popup closed before it took the keys.");
        }
    }

    /// The page in view reported a new zoom level; an open page menu's Zoom row shows it.
    pub(super) fn menu_zoom_changed(&self, factor: f64, cx: &mut Context<Self>) {
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

    fn is_current_menu(&self, generation: u64) -> bool {
        self.menu
            .as_ref()
            .is_some_and(|menu| menu.generation == generation)
    }

    /// Popup input is applied in the main window, after the popup's own update ends.
    fn queue_menu_action(&self, generation: u64, action: MenuAction, cx: &mut Context<Self>) {
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

    fn apply_menu_action(
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
        };
        self.retire_menu(cx);
        match action {
            MenuAction::Deactivated => {
                self.menu_dismissal = Some(MenuDismissal {
                    kind,
                    at: Instant::now(),
                });
            }
            MenuAction::Dismiss if in_front => self.return_menu_focus(origin.returns, window, cx),
            MenuAction::Choose(item) if in_front => self.run_menu_item(item, origin, window, cx),
            MenuAction::Dismiss
            | MenuAction::Choose(_)
            | MenuAction::Zoom(_)
            | MenuAction::Download(..) => {}
        }
        cx.notify();
    }

    /// Closes the open menu, if any. An active popup hands activation to the main window
    /// first, so Windows does not pick the next window itself.
    pub(super) fn retire_menu(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        if let Some(popup) = menu.popup {
            remove_menu_window(popup, self.parent_hwnd, cx);
        }
    }

    /// Gives the keys back after a menu closes: to the page, the row a key opened it from, or
    /// the control a click found them on. A row or control gone since falls back to the page.
    fn return_menu_focus(
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
                let moved = self.move_keys_to_chrome();
                if moved {
                    window.focus(&handle, cx);
                }
                moved
            }
        };
        if !returned {
            self.focus_page(window, cx);
        }
    }

    /// Runs a row menu's or confirmation's item; the keys go back where the menu found them
    /// unless the item placed them.
    fn run_row_item(
        &mut self,
        item: MenuItem,
        origin: MenuOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let returns = origin.returns.clone();
        let focus = match item {
            MenuItem::Tab(action) => self.run_tab_action(action, Some(origin), window, cx),
            MenuItem::ConfirmBulk { bulk, remember } => {
                self.confirm_bulk_close(bulk, remember);
                ActionFocus::Unplaced
            }
            _ => ActionFocus::Unplaced,
        };
        if focus == ActionFocus::Unplaced {
            self.return_menu_focus(returns, window, cx);
        }
        cx.notify();
    }

    /// Does what the chosen item names; the keys go back to the page unless it places them.
    fn run_menu_item(
        &mut self,
        item: MenuItem,
        origin: MenuOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            item,
            MenuItem::Tab(_) | MenuItem::ConfirmBulk { .. } | MenuItem::Cancel
        ) {
            self.run_row_item(item, origin, window, cx);
            return;
        }
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
            MenuItem::CloseActiveTab => self.close_tab(),
            // Shown for their state only, which `is_choosable` keeps from being chosen; row
            // items ran above.
            MenuItem::Download(_)
            | MenuItem::NoDownloads
            | MenuItem::Separator
            | MenuItem::ConfirmPrompt
            | MenuItem::Tab(_)
            | MenuItem::ConfirmBulk { .. }
            | MenuItem::Cancel => {}
        }
        if item.returns_keys_to_page() {
            self.focus_page(window, cx);
        }
        cx.notify();
    }
}

/// Opens a requested menu outside every entity update, then hands the popup to the browser.
fn open_requested_menu(browser: WeakEntity<BrowserView>, request: MenuRequest, cx: &mut App) {
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
fn remove_menu_window(popup: OwnedPopup, owner: isize, cx: &mut App) {
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

/// What a menu shows when it opens.
struct MenuContent {
    kind: MenuKind,
    rows: Vec<MenuRow>,
    /// The page's zoom level, which the Zoom row shows and the browser keeps current.
    zoom: f64,
    selected: Option<usize>,
}

/// The popup's content: the rows, the keyboard selection, and the keys and clicks on them.
struct MenuView {
    browser: WeakEntity<BrowserView>,
    generation: u64,
    kind: MenuKind,
    rows: Vec<MenuRow>,
    zoom: f64,
    /// The row arrows have moved to; none until the owner presses one or hovers a row.
    selected: Option<usize>,
    focus: FocusHandle,
    /// Deactivation before the popup was ever active is its hidden creation, not a click away.
    was_active: bool,
    _activation: Subscription,
}

impl MenuView {
    fn new(
        browser: WeakEntity<BrowserView>,
        generation: u64,
        menu: MenuContent,
        popup: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let MenuContent {
            kind,
            rows,
            zoom,
            selected,
        } = menu;
        let activation = cx.observe_window_activation(popup, |this: &mut Self, popup, cx| {
            if popup.is_window_active() {
                this.was_active = true;
                popup.focus(&this.focus, cx);
            } else if this.was_active {
                this.send(MenuAction::Deactivated, cx);
            }
        });
        Self {
            browser,
            generation,
            kind,
            rows,
            zoom,
            selected,
            focus: cx.focus_handle(),
            was_active: false,
            _activation: activation,
        }
    }

    fn send(&self, action: MenuAction, cx: &mut Context<Self>) {
        let generation = self.generation;
        let queued = self.browser.update(cx, |browser, cx| {
            browser.queue_menu_action(generation, action, cx);
        });
        if queued.is_err() {
            eprintln!("Menu input arrived after its browser was removed.");
        }
    }

    /// Arrows, Home and End move the selection; Enter and Space choose it; Escape closes.
    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if let Some(step) = MenuStep::for_key(key) {
            self.selected = step_selection(step, self.selected, &self.rows);
            cx.notify();
            return;
        }
        let selected = self.selected.and_then(|row| self.rows.get(row));
        let action = selected
            .and_then(|row| download_key(key, row))
            .or_else(|| key_action(key, selected.map(|row| row.item)));
        let Some(action) = action else {
            return;
        };
        cx.stop_propagation();
        self.send(action, cx);
    }
}

/// On a download row, Enter or Space presses its first button (Pause or Resume) and Delete
/// cancels it.
fn download_key(key: &str, row: &MenuRow) -> Option<MenuAction> {
    let MenuItem::Download(download) = row.item else {
        return None;
    };
    let control = match key {
        "enter" | "space" => row.controls.first().copied(),
        "delete" => row
            .controls
            .contains(&DownloadControl::Cancel)
            .then_some(DownloadControl::Cancel),
        _ => None,
    }?;
    Some(MenuAction::Download(download, control))
}

/// What a key other than the selection moves does on the selected row, if anything.
fn key_action(key: &str, selected: Option<MenuItem>) -> Option<MenuAction> {
    match (key, selected) {
        ("escape", _) => Some(MenuAction::Dismiss),
        // On the Zoom row, Left and Right step like its − and + buttons.
        ("left", Some(MenuItem::Zoom)) => Some(MenuAction::Zoom(BrowserShortcut::ZoomOut)),
        ("right", Some(MenuItem::Zoom)) => Some(MenuAction::Zoom(BrowserShortcut::ZoomIn)),
        ("enter" | "space", Some(item)) if item.is_choosable() => Some(MenuAction::Choose(item)),
        _ => None,
    }
}

impl MenuView {
    /// A download row's Pause, Resume and Cancel buttons.
    fn download_buttons(
        &self,
        index: usize,
        row: &MenuRow,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let MenuItem::Download(download) = row.item else {
            return Vec::new();
        };
        row.controls
            .iter()
            .map(|&control| {
                let (id, glyph, verb) = control_face(control);
                crate::controls::push_button(div().id((id, index)), format!("{verb} {}", row.label))
                    .h(px(22.))
                    .w(px(22.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(11.))
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::line()))
                    .font_family(crate::controls::glyph::FONT)
                    .text_size(px(10.))
                    .child(glyph)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.send(MenuAction::Download(download, control), cx);
                    }))
            })
            .collect()
    }

    /// The Zoom row's − / percentage / + stepper; each press zooms the page in view.
    fn zoom_stepper(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let menu = cx.weak_entity();
        let label = super::essentials::zoom_percent(self.zoom);
        super::essentials::zoom_stepper("menu-zoom", label, move |shortcut, _, cx| {
            let sent = menu.update(cx, |menu, cx| menu.send(MenuAction::Zoom(shortcut), cx));
            if sent.is_err() {
                eprintln!("A zoom click arrived after its menu was removed.");
            }
        })
    }
}

impl Render for MenuView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::current();
        let mut card = div()
            .id("chrome-menu")
            .role(gpui::Role::Menu)
            .aria_label(self.kind.name())
            .track_focus(&self.focus)
            .size_full()
            .p(px(MENU_PADDING))
            .flex()
            .flex_col()
            .border_1()
            .border_color(theme::rgba(palette.panel_line))
            .bg(theme::panel())
            .font_family("Segoe UI Variable Text")
            .text_size(px(syrinx_ui::metrics::text::BODY))
            .text_color(theme::text())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.key_down(event, cx);
            }));
        for (index, row) in self.rows.iter().enumerate() {
            let item = row.item;
            if !item.is_selectable() {
                card = card.child(drawn_only_row(index, row, &palette));
                continue;
            }
            let selected = self.selected == Some(index);
            let drawn = menu_row(index, row, selected, &palette).on_hover(cx.listener(
                move |this, hovered: &bool, _, cx| {
                    if *hovered && this.selected != Some(index) {
                        this.selected = Some(index);
                        cx.notify();
                    }
                },
            ));
            card = card.child(if item == MenuItem::Zoom {
                // Screen readers hear the level with the row, such as "Zoom, 125%".
                let name = format!(
                    "{}, {}",
                    row.label,
                    super::essentials::zoom_percent(self.zoom)
                );
                drawn.aria_label(name).child(self.zoom_stepper(cx))
            } else if !item.is_choosable() {
                // A download is heard with its state, such as "synthetic.zip, Done, 1.0 MB".
                let name = match &row.shortcut {
                    Some(detail) => format!("{}, {detail}", row.label),
                    None => row.label.clone(),
                };
                drawn
                    .aria_label(name)
                    .children(self.download_buttons(index, row, cx))
            } else {
                drawn.on_click(cx.listener(move |this, _, _, cx| {
                    this.send(MenuAction::Choose(item), cx);
                }))
            });
        }
        crate::controls::watch_pointer(card)
    }
}

/// A separator, or the confirmation's question in the menu's own type.
fn drawn_only_row(index: usize, row: &MenuRow, palette: &syrinx_ui::Palette) -> Stateful<Div> {
    if row.item == MenuItem::Separator {
        return div()
            .id(("menu-separator", index))
            .h(px(SEPARATOR_HEIGHT))
            .flex_shrink_0()
            .px(px(6.))
            .flex()
            .items_center()
            .child(
                div()
                    .h(px(1.))
                    .w_full()
                    .bg(theme::rgba(palette.panel_divider)),
            );
    }
    // A named label, so a screen reader reads the question with the menu.
    div()
        .id(("menu-prompt", index))
        .role(gpui::Role::Label)
        .aria_label(row.label.clone())
        .h(px(ITEM_HEIGHT))
        .flex_shrink_0()
        .px(px(10.))
        .flex()
        .items_center()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
}

/// One item: its name on the left and its shortcut, dimmed, on the right. Rows take no Tab
/// stop; the selected one is the focused menu's active descendant for screen readers.
fn menu_row(
    index: usize,
    row: &MenuRow,
    selected: bool,
    palette: &syrinx_ui::Palette,
) -> Stateful<Div> {
    div()
        .id(("menu-item", index))
        .role(gpui::Role::MenuItem)
        .aria_label(row.label.clone())
        .aria_selected(selected)
        .when(selected, |row| {
            let row = row
                .aria_active_descendant()
                .bg(theme::rgba(palette.row_selected));
            crate::controls::raised(row, syrinx_ui::ShadowRole::RowSelected)
        })
        .h(px(ITEM_HEIGHT))
        .flex_shrink_0()
        .px(px(10.))
        .flex()
        .items_center()
        .gap_3()
        .rounded(px(6.))
        .cursor_pointer()
        .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
        .children(row.shortcut.clone().map(|keys| {
            div()
                .flex_shrink_0()
                .text_size(px(syrinx_ui::metrics::text::SMALL))
                .text_color(theme::rgba(palette.text_muted))
                .child(keys)
        }))
}

/// Which corner of the menu sits at its anchor point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Corner {
    TopLeft,
    TopRight,
}

/// Where the menu is anchored in the main window, in logical pixels.
#[derive(Clone, Copy, Debug)]
pub(super) struct MenuAnchor {
    pub(super) position: Point<Pixels>,
    pub(super) corner: Corner,
}

/// How a menu opens: where, where the keys go when it closes, and whether a key opened it.
pub(super) struct MenuOpening {
    pub(super) anchor: MenuAnchor,
    pub(super) returns: MenuReturn,
    pub(super) select_first: bool,
}

/// The selection after an arrow, Home or End, skipping rows that cannot be selected; `None`
/// when the menu has no selectable row.
fn step_selection(step: MenuStep, selected: Option<usize>, rows: &[MenuRow]) -> Option<usize> {
    // From First or Last, later tries move inwards; Next and Previous keep their direction.
    let onward = match step {
        MenuStep::First => MenuStep::Next,
        MenuStep::Last => MenuStep::Previous,
        other => other,
    };
    let mut candidate = step.apply(selected, rows.len());
    for _ in 0..rows.len() {
        if rows
            .get(candidate)
            .is_some_and(|row| row.item.is_selectable())
        {
            return Some(candidate);
        }
        candidate = onward.apply(Some(candidate), rows.len());
    }
    None
}

fn first_selectable(rows: &[MenuRow]) -> Option<usize> {
    rows.iter().position(|row| row.item.is_selectable())
}

/// A row's menu entries as menu rows.
fn row_menu_rows(entries: Vec<RowEntry>) -> Vec<MenuRow> {
    entries
        .into_iter()
        .map(|entry| match entry {
            RowEntry::Action { action, keys } => MenuRow {
                item: MenuItem::Tab(action),
                label: action.label().to_owned(),
                shortcut: keys,
                controls: &[],
            },
            RowEntry::Separator => fixed_row(MenuItem::Separator),
        })
        .collect()
}

/// The confirmation: its question, then Close, Close and don't ask again, and Cancel.
fn confirm_rows(bulk: BulkClose, count: usize) -> Vec<MenuRow> {
    let labelled = |item: MenuItem, label: String| MenuRow {
        item,
        label,
        shortcut: None,
        controls: &[],
    };
    vec![
        labelled(MenuItem::ConfirmPrompt, bulk_question(bulk.kind, count)),
        fixed_row(MenuItem::Separator),
        labelled(
            MenuItem::ConfirmBulk {
                bulk,
                remember: false,
            },
            bulk_confirm_label(count),
        ),
        fixed_row(MenuItem::ConfirmBulk {
            bulk,
            remember: true,
        }),
        fixed_row(MenuItem::Cancel),
    ]
}

/// A row with its item's own label and no shortcut.
fn fixed_row(item: MenuItem) -> MenuRow {
    MenuRow {
        item,
        label: item.label().to_owned(),
        shortcut: None,
        controls: &[],
    }
}

/// A keyboard move of the selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuStep {
    Next,
    Previous,
    First,
    Last,
}

impl MenuStep {
    fn for_key(key: &str) -> Option<Self> {
        match key {
            "down" => Some(Self::Next),
            "up" => Some(Self::Previous),
            "home" => Some(Self::First),
            "end" => Some(Self::Last),
            _ => None,
        }
    }

    /// The row selected after this step; Down from nothing selects the first row, Up the
    /// last, and both wrap around the ends as Windows menus do.
    fn apply(self, selected: Option<usize>, rows: usize) -> usize {
        let last = rows.saturating_sub(1);
        match (self, selected) {
            (Self::First, _) | (Self::Next, None) => 0,
            (Self::Last, _) | (Self::Previous, None) => last,
            (Self::Next, Some(row)) if row >= last => 0,
            (Self::Next, Some(row)) => row + 1,
            (Self::Previous, Some(0)) => last,
            (Self::Previous, Some(row)) => row - 1,
        }
    }
}

/// Whether a press on `kind`'s button closes its menu instead of opening it: the menu is
/// still open, or the same press closed it a moment ago by deactivating its popup.
fn closes_on_reclick(
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

/// A download button's element id, glyph and spoken verb.
fn control_face(control: DownloadControl) -> (&'static str, &'static str, &'static str) {
    match control {
        DownloadControl::Pause => ("download-pause", "\u{E769}", "Pause"),
        DownloadControl::Resume => ("download-resume", "\u{E768}", "Resume"),
        DownloadControl::Cancel => ("download-cancel", "\u{E711}", "Cancel"),
    }
}

/// The Downloads flyout's rows: the newest downloads, or a line saying there are none, then
/// Clear when any has finished.
fn download_rows(list: &syrinx_core::DownloadList) -> Vec<MenuRow> {
    let fixed = fixed_row;
    let mut rows: Vec<MenuRow> = list
        .downloads()
        .iter()
        .take(MAX_DOWNLOAD_ROWS)
        .map(|download| MenuRow {
            item: MenuItem::Download(download.id),
            label: download.file_name.clone(),
            shortcut: Some(download.detail()),
            controls: download.controls(),
        })
        .collect();
    if rows.is_empty() {
        rows.push(fixed(MenuItem::NoDownloads));
    }
    if list.has_finished() {
        rows.push(fixed(MenuItem::ClearDownloads));
    }
    rows
}

/// Where the ☰ menu and the Downloads flyout hang from: the right edge of the title bar's own
/// controls, left of the caption buttons and the gap before them. Ctrl J and the palette
/// open the flyout with no button press to measure, so the corner follows the title bar's
/// layout constants rather than a button's painted bounds.
fn title_bar_corner(window: &Window) -> Point<Pixels> {
    corner_for_width(f32::from(window.viewport_size().width))
}

/// The corner for a window `width` logical pixels wide.
fn corner_for_width(width: f32) -> Point<Pixels> {
    point(
        px(width - caption_buttons_width() - CAPTION_GAP),
        px(TITLE_BAR_HEIGHT),
    )
}

/// The popup's logical size for `rows` of `kind`, separators counted at their own height.
fn kind_size(kind: MenuKind, rows: &[MenuRow]) -> gpui::Size<Pixels> {
    let separators = rows
        .iter()
        .filter(|row| row.item == MenuItem::Separator)
        .count();
    let items = rows.len() - separators;
    size(px(kind.width()), menu_size(items, separators).height)
}

/// The popup's logical size for `items` rows and `separators` lines between groups.
fn menu_size(items: usize, separators: usize) -> gpui::Size<Pixels> {
    let height = items as f32 * ITEM_HEIGHT
        + separators as f32 * SEPARATOR_HEIGHT
        + 2. * (MENU_PADDING + MENU_BORDER);
    size(px(MENU_WIDTH), px(height))
}

/// The menu's place on screen in physical pixels, kept inside the main window's margin.
fn menu_rect(
    main: Bounds<Pixels>,
    scale: f32,
    anchor: MenuAnchor,
    menu: gpui::Size<Pixels>,
) -> OverlayRect {
    let (width, height) = (f32::from(menu.width), f32::from(menu.height));
    let (main_width, main_height) = (f32::from(main.size.width), f32::from(main.size.height));
    let x = f32::from(anchor.position.x);
    let left = match anchor.corner {
        Corner::TopLeft => x,
        Corner::TopRight => x - width,
    };
    let left = keep_inside(left, width, main_width);
    let top = keep_inside(f32::from(anchor.position.y), height, main_height);
    let physical = |logical: f32| (logical * scale).round() as i32;
    OverlayRect {
        left: physical(f32::from(main.origin.x) + left),
        top: physical(f32::from(main.origin.y) + top),
        width: physical(width),
        height: physical(height),
    }
}

/// Clamps a span's start so it stays `WINDOW_MARGIN` inside `extent`, or at the margin when
/// it cannot fit.
fn keep_inside(start: f32, span: f32, extent: f32) -> f32 {
    let latest = (extent - WINDOW_MARGIN - span).max(WINDOW_MARGIN);
    start.clamp(WINDOW_MARGIN, latest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main_window(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn anchor(x: f32, y: f32, corner: Corner) -> MenuAnchor {
        MenuAnchor {
            position: point(px(x), px(y)),
            corner,
        }
    }

    #[test]
    fn the_corner_sits_left_of_the_caption_buttons_and_their_gap_at_any_width() {
        for width in [800., 1280., 1920.] {
            let corner = corner_for_width(width);
            assert_eq!(
                f32::from(corner.x) + CAPTION_GAP + caption_buttons_width(),
                width,
                "{width}"
            );
            assert_eq!(f32::from(corner.y), TITLE_BAR_HEIGHT);
        }
        // Today's layout: three 36 px buttons and a 4 px gap.
        assert_eq!(f32::from(corner_for_width(1280.).x), 1168.);
    }

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
    fn left_and_right_step_the_zoom_row_and_nothing_else() {
        let zoom = Some(MenuItem::Zoom);
        assert!(matches!(
            key_action("left", zoom),
            Some(MenuAction::Zoom(BrowserShortcut::ZoomOut))
        ));
        assert!(matches!(
            key_action("right", zoom),
            Some(MenuAction::Zoom(BrowserShortcut::ZoomIn))
        ));
        assert!(
            key_action("enter", zoom).is_none(),
            "Enter does not choose the row"
        );
        assert!(key_action("left", Some(MenuItem::Find)).is_none());
        assert!(key_action("right", None).is_none());
        assert!(matches!(
            key_action("enter", Some(MenuItem::Find)),
            Some(MenuAction::Choose(MenuItem::Find))
        ));
        assert!(matches!(
            key_action("escape", zoom),
            Some(MenuAction::Dismiss)
        ));
    }

    #[test]
    fn download_rows_carry_their_controls_and_keys_press_them() {
        let mut list = syrinx_core::DownloadList::default();
        list.add(3, 1, "synthetic.zip", None);
        let rows = download_rows(&list);
        assert_eq!(rows.len(), 1, "no Clear row while nothing has finished");
        let row = &rows[0];
        assert_eq!(
            row.controls,
            [DownloadControl::Pause, DownloadControl::Cancel]
        );
        assert!(matches!(
            download_key("enter", row),
            Some(MenuAction::Download(3, DownloadControl::Pause))
        ));
        assert!(matches!(
            download_key("delete", row),
            Some(MenuAction::Download(3, DownloadControl::Cancel))
        ));
        assert!(download_key("left", row).is_none());
        list.fail(
            3,
            syrinx_core::DownloadInterruption::Blocked(syrinx_core::BlockReason::Unsafe),
            0,
        );
        let rows = download_rows(&list);
        assert!(rows[0].controls.is_empty(), "a blocked file is never kept");
        assert!(download_key("enter", &rows[0]).is_none());
        assert_eq!(rows[1].item, MenuItem::ClearDownloads);
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
    fn the_popup_fits_its_rows_exactly() {
        assert_eq!(menu_size(PAGE_ITEMS.len(), 0), size(px(220.), px(220.)));
        assert_eq!(menu_size(8, 0), size(px(220.), px(250.)));
        assert_eq!(menu_size(3, 0), size(px(220.), px(100.)));
    }

    #[test]
    fn separators_count_at_their_own_height() {
        // The ⋯ menu: four items and one separator.
        let rows: Vec<MenuRow> = SIDEBAR_ITEMS.iter().map(|&item| fixed_row(item)).collect();
        assert_eq!(
            kind_size(MenuKind::Sidebar, &rows),
            size(px(220.), px(4. * 30. + 9. + 10.))
        );
        assert_eq!(
            menu_size(9, 3),
            size(px(220.), px(9. * 30. + 3. * 9. + 10.))
        );
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

    /// A row menu's shape: an item, a separator, two items, a separator and an item.
    fn grouped_rows() -> Vec<MenuRow> {
        let tab = syrinx_core::TabId::new(5).expect("synthetic tab id");
        row_menu_rows(vec![
            RowEntry::Action {
                action: TabAction::Reload(tab),
                keys: None,
            },
            RowEntry::Separator,
            RowEntry::Action {
                action: TabAction::Close(tab),
                keys: Some("Ctrl W, Delete".to_owned()),
            },
            RowEntry::Action {
                action: TabAction::Duplicate(tab),
                keys: None,
            },
            RowEntry::Separator,
            RowEntry::Action {
                action: TabAction::ReopenClosed,
                keys: None,
            },
        ])
    }

    #[test]
    fn arrows_skip_separators_in_both_directions_and_wrap() {
        let rows = grouped_rows();
        let step = |step, from| step_selection(step, from, &rows);
        assert_eq!(step(MenuStep::Next, None), Some(0));
        assert_eq!(step(MenuStep::Next, Some(0)), Some(2));
        assert_eq!(step(MenuStep::Previous, Some(2)), Some(0));
        assert_eq!(step(MenuStep::Next, Some(3)), Some(5));
        assert_eq!(step(MenuStep::Next, Some(5)), Some(0));
        assert_eq!(step(MenuStep::Previous, Some(0)), Some(5));
        assert_eq!(step(MenuStep::Last, Some(2)), Some(5));
        assert_eq!(first_selectable(&rows), Some(0));
        assert_eq!(rows[2].shortcut.as_deref(), Some("Ctrl W, Delete"));
    }

    #[test]
    fn the_confirmation_names_the_count_and_its_question_cannot_be_chosen() {
        let bulk = BulkClose {
            kind: super::super::row_menu::BulkKind::Others,
            tab: syrinx_core::TabId::new(5).expect("synthetic tab id"),
        };
        let rows = confirm_rows(bulk, 3);
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Close 3 other tabs?",
                "",
                "Close 3 tabs",
                "Close and don't ask again",
                "Cancel"
            ]
        );
        assert!(!rows[0].item.is_selectable());
        // Arrows start on Close, never on the question.
        assert_eq!(step_selection(MenuStep::Next, None, &rows), Some(2));
        assert_eq!(step_selection(MenuStep::First, None, &rows), Some(2));
        assert!(matches!(
            key_action("enter", Some(rows[3].item)),
            Some(MenuAction::Choose(MenuItem::ConfirmBulk {
                remember: true,
                ..
            }))
        ));
        assert!(key_action("enter", Some(rows[0].item)).is_none());
    }

    #[test]
    fn the_page_menu_hangs_from_its_right_corner_in_screen_pixels() {
        // A 1280-wide window at (100, 50); the ☰ corner sits at x 1176, under the title bar.
        let rect = menu_rect(
            main_window(100., 50., 1280., 800.),
            1.,
            anchor(1176., 36., Corner::TopRight),
            menu_size(8, 0),
        );
        assert_eq!(
            rect,
            OverlayRect {
                left: 1056,
                top: 86,
                width: 220,
                height: 250
            }
        );
    }

    #[test]
    fn a_right_click_near_an_edge_keeps_the_menu_inside_the_window() {
        let main = main_window(0., 0., 1000., 600.);
        let rect = menu_rect(
            main,
            1.,
            anchor(990., 590., Corner::TopLeft),
            menu_size(8, 0),
        );
        assert_eq!((rect.left, rect.top), (772, 342));
        let rect = menu_rect(
            main,
            1.,
            anchor(-20., -5., Corner::TopLeft),
            menu_size(3, 0),
        );
        assert_eq!((rect.left, rect.top), (8, 8));
    }

    #[test]
    fn physical_pixels_follow_the_main_windows_scale_on_any_monitor() {
        let rect = menu_rect(
            main_window(-1600., 40., 1280., 800.),
            1.5,
            anchor(20., 100., Corner::TopLeft),
            menu_size(3, 0),
        );
        assert_eq!(
            rect,
            OverlayRect {
                left: -2370,
                top: 210,
                width: 330,
                height: 150
            }
        );
    }

    #[test]
    fn arrows_move_the_selection_and_wrap_around_the_ends() {
        use MenuStep::*;
        assert_eq!(Next.apply(None, 8), 0, "Down selects the first row");
        assert_eq!(Previous.apply(None, 8), 7, "Up selects the last row");
        assert_eq!(Next.apply(Some(2), 8), 3);
        assert_eq!(Previous.apply(Some(2), 8), 1);
        assert_eq!(Next.apply(Some(7), 8), 0);
        assert_eq!(Previous.apply(Some(0), 8), 7);
        assert_eq!(First.apply(Some(5), 8), 0);
        assert_eq!(Last.apply(Some(1), 8), 7);
        assert_eq!(MenuStep::for_key("down"), Some(Next));
        assert_eq!(MenuStep::for_key("enter"), None);
    }

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

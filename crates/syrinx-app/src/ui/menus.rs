//! The chrome's drop-down menus: the page menu behind ☰, the sidebar's ⋯ menu, a sidebar
//! row's menu and its bulk-close confirmation (#95), the Downloads flyout behind ↓ (#143), and
//! Back and Forward's lists of the tab's recent pages (#126).
//! A menu is its own owned popup window: GPUI content drawn over the page card in the
//! main window never receives the page's clicks or keys (#12).

use std::fmt;
use std::time::{Duration, Instant};

use gpui::{
    KeyDownEvent, MouseDownEvent, MouseMoveEvent, Pixels, Point, WeakEntity,
    prelude::FluentBuilder as _,
};
use syrinx_core::{BrowserAction, DestinationSurface, DownloadControl, Shortcut};
use syrinx_platform::browser::{BrowserError, OverlayRect, hide_overlay, syrinx_in_foreground};

use super::row_menu::{BulkClose, MenuOrigin, TabAction, browser_key_label};
use super::sidebar_tree::TreeId;
use super::title_bar::{CAPTION_GAP, caption_buttons_width};
use super::{
    history_list::ReturnFocus,
    owned_popup::{
        OwnedPopup, OwnedPopupError, PopupSpec, main_window_bounds, open_owned_popup,
        round_popup_corners, show_and_activate,
    },
    *,
};

mod card;
mod downloads;
mod history_rows;
mod items;
mod opening;
mod tab_rows;
mod view;

pub(super) use card::{Corner, MenuAnchor};
use {card::*, downloads::*, items::*, view::*};

/// The menu's width, narrow enough that the sidebar menu stays inside a wide sidebar.
const MENU_WIDTH: f32 = 220.;
/// The Downloads flyout is wider, so a file name, its progress and its buttons fit on one row.
const DOWNLOADS_WIDTH: f32 = 380.;
/// The flyout lists this many downloads, newest first; the session's list keeps more.
const MAX_DOWNLOAD_ROWS: usize = 8;
const ITEM_HEIGHT: f32 = 30.;
/// An item's padding either side of its label and shortcut.
const ITEM_PAD_X: f32 = 10.;
/// An item's corner radius, inside the card's padding.
const ITEM_RADIUS: f32 = 6.;
/// How far a separator's line stops short of the card's sides.
const SEPARATOR_INSET: f32 = 6.;
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
/// The most of a history row's width its host may take before it is cut short.
const HOST_SHARE: f32 = 0.4;

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
    /// Back's or Forward's list of the tab's pages that way (#126).
    History(syrinx_core::HistoryDirection),
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
    /// The ⋯ menu's Close tab, on the tab active when the menu opened.
    CloseActiveTab,
    /// A sidebar row's action (#95).
    Tab(TabAction),
    /// The confirmation's question; shown, never chosen.
    ConfirmPrompt,
    /// Closes the tabs the confirmation named (`OpenMenu::asked`); `remember` stops it
    /// asking again.
    ConfirmBulk {
        bulk: BulkClose,
        remember: bool,
    },
    Cancel,
    /// A page in Back's or Forward's list, `steps` presses away on `tab` (#126).
    HistoryPage {
        tab: TabId,
        direction: syrinx_core::HistoryDirection,
        steps: usize,
    },
    /// Says the list was reset, so pages the engine can still reach are not listed; shown only.
    HistoryUnlisted,
}

impl MenuKind {
    /// The fixed items; the Downloads flyout's rows come from the session's list instead.
    fn items(self) -> &'static [MenuItem] {
        match self {
            MenuKind::Page => &PAGE_ITEMS,
            MenuKind::Sidebar => &SIDEBAR_ITEMS,
            MenuKind::Downloads | MenuKind::Row | MenuKind::Confirm | MenuKind::History(_) => &[],
        }
    }

    fn name(self) -> &'static str {
        match self {
            MenuKind::Page => "Page menu",
            MenuKind::Sidebar => "Sidebar menu",
            MenuKind::Downloads => "Downloads",
            MenuKind::Row => "Tab menu",
            MenuKind::Confirm => "Close tabs",
            MenuKind::History(direction) => super::back_forward::way(direction).menu_name,
        }
    }

    fn width(self) -> f32 {
        match self {
            MenuKind::Downloads => DOWNLOADS_WIDTH,
            MenuKind::History(_) => history_rows::HISTORY_WIDTH,
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
            MenuItem::HistoryPage { .. } => "Page",
            MenuItem::HistoryUnlisted => "Pages aren't listed",
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
            | MenuItem::Cancel
            | MenuItem::HistoryPage { .. }
            | MenuItem::HistoryUnlisted => return None,
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

    /// Whether arrows and the pointer can select the row; separators, the confirmation's
    /// question and the history list's note are drawn only.
    fn is_selectable(self) -> bool {
        !matches!(
            self,
            MenuItem::Separator | MenuItem::ConfirmPrompt | MenuItem::HistoryUnlisted
        )
    }
}

/// One row as the popup draws it, captured when the menu opens.
#[derive(Clone)]
struct MenuRow {
    item: MenuItem,
    label: String,
    shortcut: Option<String>,
    /// A download row's buttons; empty for every other row.
    controls: &'static [DownloadControl],
    /// A history page's placeholder icon letter; `None` for every other row.
    icon: Option<String>,
}

impl fmt::Debug for MenuRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A history row's label and host are the owner's browsing; never print them.
        formatter
            .debug_struct("MenuRow")
            .field("item", &self.item)
            .finish_non_exhaustive()
    }
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
    /// The tab active when it opened, which the ⋯ menu's Close tab closes.
    active: Option<TabId>,
    /// The tabs a bulk-close question named; empty for every other menu.
    asked: Vec<TabId>,
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

impl MenuReturn {
    /// Whether a key opened the menu on a focused row (Shift F10 or the Menu key).
    pub(super) fn keyed(&self) -> bool {
        matches!(self, MenuReturn::Row(_))
    }
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

/// What a closing menu's action does once the menu is gone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AfterClose {
    /// Remember which menu closed, so the press that closed it does not reopen it.
    RecordDismissal,
    /// Give the keys back where the menu took them from (`return_menu_focus`).
    ReturnKeys,
    Run(MenuItem),
    Nothing,
}

impl MenuAction {
    /// What follows this action's close; a key or click acts only while Syrinx is `in_front`.
    fn after_close(self, in_front: bool) -> AfterClose {
        match self {
            MenuAction::Deactivated => AfterClose::RecordDismissal,
            MenuAction::Dismiss if in_front => AfterClose::ReturnKeys,
            MenuAction::Choose(item) if in_front => AfterClose::Run(item),
            MenuAction::Dismiss
            | MenuAction::Choose(_)
            | MenuAction::Zoom(_)
            | MenuAction::Download(..) => AfterClose::Nothing,
        }
    }
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

/// How a menu opens: where, where the keys go when it closes, and whether a key opened it.
pub(super) struct MenuOpening {
    pub(super) anchor: MenuAnchor,
    pub(super) returns: MenuReturn,
    pub(super) select_first: bool,
    /// The tabs a bulk-close question names; empty for every other menu.
    pub(super) asked: Vec<TabId>,
}

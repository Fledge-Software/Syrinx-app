//! The chrome's two drop-down menus: the page menu behind ☰ and the sidebar's ⋯ menu.
//! Each also opens where the owner right-clicks, and every item shows its shortcut.
//! A menu is its own owned popup window: GPUI content drawn over the page card in the
//! main window never receives the page's clicks or keys (#12).

use std::time::{Duration, Instant};

use gpui::{KeyDownEvent, MouseDownEvent, Pixels, Point, WeakEntity, prelude::FluentBuilder as _};
use syrinx_core::{DestinationSurface, Shortcut};
use syrinx_platform::browser::{
    BrowserError, OverlayRect, focus_chrome, hide_overlay, syrinx_in_foreground,
};

use super::{
    owned_popup::{OwnedPopup, OwnedPopupError, PopupSpec, open_owned_popup, show_and_activate},
    *,
};

/// The menu's width, narrow enough that the sidebar menu stays inside a wide sidebar.
const MENU_WIDTH: f32 = 220.;
const ITEM_HEIGHT: f32 = 30.;
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

    fn name(self) -> &'static str {
        match self {
            MenuKind::Page => "Page menu",
            MenuKind::Sidebar => "Sidebar menu",
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

    /// Whether the item puts the keys in a chrome field or list (History places them itself);
    /// every other item returns them to the page, where they were before the menu opened.
    fn takes_keys(self) -> bool {
        matches!(
            self,
            MenuItem::Find | MenuItem::NewFolder | MenuItem::History
        )
    }
}

/// One row as the popup draws it, captured when the menu opens.
#[derive(Clone, Debug)]
struct MenuRow {
    item: MenuItem,
    label: String,
    shortcut: Option<String>,
}

/// The open menu: its popup once the window exists, `None` while it is opening.
pub(super) struct OpenMenu {
    kind: MenuKind,
    generation: u64,
    popup: Option<OwnedPopup>,
    view: Option<WeakEntity<MenuView>>,
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
    /// Escape or the window's close request: the keys go back to the page.
    Dismiss,
    /// The owner clicked elsewhere or switched away; focus stays where that put it.
    Deactivated,
}

/// Where a requested menu opens.
struct MenuRequest {
    generation: u64,
    kind: MenuKind,
    rows: Vec<MenuRow>,
    rect: OverlayRect,
    parent: isize,
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
        let right = f32::from(window.viewport_size().width) - 3. * TITLE_BAR_HEIGHT - 4.;
        let position = point(px(right), px(TITLE_BAR_HEIGHT));
        self.show_menu(MenuKind::Page, position, Corner::TopRight, window, cx);
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

    /// Records the menu and opens its popup once this update ends (see `open_requested_menu`).
    fn show_menu(
        &mut self,
        kind: MenuKind,
        position: Point<Pixels>,
        corner: Corner,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The peek's copy of the sidebar has no menu.
        if !self.is_main_window(window) || self.closing {
            return;
        }
        // A menu replaces every other popup; a live palette's late close would refocus the
        // page and close the menu again (#75).
        self.retire_palette(cx);
        self.retire_peek(cx);
        self.retire_menu(cx);
        let rows = self.menu_rows(kind);
        let size = menu_size(rows.len());
        let anchor = MenuAnchor { position, corner };
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
        });
        let request = MenuRequest {
            generation,
            kind,
            rows,
            rect,
            parent: self.parent_hwnd,
        };
        let browser = cx.weak_entity();
        cx.defer(move |cx| open_requested_menu(browser, request, cx));
        cx.notify();
    }

    /// The rows of `kind` as they read now: the zoom level and favorites editing included.
    fn menu_rows(&self, kind: MenuKind) -> Vec<MenuRow> {
        let zoom = self
            .shown
            .and_then(|id| self.zoom_levels.get(&id.get()).copied())
            .unwrap_or(1.);
        let editing = self.home.editing == Some(DestinationSurface::Favorites);
        kind.items()
            .iter()
            .map(|&item| MenuRow {
                item,
                label: match item {
                    MenuItem::ResetZoom => format!("{} · {:.0}%", item.label(), zoom * 100.),
                    MenuItem::EditFavorites if editing => "Done editing favorites".to_owned(),
                    _ => item.label().to_owned(),
                },
                shortcut: item.shortcut(),
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
        // A queued key or click must not reactivate Syrinx after Alt Tab.
        let in_front = syrinx_in_foreground();
        let kind = self.menu.as_ref().map(|menu| menu.kind);
        self.retire_menu(cx);
        match action {
            MenuAction::Deactivated => {
                self.menu_dismissal = kind.map(|kind| MenuDismissal {
                    kind,
                    at: Instant::now(),
                });
            }
            MenuAction::Dismiss if in_front => self.focus_page(window, cx),
            MenuAction::Choose(item) if in_front => self.run_menu_item(item, window, cx),
            MenuAction::Dismiss | MenuAction::Choose(_) => {}
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

    /// Does what the chosen item names; the keys go back to the page unless it takes them.
    fn run_menu_item(&mut self, item: MenuItem, window: &mut Window, cx: &mut Context<Self>) {
        if item.takes_keys()
            && let Err(error) = focus_chrome(self.parent_hwnd)
        {
            self.message = error.to_string();
        }
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
        if !item.takes_keys() {
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
        rect,
        parent,
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
                menu_size(rows.len()),
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
        // A failure leaves a square, shadowless menu that still works, so it is only reported.
        if let Err(error) = syrinx_platform::window_frame::round_with_shadow(hwnd) {
            eprintln!("Cannot round the menu's corners: {error}");
        }
        let closing = browser.clone();
        popup.on_window_should_close(cx, move |_, cx| {
            let queued = closing.update(cx, |browser, cx| {
                browser.queue_menu_action(generation, MenuAction::Dismiss, cx);
            });
            queued.is_err() // With no owner entity, nothing else will close this window.
        });
        let view = cx.new(|cx| MenuView::new(browser, generation, kind, rows, popup, cx));
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

/// The popup's content: the rows, the keyboard selection, and the keys and clicks on them.
struct MenuView {
    browser: WeakEntity<BrowserView>,
    generation: u64,
    kind: MenuKind,
    rows: Vec<MenuRow>,
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
        kind: MenuKind,
        rows: Vec<MenuRow>,
        popup: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
            selected: None,
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
            self.selected = Some(step.apply(self.selected, self.rows.len()));
            cx.notify();
            return;
        }
        let action = match key {
            "escape" => MenuAction::Dismiss,
            "enter" | "space" => match self.selected.and_then(|row| self.rows.get(row)) {
                Some(row) => MenuAction::Choose(row.item),
                None => return,
            },
            _ => return,
        };
        cx.stop_propagation();
        self.send(action, cx);
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
            let selected = self.selected == Some(index);
            card = card.child(
                menu_row(index, row, selected, &palette)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && this.selected != Some(index) {
                            this.selected = Some(index);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send(MenuAction::Choose(item), cx);
                    })),
            );
        }
        crate::controls::watch_pointer(card)
    }
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
            row.aria_active_descendant()
                .bg(theme::rgba(palette.row_selected))
        })
        .h(px(ITEM_HEIGHT))
        .flex_shrink_0()
        .px(px(10.))
        .flex()
        .items_center()
        .gap_3()
        .rounded(px(6.))
        .cursor_pointer()
        .child(div().flex_1().child(row.label.clone()))
        .children(row.shortcut.clone().map(|keys| {
            div()
                .text_size(px(syrinx_ui::metrics::text::SMALL))
                .text_color(theme::rgba(palette.text_muted))
                .child(keys)
        }))
}

/// Which corner of the menu sits at its anchor point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Corner {
    TopLeft,
    TopRight,
}

/// Where the menu is anchored in the main window, in logical pixels.
#[derive(Clone, Copy, Debug)]
struct MenuAnchor {
    position: Point<Pixels>,
    corner: Corner,
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

/// The popup's logical size for `rows` items.
fn menu_size(rows: usize) -> gpui::Size<Pixels> {
    let height = rows as f32 * ITEM_HEIGHT + 2. * (MENU_PADDING + MENU_BORDER);
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
    fn the_popup_fits_its_rows_exactly() {
        assert_eq!(menu_size(8), size(px(220.), px(250.)));
        assert_eq!(menu_size(3), size(px(220.), px(100.)));
    }

    #[test]
    fn the_page_menu_hangs_from_its_right_corner_in_screen_pixels() {
        // A 1280-wide window at (100, 50); the ☰ corner sits at x 1176, under the title bar.
        let rect = menu_rect(
            main_window(100., 50., 1280., 800.),
            1.,
            anchor(1176., 36., Corner::TopRight),
            menu_size(8),
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
        let rect = menu_rect(main, 1., anchor(990., 590., Corner::TopLeft), menu_size(8));
        assert_eq!((rect.left, rect.top), (772, 342));
        let rect = menu_rect(main, 1., anchor(-20., -5., Corner::TopLeft), menu_size(3));
        assert_eq!((rect.left, rect.top), (8, 8));
    }

    #[test]
    fn physical_pixels_follow_the_main_windows_scale_on_any_monitor() {
        let rect = menu_rect(
            main_window(-1600., 40., 1280., 800.),
            1.5,
            anchor(20., 100., Corner::TopLeft),
            menu_size(3),
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

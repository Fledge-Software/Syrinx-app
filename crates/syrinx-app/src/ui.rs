//! Connects the shared session to browser chrome without running storage or search on the UI thread.

mod back_forward;
mod chrome_focus;
mod clear_data;
mod customize;
mod downloads;
mod essentials;
mod favorites;
mod find;
mod folder_cards;
mod folder_editor;
mod folder_expansion;
mod fullscreen;
mod history_list;
mod history_repair;
mod home;
mod home_palette;
mod import;
mod incoming_links;
mod main_window;
mod menus;
mod mute;
mod navigation_intents;
mod navigation_status;
mod owned_popup;
mod page_card;
mod page_lifecycle;
mod page_text_capture;
mod page_text_settings;
mod palette_acceptance;
mod palette_popup;
mod panes;
mod peek;
mod peek_state;
mod permissions;
mod popups;
mod process_recovery;
mod recovery;
mod row_menu;
mod runtime_notice;
mod saved_moves;
mod security;
mod settings_page;
mod sidebar_rows;
mod sidebar_tree;
mod sidebar_view;
mod site_permissions;
mod site_zoom;
mod suspended_fixture;
mod theme_editor;
mod tile_editor;
mod tiles;
mod title_bar;
mod updates;
mod usage_preview;
mod usage_report;

#[cfg(test)]
mod shutdown_tests;

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashSet},
    num::NonZeroU64,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use futures::{StreamExt, channel::mpsc::unbounded};
use gpui::StatefulInteractiveElement;
use gpui::{
    App, AppContext, Bounds, Context, Div, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render,
    Stateful, Styled, Subscription, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    actions, div, point, px, rgb, size,
};
use gpui_component::Root;
use main_window::update_in_main;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use syrinx_core::usage::{ErrorCategory, Feature, UsageRecorder};
use syrinx_core::{
    ActivationTarget, BrowserAction, CommandAction, CommandContext, CommandRegistry,
    HistoryDirection, PageOrigin, Route, SavedFolderId, TabId, TabModelSnapshot, TabSnapshot,
    route,
};
use syrinx_platform::browser::{
    BrowserCommand, BrowserEvent, BrowserHost, BrowserShortcut, PageBounds, set_overlay_owner,
};
use syrinx_platform::instance::LinkFeed;
use syrinx_search::{BlankQuery, BoostWeights, Scope, SearchPolicy, Target};
use syrinx_ui::{
    SidebarSizing,
    focus::{Direction, ModalityInput, Pane},
    metrics::{chrome::TITLE_BAR_HEIGHT, radius},
    sidebar::SIDEBAR_PADDING_RIGHT,
};
use title_bar::title_bar;

pub(crate) use panes::{FocusNextControl, FocusPreviousControl};

use history_list::ChromeEscape;
use home_palette::HomePaletteIntent;
use navigation_intents::NavigationIntents;
use navigation_status::NavigationStatus;

use crate::{
    controls::{glyph, icon_button},
    navigation,
    palette::{self, PaletteEvent, PaletteView},
    session::{self, SessionCommand, SessionEvent, SessionHandle},
    theme,
};

/// The status line under the page card: one caption line, kept thin so the page gets the room
/// (owner, 2026-10-25; it was 28).
const FOOTER_HEIGHT: f32 = 14.;

actions!(
    browser,
    [
        NewTab,
        CloseTab,
        Address,
        Palette,
        NextTab,
        PreviousTab,
        ReopenTab,
        SavePage,
        Back,
        Forward,
        Reload,
        ShowHistory,
        ClearBrowsingData,
        InstallUpdate,
        CheckUpdates,
        ToggleSidebar,
        DismissPeek,
        Fullscreen
    ]
);

/// Binds the window's keys and each part of the chrome's own.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys(window_bindings());
    crate::palette::bind_keys(cx);
    import::bind_keys(cx);
    essentials::bind_zoom_keys(cx);
    back_forward::bind_keys(cx);
    find::bind_keys(cx);
    folder_editor::bind_keys(cx);
    panes::bind_keys(cx);
}

/// Each browser command's shortcut from the command registry, Ctrl K for the palette, the
/// window-only keys, and last the close-tab key's meaning on a focused sidebar row (DW-63).
fn window_bindings() -> Vec<KeyBinding> {
    let registry = CommandRegistry::browser();
    let mut bindings: Vec<KeyBinding> = registry
        .iter()
        .filter_map(|(_, command)| {
            let CommandAction::Browser(action) = command.action else {
                return None;
            };
            let shortcut = command.shortcut.as_ref()?;
            Some(key_binding(action, shortcut.keystroke()))
        })
        .collect();
    // The palette is where commands run, so it is not itself a command.
    bindings.push(KeyBinding::new("ctrl-k", Palette, None));
    // The sidebar toggle, Settings and peek dismissal are window keys, not palette commands.
    bindings.extend([
        KeyBinding::new("ctrl-alt-s", ToggleSidebar, None),
        KeyBinding::new(
            settings_page::SETTINGS_KEY,
            settings_page::OpenSettings,
            None,
        ),
        KeyBinding::new("escape", DismissPeek, Some("Syrinx")),
        KeyBinding::new("escape", crate::palette::Dismiss, Some("SyrinxPeek")),
    ]);
    // Fullscreen is a window key; Escape leaves page fullscreen only while it is active.
    bindings.extend([
        KeyBinding::new("f11", Fullscreen, None),
        KeyBinding::new(
            "escape",
            fullscreen::ExitPageFullscreen,
            Some("SyrinxFullscreen"),
        ),
    ]);
    // After the window's close-tab binding, so a focused row's meaning wins (see the helper).
    if let Some(close) = browser_keystroke(&registry, BrowserAction::CloseTab) {
        bindings.push(folder_editor::close_row_page_binding(close));
    }
    bindings
}

/// The registry's shortcut for one browser action, if it has one.
fn browser_keystroke(registry: &CommandRegistry, wanted: BrowserAction) -> Option<&str> {
    registry
        .iter()
        .find_map(|(_, command)| match command.action {
            CommandAction::Browser(action) if action == wanted => command
                .shortcut
                .as_ref()
                .map(|shortcut| shortcut.keystroke()),
            _ => None,
        })
}

fn key_binding(action: BrowserAction, keystroke: &str) -> KeyBinding {
    match action {
        BrowserAction::NewTab => KeyBinding::new(keystroke, NewTab, None),
        BrowserAction::CloseTab => KeyBinding::new(keystroke, CloseTab, None),
        BrowserAction::ReopenTab => KeyBinding::new(keystroke, ReopenTab, None),
        BrowserAction::Reload => KeyBinding::new(keystroke, Reload, None),
        BrowserAction::Back => KeyBinding::new(keystroke, Back, None),
        BrowserAction::Forward => KeyBinding::new(keystroke, Forward, None),
        BrowserAction::EditAddress => KeyBinding::new(keystroke, Address, None),
        BrowserAction::NextTab => KeyBinding::new(keystroke, NextTab, None),
        BrowserAction::PreviousTab => KeyBinding::new(keystroke, PreviousTab, None),
        BrowserAction::SavePage => KeyBinding::new(keystroke, SavePage, None),
        BrowserAction::ShowHistory => KeyBinding::new(keystroke, ShowHistory, None),
        BrowserAction::FindInPage => KeyBinding::new(keystroke, essentials::Find, None),
        BrowserAction::ShowDownloads => KeyBinding::new(keystroke, essentials::Downloads, None),
        BrowserAction::Print => KeyBinding::new(keystroke, essentials::Print, None),
        BrowserAction::ZoomIn => KeyBinding::new(keystroke, essentials::ZoomIn, None),
        BrowserAction::ZoomOut => KeyBinding::new(keystroke, essentials::ZoomOut, None),
        BrowserAction::ResetZoom => KeyBinding::new(keystroke, essentials::ZoomReset, None),
        BrowserAction::ClearBrowsingData => KeyBinding::new(keystroke, ClearBrowsingData, None),
        BrowserAction::InstallUpdateFromFile => KeyBinding::new(keystroke, InstallUpdate, None),
        BrowserAction::CheckForUpdates => KeyBinding::new(keystroke, CheckUpdates, None),
    }
}

/// The session command that opens `target`, or `None` for a tab this snapshot no longer has.
///
/// Saved pages always go to the worker: its `open_saved` focuses a live instance against its own
/// current model, so a click racing a queued close can neither fail nor load a duplicate (D43).
fn activation_command(
    model: &TabModelSnapshot,
    target: ActivationTarget,
) -> Option<SessionCommand> {
    match target {
        ActivationTarget::Saved(id) => Some(SessionCommand::OpenSaved(id)),
        ActivationTarget::Tab(_) => match route(model, target)? {
            Route::Focus(id) => Some(SessionCommand::FocusTab(id)),
            Route::Load(id) => Some(SessionCommand::OpenSaved(id)),
        },
    }
}

/// Runs a browser command through the same handler as its keyboard shortcut; clear browsing
/// data and the two update commands have no native page shortcut, so they run directly.
fn browser_shortcut(action: BrowserAction) -> Option<BrowserShortcut> {
    Some(match action {
        BrowserAction::NewTab => BrowserShortcut::NewTab,
        BrowserAction::CloseTab => BrowserShortcut::CloseTab,
        BrowserAction::ReopenTab => BrowserShortcut::ReopenTab,
        BrowserAction::Reload => BrowserShortcut::Reload,
        BrowserAction::Back => BrowserShortcut::Back,
        BrowserAction::Forward => BrowserShortcut::Forward,
        BrowserAction::EditAddress => BrowserShortcut::Address,
        BrowserAction::NextTab => BrowserShortcut::NextTab,
        BrowserAction::PreviousTab => BrowserShortcut::PreviousTab,
        BrowserAction::SavePage => BrowserShortcut::SavePage,
        BrowserAction::FindInPage => BrowserShortcut::Find,
        BrowserAction::ShowDownloads => BrowserShortcut::Downloads,
        BrowserAction::Print => BrowserShortcut::Print,
        BrowserAction::ZoomIn => BrowserShortcut::ZoomIn,
        BrowserAction::ZoomOut => BrowserShortcut::ZoomOut,
        BrowserAction::ResetZoom => BrowserShortcut::ZoomReset,
        BrowserAction::ShowHistory => BrowserShortcut::History,
        BrowserAction::ClearBrowsingData
        | BrowserAction::InstallUpdateFromFile
        | BrowserAction::CheckForUpdates => return None,
    })
}

/// Whether a palette command hands the keys back to the page once it has run; the others
/// (edit address, find, history, downloads) place focus themselves and must not be overridden.
fn command_returns_focus_to_page(action: BrowserAction) -> bool {
    !matches!(
        action,
        BrowserAction::EditAddress
            | BrowserAction::FindInPage
            | BrowserAction::ShowHistory
            | BrowserAction::ShowDownloads
    )
}

/// Whether a shortcut opens something in Syrinx's chrome, which page fullscreen hides.
fn needs_chrome(shortcut: BrowserShortcut) -> bool {
    matches!(
        shortcut,
        BrowserShortcut::Find
            | BrowserShortcut::FindNext
            | BrowserShortcut::FindPrevious
            | BrowserShortcut::History
            | BrowserShortcut::Settings
    )
}

/// How this launch runs: normally, or as one of the unattended synthetic checks.
pub struct LaunchChecks {
    pub smoke: bool,
    /// P1-15's measurement mode: this many synthetic tabs suspended in a fresh profile.
    pub suspended_tabs: usize,
    /// P1-33's palette acceptance run.
    pub acceptance: Option<crate::acceptance::AcceptancePlan>,
    /// P1-39b: whether this copy can update, and what start-up found.
    pub updates: (
        crate::updates::UpdateContext,
        crate::updates::StartupUpdates,
    ),
    /// AN-01's shared counters, off until the owner switches them on, and when `main` began.
    pub usage: (Arc<UsageRecorder>, Instant),
}

pub struct BrowserView {
    fullscreen: Option<(u64, bool)>,
    crashes: process_recovery::CrashState,
    fullscreen_pages: fullscreen::FullscreenPages,
    suspended_fixture: suspended_fixture::SuspendedFixture,
    connections: std::collections::HashMap<u64, syrinx_platform::browser::ConnectionState>,
    find: Option<find::FindBar>,
    find_sequence: u64,
    site_zoom: site_zoom::SiteZoom,
    back_forward: back_forward::BackForward,
    /// Tabs the owner muted in this run; never saved (#95).
    muted: HashSet<TabId>,
    /// Tabs whose page last reported itself muted, which the sidebar's badge shows (#95).
    shown_muted: HashSet<TabId>,
    /// Whether Close other tabs and Close tabs below ask first (owner, 2026-10-27).
    ask_before_bulk_close: bool,
    /// This session's downloads, in memory only (#143).
    downloads: syrinx_core::DownloadList,
    permissions: Vec<permissions::Prompt>,
    model: TabModelSnapshot,
    session_revision: Option<u64>,
    recovery: Option<recovery::RecoveryCard>,
    history_repair: Option<history_repair::HistoryRepairCard>,
    session: Option<SessionHandle>,
    browser: Option<BrowserHost>,
    session_stopped: bool,
    browser_stopped: bool,
    browser_ready: bool,
    closing: bool,
    loaded: HashSet<TabId>,
    pending_pages: HashSet<TabId>,
    popup_pages: HashSet<TabId>,
    navigation_intents: NavigationIntents,
    navigation_status: NavigationStatus,
    history: history_list::HistoryList,
    clear_data: clear_data::ClearDataDialog,
    settings: settings_page::SettingsPage,
    shown: Option<TabId>,
    bounds: Option<PageBounds>,
    sidebar_sizing: SidebarSizing,
    collapsed_folders: HashSet<SavedFolderId>,
    /// The expanded folders as last stored in settings (P1-21).
    expanded_saved: BTreeSet<SavedFolderId>,
    folder_editor: Option<folder_editor::FolderEditor>,
    sidebar_edge_focus: FocusHandle,
    /// Each window's sidebar row focus handles and drawn rows (DW-61).
    sidebar_trees: sidebar_tree::SidebarTrees,
    panes: panes::PaneFocus,
    /// Follows the main window's activation: ends a sidebar drag or an open palette when it is
    /// lost, and returns focus when it comes back; dropping it unsubscribes.
    _main_activation_watch: Subscription,
    message: String,
    failure: Option<String>,
    focus: FocusHandle,
    /// The open palette entry; `None` while the kept popup is hidden.
    palette: Option<palette_popup::PalettePopup>,
    /// The open drop-down menu, if any; it draws in its own popup window (#12).
    menu: Option<menus::OpenMenu>,
    menu_generation: u64,
    /// The menu a press on the main window last closed, so that press does not reopen it.
    menu_dismissal: Option<menus::MenuDismissal>,
    /// The palette popup, created once and kept hidden between entries (DW-90).
    kept_palette: Option<palette_popup::KeptPalette>,
    palette_entries: u64,
    /// The page is to take the keys once the owner brings Syrinx back to the front.
    page_focus_on_activation: bool,
    commands: Rc<CommandRegistry>,
    main_window: gpui::AnyWindowHandle,
    peek: Option<peek::PeekPopup>,
    /// The generation of a peek whose window opens once the current update ends.
    peek_opening: Option<u64>,
    peek_state: peek_state::PeekState,
    peek_monitoring: bool,
    peek_generation: u64,
    /// The main window's placement when the peek last checked it.
    peek_owner_geometry: peek::OwnerGeometry,
    _peek_bounds_watch: Subscription,
    home_palette: HomePaletteIntent,
    importer: import::BookmarkImporter,
    home: home::HomeState,
    /// Keeps keyboard focus as Settings' permission list opens, closes and rereads (DW-7).
    site_permission_focus: site_permissions::PermissionFocus,
    incoming: incoming_links::IncomingLinks,
    runtime: runtime_notice::RuntimeNotice,
    parent_hwnd: isize,
    /// The scale the window icon was last sized for; a monitor change re-sizes it.
    icon_scale: f32,
    query_sequence: u64,
    pending_query: Option<(u64, u64)>,
    smoke: bool,
    smoke_navigation: bool,
    smoke_waiting: bool,
    /// P1-33's scripted palette acceptance run; `None` in every other launch.
    acceptance: Option<palette_acceptance::Acceptance>,
    /// Settings' Updates section and the staged update's handover (P1-39b).
    updates: updates::UpdatePanel,
    /// Settings' Usage report section and the window's own measurements (AN-01).
    usage: usage_report::UsageState,
    /// The page-text setting and which tabs' next page must not be read (#54).
    page_text: page_text_capture::PageTextCapture,
    /// Settings' open "exclude a site" field, if any.
    page_text_site: Option<page_text_settings::SiteField>,
    outcome: Arc<AtomicU8>,
}

/// One budget for a quit without a close request to stop both workers. Windows allows about
/// 5 s after it ends the session; the engine's own save wait (3 s) fits inside (#96).
const QUIT_BOUND: Duration = Duration::from_millis(4_500);

impl BrowserView {
    pub fn new(
        data_dir: PathBuf,
        checks: LaunchChecks,
        outcome: Arc<AtomicU8>,
        links: Option<LinkFeed>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let LaunchChecks {
            smoke,
            suspended_tabs,
            acceptance,
            updates: (update_context, startup_updates),
            usage: (usage, launched),
        } = checks;
        let mut view = Self {
            fullscreen: None,
            crashes: process_recovery::CrashState::default(),
            fullscreen_pages: fullscreen::FullscreenPages::default(),
            suspended_fixture: suspended_fixture::SuspendedFixture::new(suspended_tabs),
            connections: std::collections::HashMap::new(),
            find: None,
            find_sequence: 0,
            site_zoom: site_zoom::SiteZoom::default(),
            back_forward: back_forward::BackForward::default(),
            muted: HashSet::new(),
            shown_muted: HashSet::new(),
            ask_before_bulk_close: true,
            downloads: syrinx_core::DownloadList::default(),
            permissions: Vec::new(),
            model: TabModelSnapshot::default(),
            session_revision: None,
            recovery: None,
            history_repair: None,
            session: None,
            browser: None,
            session_stopped: true,
            browser_stopped: true,
            browser_ready: false,
            closing: false,
            loaded: HashSet::new(),
            pending_pages: HashSet::new(),
            popup_pages: HashSet::new(),
            navigation_intents: NavigationIntents::default(),
            navigation_status: NavigationStatus::default(),
            history: history_list::HistoryList::default(),
            clear_data: clear_data::ClearDataDialog::default(),
            settings: settings_page::SettingsPage::new(cx),
            shown: None,
            bounds: None,
            sidebar_sizing: SidebarSizing::default(),
            collapsed_folders: HashSet::new(),
            expanded_saved: BTreeSet::new(),
            folder_editor: None,
            // A tracked handle takes its Tab stop from the handle, not from `tab_index`.
            sidebar_edge_focus: cx.focus_handle().tab_stop(true),
            sidebar_trees: sidebar_tree::SidebarTrees::default(),
            panes: panes::PaneFocus::new(cx),
            _main_activation_watch: cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.sidebar_drag_abandoned(window, cx);
                    this.end_palette_when_syrinx_left(cx);
                } else {
                    // Returning from our own popup must preserve Escape's edge dismissal.
                    if !this.peek_monitoring {
                        this.peek_state.rearm();
                    }
                    this.watch_peek(window, cx);
                    this.focus_page_after_activation(window, cx);
                    this.reassert_palette_after_activation(cx);
                }
            }),
            message: "Opening your workspace…".to_owned(),
            failure: None,
            focus: cx.focus_handle(),
            palette: None,
            menu: None,
            menu_generation: 0,
            menu_dismissal: None,
            kept_palette: None,
            palette_entries: 0,
            page_focus_on_activation: false,
            commands: Rc::new(CommandRegistry::browser()),
            main_window: Window::window_handle(window),
            peek: None,
            peek_opening: None,
            peek_state: peek_state::PeekState::default(),
            peek_monitoring: false,
            peek_generation: 0,
            peek_owner_geometry: peek::OwnerGeometry::of(window),
            _peek_bounds_watch: cx.observe_window_bounds(window, |this, window, cx| {
                this.owner_bounds_observed(window, cx);
                this.follow_palette(window, cx);
                this.refresh_icon_for_scale(window);
            }),
            home_palette: HomePaletteIntent::default(),
            importer: import::BookmarkImporter::default(),
            home: home::HomeState::default(),
            site_permission_focus: site_permissions::PermissionFocus::new(cx),
            incoming: incoming_links::IncomingLinks::default(),
            runtime: runtime_notice::RuntimeNotice::default(),
            parent_hwnd: 0,
            icon_scale: window.scale_factor(),
            query_sequence: 0,
            pending_query: None,
            smoke,
            smoke_navigation: false,
            smoke_waiting: false,
            acceptance: acceptance.map(palette_acceptance::Acceptance::new),
            updates: updates::UpdatePanel::new(update_context, startup_updates),
            usage: usage_report::UsageState::new(usage, launched),
            page_text: page_text_capture::PageTextCapture::default(),
            page_text_site: None,
            outcome,
        };
        // A Windows session end quits GPUI without a close request; stop the workers then (#96).
        cx.on_app_quit(|view, _cx| {
            view.stop_workers_on_quit();
            async {}
        })
        .detach();
        window.focus(&view.focus, cx);
        match native_handle(window) {
            Ok(handle) => {
                view.parent_hwnd = handle;
                show_app_icon(handle);
                title_bar::correct_caption_hit_test(handle);
                owned_popup::keep_frame_with_family(handle);
            }
            Err(error) => {
                view.fail(error);
                return view;
            }
        }
        view.start_session(data_dir.join(session::STATE_FOLDER), window, cx);
        view.start_browser(data_dir.join("WebView2"), window, cx);
        if let Some(links) = links {
            view.start_link_feed(links, window, cx);
        }
        view.start_acceptance(window, cx);
        view
    }

    fn start_session(&mut self, directory: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let usage = self.usage.recorder.clone();
        let started = if self.acceptance.is_some() {
            session::spawn_acceptance(directory, usage)
        } else {
            session::spawn(directory, usage)
        };
        let (session, mut events) = match started {
            Ok(started) => started,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        self.session = Some(session);
        self.session_stopped = false;
        cx.spawn_in(window, async move |view, cx| {
            while let Some(event) = events.next().await {
                if let Err(error) = update_in_main(&view, cx, |view, window, cx| {
                    view.session_event(event, window, cx)
                }) {
                    // Ending drops the stream; the worker keeps saving, unheard by this window (#9).
                    eprintln!("Session events stopped reaching the browser window: {error:#}");
                    return;
                }
            }
            let stopped = update_in_main(&view, cx, |view, window, cx| {
                if !view.session_stopped {
                    view.fail_unless_failed("The session worker stopped unexpectedly.");
                    view.session_event(SessionEvent::Stopped, window, cx);
                }
            });
            if let Err(error) = stopped {
                eprintln!("Session event stream closed after its window was removed: {error:#}");
            }
        })
        .detach();
    }

    fn start_browser(&mut self, profile: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let (browser, mut events) = match BrowserHost::spawn(self.parent_hwnd, profile) {
            Ok(started) => started,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        self.browser = Some(browser);
        self.browser_stopped = false;
        cx.spawn_in(window, async move |view, cx| {
            while let Some(event) = events.next().await {
                if let Err(error) = update_in_main(&view, cx, |view, window, cx| {
                    view.browser_event(event, window, cx)
                }) {
                    // Ending here drops the stream, so its sender fails next; say why (#9).
                    eprintln!("Page host events stopped reaching the browser window: {error:#}");
                    return;
                }
            }
            let stopped = update_in_main(&view, cx, |view, window, cx| {
                if !view.browser_stopped {
                    view.fail("The page host stopped unexpectedly.");
                    view.browser_event(BrowserEvent::Stopped, window, cx);
                }
            });
            if let Err(error) = stopped {
                eprintln!("Browser event stream closed after its window was removed: {error:#}");
            }
        })
        .detach();
    }

    fn session_event(&mut self, event: SessionEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            SessionEvent::RecoveryOffered(prompt) => {
                // Start-up then waited on the owner, so its timings would measure the owner.
                self.usage.startup_waited();
                if !self.closing {
                    self.recovery = Some(recovery::RecoveryCard::new(prompt));
                    self.message =
                        "A verified backup is available. Confirm before continuing recovery."
                            .to_owned();
                }
            }
            SessionEvent::HistoryRepairOffered(prompt) => {
                self.usage.startup_waited();
                if !self.closing {
                    self.recovery = None;
                    self.history_repair = Some(history_repair::HistoryRepairCard::new(prompt));
                    self.message =
                        "Clearing all history is needed before Syrinx can continue.".to_owned();
                }
            }
            SessionEvent::Preferences {
                settings,
                destinations,
                site_zooms,
            } => {
                self.site_zoom.restore(site_zooms.0);
                self.runtime_settings_loaded(&settings);
                self.restore_folder_expansion(&settings);
                self.restore_page_text(&settings);
                self.restore_preferences(&settings, destinations, window, cx);
            }
            SessionEvent::Destinations(destinations) => self.destinations_changed(destinations),
            SessionEvent::State(snapshot) => {
                self.recovery = None;
                self.history_repair = None;
                // Restored sessions already have saved revisions; only the first state is startup.
                let previous_revision = self.session_revision.replace(snapshot.revision);
                let initial = previous_revision.is_none();
                let changed = previous_revision != Some(snapshot.revision);
                if initial {
                    self.collapse_restored_folders(&snapshot.model);
                }
                let saved_message = if initial
                    && !snapshot.model.tabs.is_empty()
                    && snapshot.previous_shutdown == syrinx_store::ShutdownState::Unclean
                {
                    "Recovered the saved session after an interrupted exit."
                } else {
                    "Saved locally"
                };
                let new_home = self.home_palette.acknowledge(
                    &snapshot.model,
                    &snapshot.created,
                    snapshot.home_entry,
                );
                // Switching or opening a page leaves the history list for that page.
                if snapshot.model.active_id != self.model.active_id {
                    self.history.close();
                    self.clear_data.close();
                }
                if settings_page::settings_tab_closed(&self.model, &snapshot.model) {
                    self.close_settings_subviews();
                }
                self.model = snapshot.model;
                self.forget_closed_history();
                if initial {
                    self.usage.session_restored();
                }
                // Home shows no page, so a selection that ends there has nothing to time.
                let home = self.active_address().is_empty();
                self.usage.state_arrived(self.model.active_id, home);
                self.save_folder_expansion();
                self.crashes.retain(&self.model);
                self.retain_mute();
                let tabs = &self.model.tabs;
                self.site_zoom
                    .retain_pages(|id| tabs.iter().any(|tab| tab.id.get() == id));
                self.page_text
                    .tabs_changed(&self.model, initial, &snapshot.reopened);
                self.navigation_status.retain(&self.model);
                if !self.closing {
                    if let Some(id) = snapshot.navigate {
                        self.navigation_status.forget(id);
                    }
                    self.message = self
                        .navigation_status
                        .active_message(&self.model)
                        .unwrap_or(saved_message)
                        .to_owned();
                    if initial && let Some(notice) = self.updates.take_startup_notice() {
                        self.message = notice;
                    }
                }
                // After the status line, so a withdrawn removal card can explain itself.
                self.follow_folder_editor();
                if let Some(id) = snapshot.navigate
                    && !self.closing
                {
                    self.navigate_committed(id);
                }
                if let Some((request, target)) = snapshot.popup {
                    self.commit_popup(request, target);
                }
                let unattended =
                    self.smoke || self.suspended_fixture.enabled() || self.acceptance.is_some();
                if new_home && !self.closing && !unattended {
                    self.take_keys_for_new_home();
                }
                self.sync_pages();
                // Work that waited for the session to accept commands.
                self.open_forwarded_links();
                self.save_runtime_version();
                self.restore_fullscreen_if_hidden(window, cx);
                if initial {
                    self.record_clean_start(cx);
                    self.seed_suspended_fixture();
                    // After this frame, so the window shows its tabs before the popup is built.
                    cx.spawn(async move |view, cx| {
                        if view.update(cx, Self::prepare_palette).is_err() {
                            eprintln!("The browser closed before the palette was prepared.");
                        }
                    })
                    .detach();
                }
                // Only committed model changes invalidate an open palette's catalogue snapshot.
                if changed
                    && !self.closing
                    && let Some(palette) = &self.palette
                {
                    self.pending_query = None;
                    let context = self.command_context();
                    palette
                        .view
                        .update(cx, |palette, cx| palette.refresh(context, cx));
                }
                if self.smoke && self.model.tabs.is_empty() && !self.closing {
                    self.command(SessionCommand::NewTab {
                        address: "about:blank".to_owned(),
                    });
                }
                if new_home && !self.closing && !unattended {
                    self.open_palette(false, window, cx);
                }
            }
            SessionEvent::Permission {
                id,
                request,
                result,
            } => self.permission_result(id, request, result),
            SessionEvent::PopupRejected { request } => {
                self.page_command(BrowserCommand::CancelPopup { request });
            }
            SessionEvent::Search { request_id, result } => {
                if let Some((pending, local)) = self.pending_query
                    && pending == request_id
                    && let Some(palette) = &self.palette
                {
                    let failed = result.is_err();
                    // Saved instances collapse into one row against the current model.
                    let result = result.map(|results| palette::arrange(results, &self.model));
                    let view = palette.view.clone();
                    // The reply may apply a waiting Enter, which closes the popup's own window.
                    // An `Err` means the popup already closed and its reply has nothing to show.
                    let delivered = palette.window.update(cx, |_, popup, cx| {
                        view.update(cx, |palette, cx| palette.receive(local, result, popup, cx))
                    });
                    if self.smoke && failed && matches!(delivered, Ok(true)) {
                        self.outcome.store(2, Ordering::Release);
                    }
                }
            }
            SessionEvent::QueryTimed {
                request_id,
                elapsed,
            } => self.acceptance_query_timed(request_id, elapsed),
            SessionEvent::SyntheticCatalogue(result) => self.acceptance_synthetic(result),
            SessionEvent::Imported { request_id, result } => {
                self.bookmarks_imported(request_id, result);
            }
            SessionEvent::History { request_id, result } => {
                self.history.receive(request_id, result);
            }
            SessionEvent::HistoryCleared { request_id, result } => {
                self.history_cleared(request_id, result);
            }
            SessionEvent::SiteZoomsCleared { request_id, result } => {
                self.site_zooms_cleared(request_id, result);
            }
            SessionEvent::SiteZoomsReplaced(levels) => self.replace_site_zooms(levels),
            SessionEvent::RememberedPermissions { request_id, result } => {
                self.site_permissions_listed(request_id, result, window, cx);
            }
            SessionEvent::PermissionForgotten { request_id, result } => {
                self.site_permission_forgotten(request_id, result);
            }
            SessionEvent::Failure { error, fatal } => {
                if self.smoke {
                    self.outcome.store(2, Ordering::Release);
                }
                // A stopped session cannot persist pages, so the page host stops with it.
                if fatal {
                    self.page_command(BrowserCommand::Shutdown);
                    self.home.site_permissions.session_lost();
                }
                if fatal && self.abandon_import_write() {
                    self.fail(format!(
                        "Storage stopped while importing bookmarks; the import may not have been saved: {error}"
                    ));
                } else if fatal {
                    self.fail(format!("Storage stopped: {error}"));
                    self.clear_data_session_lost();
                } else {
                    self.message = error.to_string();
                }
            }
            SessionEvent::UpdateSnapshot(result) => self.update_snapshot_taken(result, cx),
            SessionEvent::Usage { status, problem } => {
                self.usage_status_received(status, problem);
            }
            SessionEvent::UsageReport {
                request_id,
                period,
                result,
            } => self.usage_report_received(request_id, period, result),
            SessionEvent::Stopped => {
                self.update_snapshot_lost();
                self.abandon_import_write();
                self.home.site_permissions.session_lost();
                if !self.closing {
                    self.fail_unless_failed(
                        "Local storage stopped. Restart Syrinx before continuing.",
                    );
                }
                self.session_stopped = true;
                if let Some(session) = self.session.take()
                    && let Err(error) = session.join()
                {
                    self.fail(error.to_string());
                }
                self.finish_close(cx);
            }
        }
        if self.closing {
            window.refresh();
        }
        cx.notify();
    }

    fn browser_event(&mut self, event: BrowserEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = event.into_current() else {
            return;
        };
        if process_recovery::smoke_process_failure(self.smoke, &event, &self.outcome) {
            self.request_close(cx);
            return;
        }
        if let Some(message) = self.navigation_status.observe(
            &event,
            &self.model,
            &self.loaded,
            &self.pending_pages,
            self.closing,
        ) {
            self.message = message.to_owned();
        }
        match event {
            BrowserEvent::PageCrashed { id } => {
                self.usage.recorder.error(ErrorCategory::RendererFailed);
                self.page_crashed(id, window);
                self.recovery_focus(window, cx);
            }
            BrowserEvent::PageRestored { id } => {
                self.crashes.restored(id);
                self.reapply_mute(id);
                self.recovery_focus(window, cx);
            }
            BrowserEvent::RecoveryStarted { sequence, tabs } => {
                self.browser_recovering(sequence, tabs, window);
                self.recovery_focus(window, cx);
            }
            BrowserEvent::RecoveryFinished {
                sequence,
                restored: _,
                failed,
                stopped,
            } => {
                if self.crashes.finished(sequence, stopped) {
                    self.message = if stopped {
                        "Browser engine stopped. Reload a tab to try again.".into()
                    } else if failed.is_empty() {
                        "Page engine recovered. Your tabs are intact.".into()
                    } else {
                        "Some pages could not be restored. Reload an affected tab to retry.".into()
                    };
                    self.recovery_focus(window, cx);
                }
            }
            BrowserEvent::Callback { .. } => unreachable!("callback envelopes are unwrapped once"),
            BrowserEvent::Fullscreen { id, active } => self.page_fullscreen(id, active, window, cx),
            BrowserEvent::Suspended { id, suspended } => {
                self.suspended_fixture.observe(id, suspended)
            }
            BrowserEvent::Connection { id, state } => self.observe_connection(id, state),
            BrowserEvent::ZoomChanged { id, factor } => self.page_zoom_reported(id, factor, cx),
            // Only the mute state is shown: no speaker icon while a page plays (owner, 2026-10-27).
            BrowserEvent::AudioChanged { id, muted, .. } => self.audio_reported(id, muted, cx),
            BrowserEvent::FindResult {
                id,
                request,
                count,
                active,
            } => self.find_completed(id, request, count, active),
            BrowserEvent::DownloadDialog { .. } => {}
            BrowserEvent::DownloadStarted {
                download,
                tab,
                file_name,
                total,
            } => self.download_started(download, tab, &file_name, total, window, cx),
            BrowserEvent::DownloadChanged {
                download,
                state,
                received,
                total,
                can_resume,
            } => self.download_changed(download, state, received, (total, can_resume), window, cx),
            BrowserEvent::PermissionRequested {
                id,
                request,
                uri,
                kind,
            } => self.permission_requested(id, request, &uri, kind),
            BrowserEvent::PermissionFinished { id, request } => {
                self.permission_finished(id, request)
            }
            BrowserEvent::Ready => {
                self.browser_ready = true;
                // Before any page exists, so every page is observed by the owner's setting.
                self.send_page_text_policy();
                self.resize_pages(window);
                self.sync_pages();
            }
            BrowserEvent::RuntimeVersion(version) => self.runtime_version_observed(version),
            BrowserEvent::Title { id, title } => {
                self.acceptance_title(id, &title);
                self.history_retitled(id, &title);
                self.metadata(id, Some(title), None);
            }
            BrowserEvent::CloseRequested { id } => self.page_requested_close(id),
            BrowserEvent::Created { id } => {
                // A new page starts audible whatever its tab's last page reported.
                if let Ok(tab) = TabId::new(id) {
                    self.page_gone(tab);
                }
                self.reapply_mute(id);
                // A new controller starts at 100%; its first address applies the site's level.
                self.site_zoom.forget_page(id);
                if let Ok(id) = TabId::new(id)
                    && page_lifecycle::created(id, &mut self.pending_pages, &mut self.loaded)
                {
                    self.sync_pages();
                    self.finish_smoke(cx);
                }
            }
            BrowserEvent::CreationFailed { id, error } => {
                self.usage.recorder.error(ErrorCategory::WebviewCreation);
                let active = self.active_page_feedback(id);
                let mut popup_failed = false;
                if let Ok(id) = TabId::new(id) {
                    popup_failed = self.popup_creation_failed(id);
                    self.navigation_status.forget(id);
                }
                if popup_failed {
                    self.message = format!(
                        "{error}. Closing the popup because its opener could not be preserved."
                    );
                } else if active {
                    self.message = format!("{error}. Select the tab to retry.");
                }
                if self.smoke {
                    self.outcome.store(2, Ordering::Release);
                    self.request_close(cx);
                }
            }
            BrowserEvent::NavigationCompleted {
                id,
                success,
                cancelled,
            } => {
                // A navigation that became a download ends with success false and no new
                // document (#143): neither a failure to count nor a visit to retract. The
                // mark was read by `navigation_status.observe` above and is spent here.
                let became_download =
                    self.navigation_status.finish_download_navigation(id) && !success;
                // A stopped or replaced navigation is not a failure to count.
                if !success && !cancelled && !became_download {
                    self.usage.recorder.error(ErrorCategory::NavigationFailed);
                }
                // History retracts a visit whose navigation failed.
                if !became_download
                    && let Some(tab) = self.model.tabs.iter().find(|tab| tab.id.get() == id)
                {
                    self.command(SessionCommand::ObserveNavigation {
                        id: tab.id,
                        success,
                    });
                }
                if self.smoke
                    && self
                        .model
                        .active_id
                        .is_some_and(|active| active.get() == id)
                {
                    self.smoke_navigation = success;
                    if !success {
                        self.outcome.store(2, Ordering::Release);
                        self.request_close(cx);
                    } else {
                        self.finish_smoke(cx);
                    }
                }
            }
            BrowserEvent::ProcessFailed { id, kind } => {
                if self.active_page_feedback(id) {
                    self.message = if kind == syrinx_platform::browser::RENDER_PROCESS_UNRESPONSIVE
                    {
                        "This page is not responding. You can wait or reload it.".into()
                    } else {
                        "A page component failed; the engine is handling its recovery.".into()
                    };
                }
            }
            BrowserEvent::Url { id, url } => {
                self.acceptance_observed(&url);
                self.page_site_changed(id, &url);
                self.forget_find(id, true);
                self.metadata(id, None, Some(url));
            }
            BrowserEvent::History { id, report } => self.history_reported(id, &report),
            BrowserEvent::Loading { .. } => {}
            BrowserEvent::DataCleared { request, outcome } => {
                self.browser_data_cleared(request, outcome);
            }
            BrowserEvent::PageText { id, outcome } => self.page_text_arrived(id, outcome),
            // Every page shortcut is a key the page handed over, so the ring may show (DW-62).
            BrowserEvent::Shortcut(shortcut) => {
                crate::controls::note_input(ModalityInput::PageKey, window, cx);
                self.shortcut(shortcut, window, cx);
            }
            BrowserEvent::PageFocused { id } => {
                self.end_palette_on_page_focus(id, cx);
                self.page_took_focus(id, window, cx);
            }
            BrowserEvent::PopupRequested { id, request, url } => {
                self.request_popup(id, request, url);
            }
            BrowserEvent::PopupBlocked { id } => {
                if self.active_page_feedback(id) {
                    self.message = "This popup could not be opened.".to_owned();
                }
            }
            BrowserEvent::Error { tab, error } => {
                // A failed page control may be a zoom Syrinx asked for; stop waiting for it.
                if let (Some(id), syrinx_platform::browser::BrowserError::PageControl) =
                    (tab, &error)
                {
                    self.site_zoom.abandon_apply(id);
                }
                if let Some(id) = mute::refused_mute(tab, &error) {
                    // Its own status line, not the generic error; the smoke check below still runs.
                    self.mute_refused(id);
                } else if tab.is_none() {
                    self.fail(error.to_string());
                } else if tab.is_some_and(|id| self.active_page_feedback(id)) {
                    self.message = error.to_string();
                }
                if self.smoke {
                    self.outcome.store(2, Ordering::Release);
                    self.request_close(cx);
                }
            }
            BrowserEvent::Stopped => {
                if !self.closing && self.failure.is_none() {
                    self.fail("The page host stopped. Restart Syrinx before continuing.");
                }
                self.browser_stopped = true;
                self.browser_ready = false;
                if let Some(browser) = self.browser.take()
                    && let Err(error) = browser.join()
                {
                    self.fail(error.to_string());
                }
                if self.closing && !self.session_stopped {
                    self.command(SessionCommand::Shutdown);
                }
                self.finish_close(cx);
            }
        }
        cx.notify();
    }

    fn active_page_feedback(&self, raw_id: u64) -> bool {
        !self.closing
            && navigation_status::owned_page(raw_id, &self.model, &self.loaded, &self.pending_pages)
                .is_some_and(|id| self.model.active_id == Some(id))
    }

    fn fail(&mut self, message: impl Into<String>) {
        // Normal loading and save notices must not conceal a stopped worker or engine failure.
        self.outcome.store(2, Ordering::Release);
        self.failure = Some(message.into());
    }

    /// As `fail`, but never replaces a reason already shown with a vaguer one (#9).
    fn fail_unless_failed(&mut self, message: impl Into<String>) {
        if self.failure.is_none() {
            self.fail(message);
        }
    }

    fn metadata(&mut self, id: u64, title: Option<String>, address: Option<String>) {
        let Some(tab) = self.model.tabs.iter().find(|tab| tab.id.get() == id) else {
            return;
        };
        self.command(SessionCommand::ObserveMetadata {
            id: tab.id,
            title,
            address,
        });
    }

    fn command(&mut self, command: SessionCommand) {
        self.try_command(command);
    }

    /// Queues a command for the session worker; returns whether it was queued.
    fn try_command(&mut self, command: SessionCommand) -> bool {
        self.queue_command(command, HomePaletteIntent::send)
    }

    /// Queues a command whose Home tab opens no palette, as the settings gear needs (#14).
    fn try_command_without_entry(&mut self, command: SessionCommand) -> bool {
        self.queue_command(command, HomePaletteIntent::send_without_entry)
    }

    /// Queues a command through `send`, which decides what its Home tab does to the palette.
    fn queue_command(&mut self, command: SessionCommand, send: HomeSend) -> bool {
        let ready = self.session_revision.is_some();
        let Some(session) = &self.session else {
            if session_command_allowed(self.closing, ready, &command) {
                self.message = "Local storage is unavailable.".to_owned();
            }
            return false;
        };
        match queue_session_command(
            self.closing,
            ready,
            &mut self.home_palette,
            &session.commands,
            command,
            send,
        ) {
            Ok(queued) => queued,
            Err(_) => {
                // The worker's Failure event, sent once its loop ends, replaces this with the reason.
                self.fail_unless_failed("The session worker stopped taking requests.");
                false
            }
        }
    }

    fn page_command(&mut self, command: BrowserCommand) -> bool {
        if !page_command_allowed(self.closing, &command) {
            return false;
        }
        if let Some(url) = loaded_address(&command)
            && !self.acceptance_permits(url)
        {
            return false;
        }
        let Some(browser) = &self.browser else {
            self.message = "The page host is unavailable.".to_owned();
            return false;
        };
        match browser.send(command) {
            Ok(()) => true,
            Err(error) => {
                self.fail(error.to_string());
                false
            }
        }
    }

    fn sync_pages(&mut self) {
        self.sync_pages_and_focus(true);
    }

    /// Shows the page the model wants; `focus_shown` gives a newly shown page the keys.
    fn sync_pages_and_focus(&mut self, focus_shown: bool) {
        if self.closing || self.session_revision.is_none() {
            return;
        }
        self.popup_pages
            .retain(|id| self.model.tabs.iter().any(|tab| tab.id == *id));
        if self.sync_suspended_fixture() {
            return;
        }
        let desired = self
            .navigation_intents
            .desired_page(&self.model, self.browser_ready, self.smoke)
            .or_else(|| self.blank_popup());
        if !self.browser_ready {
            return;
        }
        let removed = page_lifecycle::removed(&self.model, &self.loaded, &self.pending_pages);
        for id in removed {
            self.connections.remove(&id.get());
            self.forget_find(id.get(), false);
            downloads::forget_removed_page(&mut self.downloads, id);
            if self.page_command(BrowserCommand::Close { id: id.get() }) {
                self.loaded.remove(&id);
                self.page_gone(id);
                self.pending_pages.remove(&id);
            }
        }
        if let Some((id, url)) = &desired
            && !self.loaded.contains(id)
            && !self.pending_pages.contains(id)
        {
            let create = self.page_text.create_command(*id, url.clone());
            let queued = self.page_command(create);
            if queued {
                self.pending_pages.insert(*id);
                self.page_text.page_created(*id);
            }
            self.navigation_intents.creation_queued(*id, queued);
        }
        // The history list and clear dialog occupy the page card, so no page is shown beneath.
        let card_free = !self.history.is_open() && !self.clear_data.is_open();
        let next = desired
            .map(|(id, _)| id)
            .filter(|id| self.loaded.contains(id) && card_free);
        if next != self.shown && self.page_command(BrowserCommand::ShowActive(next.map(TabId::get)))
        {
            if let Some(previous) = self.shown {
                self.forget_find(previous.get(), true);
            }
            if let Some(id) = next {
                self.usage.page_shown(id);
            }
            self.shown = next;
            if focus_shown
                && self.palette.is_none()
                && self.peek.is_none()
                && let Some(id) = next
            {
                self.page_command(BrowserCommand::Focus { id: id.get() });
            }
        }
    }

    fn navigate(&mut self, text: String, force_search: bool) {
        let url = match navigation::destination(&text, force_search) {
            Ok(url) => url,
            Err(message) => {
                self.message = message.to_owned();
                return;
            }
        };
        // An acceptance run never records or loads anything beyond its loopback pages.
        if !self.acceptance_permits(&url) {
            return;
        }
        self.command(SessionCommand::NavigateActive { address: url });
    }

    fn navigate_committed(&mut self, id: TabId) {
        self.page_text.owner_navigated(id);
        let Some(tab) = self.model.tabs.iter().find(|tab| tab.id == id) else {
            self.fail("The acknowledged navigation has no live tab.");
            return;
        };
        let url = tab.metadata.address.clone();
        // The browser queue preserves Create before a second address submitted during loading.
        if self.loaded.contains(&id) || self.pending_pages.contains(&id) {
            self.page_command(BrowserCommand::Navigate { id: id.get(), url });
        } else {
            // Keep the committed destination until this tab can create its native page.
            self.navigation_intents.remember(id, url);
        }
    }

    /// Ctrl Tab or Ctrl Shift Tab (`by_key`), or the palette's Next and Previous tab commands.
    fn switch_tab(&mut self, backwards: bool, by_key: bool) {
        // The palette counts its own commands; a single tab has nowhere to switch to.
        if by_key && self.model.tabs.len() > 1 {
            self.count(Feature::TabSwitchedKeyboard);
        }
        self.usage.switch_requested(self.model.active_id, None);
        self.command(SessionCommand::CycleActive { backwards });
    }

    fn close_tab(&mut self) {
        self.command(SessionCommand::CloseActive);
    }

    fn shortcut(&mut self, shortcut: BrowserShortcut, window: &mut Window, cx: &mut Context<Self>) {
        self.browser_action(shortcut, true, window, cx);
    }

    /// Runs a browser shortcut from a key (`by_key`) or a palette command; only keys count as
    /// keyboard use (AN-01).
    fn browser_action(
        &mut self,
        shortcut: BrowserShortcut,
        by_key: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing {
            return;
        }
        // A fullscreen page fills the window, leaving the find bar and History nowhere to show,
        // so leave fullscreen first and then open them. The accelerator handler cannot know the
        // fullscreen state, so the key is already taken from the page by the time we get here.
        if self.fullscreen.is_some() && needs_chrome(shortcut) {
            self.fullscreen_shortcut(window);
        }
        match shortcut {
            BrowserShortcut::DismissPeek(generation) => {
                self.dismiss_peek(generation, window, cx);
            }
            BrowserShortcut::ToggleSidebar => self.toggle_sidebar(window, cx),
            BrowserShortcut::Settings => self.open_settings_from_page(window, cx),
            // Only a focused page sends these; GPUI's own F6 binding covers the chrome.
            BrowserShortcut::FocusNextPane => self.cycle_pane(Direction::Forward, true, window, cx),
            BrowserShortcut::FocusPreviousPane => {
                self.cycle_pane(Direction::Backward, true, window, cx)
            }
            BrowserShortcut::LeavePageBackward => self.leave_page_backward(window, cx),
            BrowserShortcut::Fullscreen => self.fullscreen_shortcut(window),
            BrowserShortcut::Find => self.open_find(window, cx),
            BrowserShortcut::FindNext => self.find_next_key(false, window, cx),
            BrowserShortcut::FindPrevious => self.find_next_key(true, window, cx),
            // From a page key, the palette or a menu, the keys were the page's to return.
            BrowserShortcut::History => {
                self.toggle_history(history_list::ReturnFocus::Page, window, cx);
            }
            BrowserShortcut::Downloads => self.toggle_downloads(window, cx),
            BrowserShortcut::Print
            | BrowserShortcut::ZoomIn
            | BrowserShortcut::ZoomOut
            | BrowserShortcut::ZoomReset => self.essential_shortcut(shortcut),
            BrowserShortcut::NewTab => self.command(SessionCommand::NewTab {
                address: "about:blank".to_owned(),
            }),
            BrowserShortcut::CloseTab => self.close_tab(),
            BrowserShortcut::Address if by_key => self.palette_key(true, window, cx),
            BrowserShortcut::Palette if by_key => self.palette_key(false, window, cx),
            BrowserShortcut::Address => self.open_palette(true, window, cx),
            BrowserShortcut::Palette => self.open_palette(false, window, cx),
            BrowserShortcut::NextTab => self.switch_tab(false, by_key),
            BrowserShortcut::PreviousTab => self.switch_tab(true, by_key),
            BrowserShortcut::ReopenTab => self.command(SessionCommand::ReopenClosed),
            BrowserShortcut::SavePage => self.command(SessionCommand::SaveActive),
            BrowserShortcut::Back => self.step_history(HistoryDirection::Back),
            BrowserShortcut::Forward => self.step_history(HistoryDirection::Forward),
            BrowserShortcut::Reload => {
                self.shown_page_action(Feature::NavReload, |id| BrowserCommand::Reload { id });
            }
        }
    }

    /// Ctrl L (`address`) or Ctrl K: counts which key opened the palette, then opens it.
    fn palette_key(&mut self, address: bool, window: &mut Window, cx: &mut Context<Self>) {
        // A key that only puts focus back in the open palette did not open it; a key in the
        // peek is counted once its request reaches the main window.
        if self.palette.is_none() && self.is_main_window(window) {
            self.count(if address {
                Feature::PaletteOpenedCtrlL
            } else {
                Feature::PaletteOpenedCtrlK
            });
        }
        self.open_palette(address, window, cx);
    }

    /// Back, Forward or Reload on the shown page, counted once whichever control asked (AN-01).
    fn shown_page_action(&mut self, feature: Feature, command: fn(u64) -> BrowserCommand) {
        let Some(id) = self.shown else {
            return;
        };
        self.count(feature);
        self.page_command(command(id.get()));
    }

    /// Counts how a palette entry ended: which kind of row was chosen, or none (AN-01).
    fn count_palette_outcome(&self, event: &PaletteEvent) {
        let usage = &self.usage.recorder;
        match event {
            PaletteEvent::Submit {
                text,
                force_search: false,
            } if import::is_import_command(text) => {
                // Not a registry command, so it is counted only as "other", never by name.
                usage.count(Feature::PaletteChoseCommand);
                usage.other_command();
            }
            PaletteEvent::Submit {
                force_search: true, ..
            } => usage.count(Feature::PaletteChoseWebSearch),
            // Plain Enter counts by where the text goes: an address, or a web search.
            PaletteEvent::Submit { text, .. } => {
                let goes = navigation::destination(text, false);
                match (goes, navigation::recognized_address(text)) {
                    (Ok(_), Ok(Some(_))) => usage.count(Feature::PaletteChoseAddress),
                    (Ok(_), Ok(None)) => usage.count(Feature::PaletteChoseWebSearch),
                    // A refused entry navigates nowhere, so nothing was chosen.
                    (Err(_), _) | (_, Err(_)) => {}
                }
            }
            PaletteEvent::Activate(Target::Tab(_)) => usage.count(Feature::PaletteChoseTab),
            PaletteEvent::Activate(Target::Saved(_)) => usage.count(Feature::PaletteChoseSaved),
            PaletteEvent::Activate(Target::History(_)) => {
                usage.count(Feature::PaletteChoseHistory);
            }
            PaletteEvent::Run(action) => {
                usage.count(Feature::PaletteChoseCommand);
                match action {
                    CommandAction::Browser(action) => usage.command(*action),
                    CommandAction::Plugin { .. } => usage.other_command(),
                }
            }
            PaletteEvent::Closed | PaletteEvent::ClickedAway => {
                usage.count(Feature::PaletteDismissed);
            }
            PaletteEvent::Activate(Target::Command(_))
            | PaletteEvent::Query { .. }
            | PaletteEvent::Shortcut(_) => {}
        }
    }

    fn palette_event(&mut self, event: PaletteEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.count_palette_outcome(&event);
        let finishes_entry = event.finishes_entry();
        if finishes_entry {
            // The popup is taken here; the arms below only act on the outcome.
            self.end_palette_entry();
        }
        match event {
            PaletteEvent::Query {
                request_id,
                text,
                scope,
            } => {
                self.query_sequence = self
                    .query_sequence
                    .checked_add(1)
                    .expect("search request IDs exhausted");
                self.pending_query = Some((self.query_sequence, request_id));
                self.acceptance_query_sent(request_id);
                self.command(SessionCommand::Query {
                    request_id: self.query_sequence,
                    policy: search_policy(&text, scope),
                    text,
                    scope,
                });
            }
            // `>import bookmarks` is the palette's one command until the command registry exists.
            // Alt+Enter (`force_search`) searches the words instead.
            PaletteEvent::Submit {
                text,
                force_search: false,
            } if import::is_import_command(&text) => {
                self.start_bookmark_import(window, cx);
            }
            PaletteEvent::Submit { text, force_search } => {
                self.navigate(text, force_search);
                self.focus_page(window, cx);
            }
            PaletteEvent::Activate(target) => {
                match target {
                    Target::Tab(id) => self.activate(ActivationTarget::Tab(id)),
                    Target::Saved(id) => self.activate(ActivationTarget::Saved(id)),
                    Target::History(id) => self.command(SessionCommand::OpenHistory(id)),
                    Target::Command(_) => {
                        self.message = "Commands run through their own row.".to_owned()
                    }
                }
                self.focus_page(window, cx);
            }
            PaletteEvent::Run(action) => {
                self.run_command(action, window, cx);
            }
            PaletteEvent::Shortcut(shortcut) => {
                let popup_live = self.palette_entry_live(cx);
                dispatch_palette_shortcut(shortcut, popup_live, |shortcut| {
                    self.shortcut(shortcut, window, cx);
                });
            }
            PaletteEvent::Closed => {
                self.focus_page(window, cx);
            }
            // The chrome press that ended the entry has already chosen where focus goes.
            PaletteEvent::ClickedAway => {}
        }
        if finishes_entry {
            self.peek_state.rearm();
            self.watch_peek(window, cx);
        }
        cx.notify();
    }

    /// The one path every surface uses to open a tab or saved page (D43): never a duplicate.
    fn activate(&mut self, target: ActivationTarget) {
        let command = activation_command(&self.model, target);
        match command {
            Some(command) => {
                // A saved page may open as a new tab, so its target is learned from the state.
                let target = match target {
                    ActivationTarget::Tab(id) => Some(id),
                    ActivationTarget::Saved(_) => None,
                };
                self.usage.switch_requested(self.model.active_id, target);
                self.command(command);
            }
            None => self.message = "That tab is no longer open.".to_owned(),
        }
    }

    fn run_command(&mut self, action: CommandAction, window: &mut Window, cx: &mut Context<Self>) {
        let CommandAction::Browser(action) = action else {
            self.message = "Plugin actions need the plugin runtime, which is not built yet.".into();
            self.focus_page(window, cx);
            return;
        };
        // Find keeps focus in its Syrinx input; activating natively now means no later
        // activation takes the keys back, and Syrinx never takes the foreground (2026-10-12).
        if action == BrowserAction::FindInPage {
            self.activate_main_now();
        }
        match browser_shortcut(action) {
            Some(shortcut) => self.browser_action(shortcut, false, window, cx),
            None if action == BrowserAction::ClearBrowsingData => self.open_clear_data(),
            None if action == BrowserAction::InstallUpdateFromFile => {
                self.install_update_from_file(window, cx);
            }
            None if action == BrowserAction::CheckForUpdates => self.check_for_updates(window, cx),
            None => {}
        }
        // Edit address reopens the palette, Find focuses its bar and History returns focus to
        // where it was opened from; the rest return to the page.
        if command_returns_focus_to_page(action) {
            self.focus_page(window, cx);
        }
    }

    /// What the command registry needs to know to enable or hide each command.
    fn command_context(&self) -> CommandContext {
        CommandContext {
            active_tab: self.model.active_id.is_some(),
            active_page: self.shown.is_some(),
            tab_count: self.model.tabs.len(),
        }
    }

    /// Whether `window` is the browser's main window rather than one of its popups.
    pub(super) fn is_main_window(&self, window: &Window) -> bool {
        Window::window_handle(window) == self.main_window
    }

    fn focus_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let main = self.is_main_window(window);
        // Clear chrome focus so no control keeps its ring once keys go to the page.
        if main {
            window.focus(&self.focus, cx);
        }
        // GPUI's `activate_window` focuses the window later, from a task, which takes the keys
        // back from a page focused now; it also forces the foreground. With Syrinx in front,
        // activate natively at once instead.
        if self.activate_main_now() {
            self.acceptance_mark("focus_page: activated at once");
            self.page_focus_on_activation = false;
            self.focus_shown_page();
            return;
        }
        // Syrinx never takes the foreground from another application (owner, 2026-10-12): the
        // page takes the keys when the owner returns to Syrinx.
        self.acceptance_mark("focus_page: deferred until Syrinx is activated");
        self.page_focus_on_activation = true;
    }

    fn activate_main_now(&mut self) -> bool {
        match syrinx_platform::browser::activate_if_foreground(self.parent_hwnd) {
            Ok(activated) => activated,
            Err(error) => {
                self.message = error.to_string();
                false
            }
        }
    }

    /// Completes a `focus_page` that waited for the owner to bring Syrinx back, unless the
    /// owner has since put the keys in a chrome control.
    fn focus_page_after_activation(&mut self, window: &Window, cx: &App) {
        let deferred = std::mem::take(&mut self.page_focus_on_activation);
        let root_focused = window.focused(cx).map(|focused| focused == self.focus);
        if !deferred_focus_applies(deferred, root_focused) {
            return;
        }
        self.acceptance_mark("activation observed");
        self.focus_shown_page();
    }

    fn focus_shown_page(&mut self) {
        // A page the card does not show (Home, History, clear data, a crash card) gets no keys.
        if !self.page_card_shows_web_page() {
            self.move_keys_to_chrome();
            return;
        }
        if let Some(id) = self.shown {
            self.acceptance_mark("page focus sent");
            self.page_command(BrowserCommand::Focus { id: id.get() });
        }
    }

    fn active_address(&self) -> String {
        self.model
            .tabs
            .iter()
            .find(|tab| Some(tab.id) == self.model.active_id)
            .map_or_else(String::new, |tab| {
                if self.tab_is_home(tab) {
                    String::new()
                } else {
                    tab.metadata.address.clone()
                }
            })
    }

    /// Whether a tab shows Home; an adopted popup's blank page is a page, not Home.
    fn tab_is_home(&self, tab: &TabSnapshot) -> bool {
        let adopted = self.popup_pages.contains(&tab.id);
        tab.metadata
            .is_home(PageOrigin::from_adopted_popup(adopted))
    }

    /// Runs the orderly shutdown synchronously when GPUI quits with workers still running, as
    /// when Windows ends the session: GPUI exits the process as soon as quit handlers return.
    /// After a normal close both workers are already gone and this does nothing.
    fn stop_workers_on_quit(&mut self) {
        if self.browser.is_none() && self.session.is_none() {
            return;
        }
        self.closing = true;
        let deadline = Instant::now() + QUIT_BOUND;
        // Both workers stop together: no page event can reach storage while this thread waits.
        if let Some(session) = &self.session {
            session.request_shutdown();
        }
        if let Some(browser) = self.browser.take() {
            self.browser_stopped = true;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if let Err(error) = browser.shutdown_within(remaining) {
                eprintln!("The page host did not stop cleanly as Syrinx quit: {error}");
            }
        }
        if let Some(session) = self.session.take() {
            self.session_stopped = true;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if let Err(error) = session.shutdown_within(remaining) {
                eprintln!("Local storage did not stop cleanly as Syrinx quit: {error}");
            }
        }
    }

    pub fn request_close(&mut self, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        // An update installs only after its D13 snapshot, so closing waits for it.
        if self.hold_close_for_update_snapshot() {
            cx.notify();
            return;
        }
        self.closing = true;
        self.stop_link_feed();
        self.retire_peek(cx);
        self.retire_menu(cx);
        self.remove_palette(cx);
        self.close_import_preview(cx);
        self.message = "Saving and closing…".to_owned();
        if self.browser_stopped && !self.session_stopped {
            self.command(SessionCommand::Shutdown);
        }
        if !self.browser_stopped {
            self.page_command(BrowserCommand::Shutdown);
        }
        self.finish_close(cx);
        cx.notify();
    }

    pub fn smoke_timeout(&mut self, cx: &mut Context<Self>) {
        if !self.closing {
            self.outcome.store(2, Ordering::Release);
            self.request_close(cx);
        }
    }

    fn finish_smoke(&mut self, cx: &mut Context<Self>) {
        if self.smoke
            && self.smoke_navigation
            && !self.smoke_waiting
            && !self.closing
            && self
                .model
                .active_id
                .is_some_and(|id| self.loaded.contains(&id))
            && self.outcome.load(Ordering::Acquire) == 0
        {
            self.smoke_waiting = true;
            cx.spawn(async move |view, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(2))
                    .await;
                if view
                    .update(cx, |view, cx| {
                        if !view.closing && view.outcome.load(Ordering::Acquire) == 0 {
                            view.outcome.store(1, Ordering::Release);
                            view.request_close(cx);
                        }
                    })
                    .is_err()
                {
                    eprintln!("Smoke window closed during its runtime health check.");
                }
            })
            .detach();
        }
    }

    fn finish_close(&self, cx: &mut Context<Self>) {
        if self.closing && self.session_stopped && self.browser_stopped {
            cx.quit();
        }
    }

    /// Keeps the page card beside the sidebar; render calls this on every drag step,
    /// window resize and DPI change.
    fn resize_pages(&mut self, window: &Window) {
        let bounds = page_card::page_card_bounds(self.card_frame(window));
        if self.bounds != Some(bounds) && self.page_command(BrowserCommand::Resize(bounds)) {
            self.bounds = Some(bounds);
        }
    }

    /// The window and chrome sizes the page card is placed from.
    fn card_frame(&self, window: &Window) -> page_card::CardFrame {
        let viewport = window.viewport_size();
        page_card::CardFrame {
            viewport_width: f32::from(viewport.width),
            viewport_height: f32::from(viewport.height),
            sidebar_width: self.sidebar_sizing.width(),
            notice_height: self.notice_height(),
            permission_height: self.permission_height(),
            find_height: self.find_height(),
            fullscreen: self.fullscreen.is_some(),
            scale: window.scale_factor(),
        }
    }

    /// The title bar's contents: navigation above the sidebar, as the canvas draws it, or
    /// with the address when the sidebar is narrow or hidden; Downloads and ☰ in the right
    /// corner. A right-click on either group opens the page menu.
    fn title_contents(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let corner = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .on_mouse_down(MouseButton::Right, page_menu_listener(cx))
            .child(self.essential_controls(cx))
            .child(
                icon_button(div().id("page-menu"), glyph::MENU, "Page menu")
                    .occlude()
                    .on_click(cx.listener(|this, _, window, cx| this.open_page_menu(window, cx))),
            );
        div()
            .size_full()
            .min_w_0()
            .pr(px(title_bar::CAPTION_GAP))
            .flex()
            .items_center()
            .child(
                self.navigation(window, cx)
                    .on_mouse_down(MouseButton::Right, page_menu_listener(cx)),
            )
            // Left empty so the window can still be dragged by its title bar.
            .child(div().flex_1().min_w(px(64.)).h_full())
            .child(corner)
    }

    /// The navigation group, the address pane F6 visits first: over a wide sidebar, the
    /// canvas's window row (sidebar toggle, then back, forward and reload at the right);
    /// otherwise those buttons followed by the address field.
    fn navigation(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let toggle = icon_button(div().id("toggle-sidebar"), glyph::SIDEBAR, "Hide sidebar")
            .aria_keyshortcuts("Control+Alt+S")
            .occlude()
            .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)));
        let back = icon_button(div().id("back"), glyph::BACK, "Back")
            .aria_keyshortcuts("Alt+ArrowLeft")
            .occlude();
        let forward = icon_button(div().id("forward"), glyph::FORWARD, "Forward")
            .aria_keyshortcuts("Alt+ArrowRight")
            .occlude();
        let buttons = [
            self.history_button(back, HistoryDirection::Back, cx),
            self.history_button(forward, HistoryDirection::Forward, cx),
            self.navigation_button(
                ("reload", glyph::RELOAD, "Reload", "Control+R"),
                (Feature::NavReload, |id| BrowserCommand::Reload { id }),
                cx,
            ),
        ];
        let row = div()
            .h(px(TITLE_BAR_HEIGHT))
            .flex()
            .items_center()
            .gap(px(2.));
        let group = if self.sidebar_sizing.labels_visible() {
            // The title bar pads 12 px, as the sidebar does, so the row spans the sidebar.
            let width = self.sidebar_sizing.width() - 12. - SIDEBAR_PADDING_RIGHT;
            row.w(px(width.max(0.)))
                .flex_none()
                .child(toggle)
                .child(div().flex_1().h_full())
                .children(buttons)
        } else {
            let width =
                page_card::titlebar_navigation_width(f32::from(window.viewport_size().width));
            row.max_w(px(width))
                .flex_1()
                .min_w_0()
                .child(toggle)
                .children(buttons)
                .child(div().w(px(6.)).flex_none())
                .child(self.address_field(false, true, cx))
        };
        self.as_pane(group, Pane::Address, gpui::Role::Toolbar)
    }

    /// Back, Forward or Reload: its id, glyph, name and keys, and the use it counts (AN-01)
    /// with the command it sends.
    fn navigation_button(
        &self,
        (id, icon, name, keys): (&'static str, &'static str, &'static str, &'static str),
        (feature, command): (Feature, fn(u64) -> BrowserCommand),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        icon_button(div().id(id), icon, name)
            .aria_keyshortcuts(keys)
            .occlude()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.shown_page_action(feature, command);
                cx.notify();
            }))
    }

    /// The address, which opens the palette to edit it; on a blank page, the canvas's hint
    /// with its key. The sidebar draws the canvas's taller field. Only one drawing may track
    /// the field's focus handle, so the peek's copy passes `track: false`.
    pub(super) fn address_field(
        &self,
        in_sidebar: bool,
        track: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        // Shown and announced decoded where safe, host kept as punycode; Ctrl L edits the raw address.
        let readable = syrinx_core::readable_address(&self.active_address());
        let palette = theme::current();
        let (height, corner) = if in_sidebar {
            (38., 10.)
        } else {
            (28., radius::CONTROL)
        };
        let mut field = div()
            .id(if in_sidebar {
                "sidebar-address"
            } else {
                "address"
            })
            .role(gpui::Role::Button)
            .aria_label("Search or enter address")
            .aria_value(readable.clone())
            .aria_description(self.connection_description())
            .aria_keyshortcuts("Control+L")
            .occlude()
            .min_w_0()
            .flex_none()
            .h(px(height))
            .pl_3()
            .pr_2()
            .gap(px(10.))
            .rounded(px(corner))
            .bg(theme::rgba(palette.field))
            .shadow(theme::shadow(syrinx_ui::ShadowRole::Field))
            .flex()
            .items_center()
            .overflow_hidden()
            .cursor_text()
            .text_size(px(syrinx_ui::metrics::text::BODY))
            .on_click(cx.listener(|this, _, window, cx| this.open_palette(true, window, cx)));
        // The title bar's field takes the row's free width; the sidebar's is a full-width row.
        field = if in_sidebar {
            field.w_full()
        } else {
            field.flex_1()
        };
        if track {
            field = field.track_focus(self.address_field_focus()).focus(
                crate::controls::focus_ring_keeping(syrinx_ui::ShadowRole::Field),
            );
        }
        if !readable.is_empty() {
            return field
                .children(self.connection_mark())
                .child(div().min_w_0().truncate().child(readable));
        }
        field
            .text_color(theme::rgba(palette.text_muted))
            .child(
                div()
                    .flex_none()
                    .font_family(glyph::FONT)
                    .text_size(px(12.))
                    .child(glyph::SEARCH),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child("Search or enter address"),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(5.))
                    .rounded(px(radius::KEY_HINT))
                    .border_1()
                    .border_b_2()
                    .border_color(theme::rgba(palette.key))
                    .text_size(px(syrinx_ui::metrics::text::CAPTION))
                    .text_color(theme::rgba(palette.icon_dim))
                    .child("Ctrl K"),
            )
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.usage.frame_drawn();
        if self.session_revision.is_none() {
            return crate::controls::watch_pointer(div())
                .size_full()
                .relative()
                .flex()
                .flex_col()
                .child(page_card::window_fill_layer(None, window, cx))
                .child(title_bar(div(), window.is_maximized()))
                .child(self.startup_panel(cx));
        }
        if self.browser_ready && !self.closing {
            self.resize_pages(window);
        }
        // A fullscreen page fills the window; no title bar, toolbar, sidebar or footer is drawn.
        if self.fullscreen.is_some() {
            return div()
                .key_context("SyrinxFullscreen")
                .size_full()
                .track_focus(&self.focus)
                .on_action(cx.listener(|this, _: &Fullscreen, window, cx| {
                    this.fullscreen_shortcut(window);
                    cx.notify();
                }))
                .on_action(
                    cx.listener(|this, _: &fullscreen::ExitPageFullscreen, window, cx| {
                        this.fullscreen_shortcut(window);
                        cx.notify();
                    }),
                );
        }
        // The history list takes the page card's place until it is closed; a crashed page
        // shows its recovery card there.
        let page = if self.clear_data.is_open() {
            div()
                .flex_1()
                .h_full()
                .child(clear_data::render(&self.clear_data, cx))
        } else if self.history.is_open() {
            div()
                .flex_1()
                .h_full()
                .child(history_list::render(&self.history, cx))
        } else if let Some(card) = self.crashed_page(cx) {
            card
        } else if self.shown.is_none() && self.settings_showing() {
            self.settings_page(window, cx)
        } else if self.shown.is_none() {
            self.home_page(window, cx)
        } else {
            div().flex_1().h_full()
        };
        // The web view draws in the card once the browser is up, so the fill leaves it clear.
        let page_drawn = self.browser_ready && !self.closing && self.page_card_shows_web_page();
        let card = page_drawn.then(|| page_card::card_edges(self.card_frame(window)));
        let sidebar = self.sidebar(window, cx);
        let end_palette_on_press = self.end_palette_on_chrome_press(cx);
        let chrome = crate::controls::watch_pointer_and(div(), end_palette_on_press)
            .size_full()
            .relative()
            .key_context("Syrinx")
            .child(page_card::window_fill_layer(card, window, cx))
            .text_color(theme::text())
            .text_sm()
            .font_family("Segoe UI Variable Text")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            // A click in Syrinx's chrome decides focus itself, so a deferred page focus lapses.
            .capture_any_mouse_down(cx.listener(|this, _, _, _| {
                this.page_focus_on_activation = false;
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.sidebar_drag_moved(event, window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.sidebar_drag_released(f32::from(event.position.x), window, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.sidebar_drag_released(f32::from(event.position.x), window, cx);
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| {
                this.toggle_sidebar(window, cx);
            }))
            .on_action(cx.listener(|this, _: &settings_page::OpenSettings, _, cx| {
                this.open_settings(None);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &DismissPeek, window, cx| {
                // Escape closes the topmost of an open menu, the peek and the history list. A menu
                // still opening has no keys yet; Escape withdraws it.
                let peek = this.peek.as_ref().map(|peek| peek.generation);
                match history_list::chrome_escape(this.menu.is_some(), peek, this.history.is_open())
                {
                    ChromeEscape::Menu => {
                        this.retire_menu(cx);
                        cx.notify();
                    }
                    ChromeEscape::Peek(generation) => this.dismiss_peek(generation, window, cx),
                    ChromeEscape::History => {
                        this.close_history(window, cx);
                        cx.notify();
                    }
                    ChromeEscape::Nothing => cx.propagate(),
                }
            }))
            .on_action(cx.listener(|this, _: &NewTab, _, cx| {
                this.command(SessionCommand::NewTab {
                    address: "about:blank".to_owned(),
                });
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CloseTab, _, cx| {
                this.close_tab();
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &Address, window, cx| this.palette_key(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Palette, window, cx| this.palette_key(false, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NextTab, _, cx| {
                this.switch_tab(false, true);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PreviousTab, _, cx| {
                this.switch_tab(true, true);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ReopenTab, _, cx| {
                this.command(SessionCommand::ReopenClosed);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SavePage, _, cx| {
                this.command(SessionCommand::SaveActive);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Back, _, _| {
                this.step_history(HistoryDirection::Back);
            }))
            .on_action(cx.listener(|this, _: &Forward, _, _| {
                this.step_history(HistoryDirection::Forward);
            }))
            .on_action(cx.listener(|this, _: &Reload, _, _| {
                this.shown_page_action(Feature::NavReload, |id| BrowserCommand::Reload { id });
            }))
            .on_action(cx.listener(|this, _: &ShowHistory, window, cx| {
                let return_focus = this.chrome_return_focus(window, cx);
                this.toggle_history(return_focus, window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ClearBrowsingData, _, cx| {
                this.open_clear_data();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &InstallUpdate, window, cx| {
                this.install_update_from_file(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CheckUpdates, window, cx| {
                this.check_for_updates(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Fullscreen, window, cx| {
                this.fullscreen_shortcut(window);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &panes::FocusNextPane, window, cx| {
                this.cycle_pane(Direction::Forward, false, window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &panes::FocusPreviousPane, window, cx| {
                    this.cycle_pane(Direction::Backward, false, window, cx);
                }),
            )
            .on_action(cx.listener(|_, _: &panes::FocusNextControl, window, cx| {
                window.focus_next(cx);
            }))
            .on_action(
                cx.listener(|_, _: &panes::FocusPreviousControl, window, cx| {
                    window.focus_prev(cx);
                }),
            )
            .child(title_bar(
                self.title_contents(window, cx),
                window.is_maximized(),
            ))
            .children(self.bar_pane(
                self.runtime_banner(cx),
                Pane::RuntimeNotice,
                gpui::Role::Status,
            ))
            .children(self.bar_pane(self.permission_bar(cx), Pane::Permission, gpui::Role::Alert))
            .children(self.bar_pane(self.find_bar(window, cx), Pane::Find, gpui::Role::Search))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .min_h_0()
                    .child(self.as_pane(sidebar, Pane::Sidebar, gpui::Role::Navigation))
                    .child(self.sidebar_edge(cx))
                    .child(self.as_pane(
                        div().flex_1().min_w_0().h_full().flex().child(page),
                        Pane::Page,
                        gpui::Role::Group,
                    )),
            )
            .child(
                div()
                    .id("status")
                    .role(gpui::Role::Status)
                    .aria_label(self.failure.as_ref().unwrap_or(&self.message).clone())
                    .h(px(FOOTER_HEIGHT))
                    .px_2()
                    .bg(theme::ground())
                    .flex()
                    .items_center()
                    .text_size(px(syrinx_ui::metrics::text::CAPTION))
                    .line_height(px(FOOTER_HEIGHT))
                    .text_color(theme::icon())
                    // One line only; a long message ends in an ellipsis, and its label keeps it whole.
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(self.failure.as_ref().unwrap_or(&self.message).clone()),
                    ),
            );
        self.essential_actions(chrome, cx)
    }
}

/// Opens the page menu where the owner right-clicks the title bar.
fn page_menu_listener(
    cx: &mut Context<BrowserView>,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    cx.listener(|this, event: &MouseDownEvent, window, cx| {
        cx.stop_propagation();
        this.open_menu_at(menus::MenuKind::Page, event, window, cx);
    })
}

fn dispatch_palette_shortcut(
    shortcut: BrowserShortcut,
    popup_live: bool,
    dispatch: impl FnOnce(BrowserShortcut),
) {
    // Enter or Escape may remove the popup before its earlier refocus request is delivered.
    // Keep the retained popup identity so its queued completion can still reach the browser.
    if !popup_live
        && matches!(
            shortcut,
            BrowserShortcut::Address | BrowserShortcut::Palette
        )
    {
        return;
    }
    dispatch(shortcut);
}

fn session_command_allowed(closing: bool, ready: bool, command: &SessionCommand) -> bool {
    // Drain final native metadata before Shutdown, but accept no new browsing or recovery intent.
    if closing
        && !matches!(
            command,
            SessionCommand::ObserveMetadata { .. } | SessionCommand::Shutdown
        )
    {
        return false;
    }
    ready
        || matches!(
            command,
            SessionCommand::ConfirmRecovery { .. }
                | SessionCommand::ConfirmHistoryRepair { .. }
                | SessionCommand::Shutdown
        )
}

/// How a command reaches the session worker: `HomePaletteIntent::send` or `send_without_entry`.
type HomeSend = fn(
    &mut HomePaletteIntent,
    &std::sync::mpsc::Sender<SessionCommand>,
    SessionCommand,
) -> Result<(), std::sync::mpsc::SendError<SessionCommand>>;

fn queue_session_command(
    closing: bool,
    ready: bool,
    home: &mut HomePaletteIntent,
    sender: &std::sync::mpsc::Sender<SessionCommand>,
    command: SessionCommand,
    send: HomeSend,
) -> Result<bool, std::sync::mpsc::SendError<SessionCommand>> {
    if !session_command_allowed(closing, ready, &command) {
        return Ok(false);
    }
    send(home, sender, command)?;
    Ok(true)
}

/// The address a command loads into a page, which an acceptance run must check first.
fn loaded_address(command: &BrowserCommand) -> Option<&str> {
    match command {
        BrowserCommand::Create { url, .. }
        | BrowserCommand::Reopen { url, .. }
        | BrowserCommand::Navigate { url, .. } => Some(url),
        _ => None,
    }
}

fn page_command_allowed(closing: bool, command: &BrowserCommand) -> bool {
    !closing || matches!(command, BrowserCommand::Shutdown)
}

/// A labelled chrome button, reachable with Tab and named by its label; glyph buttons
/// override the name with `aria_label`.
fn button(id: impl Into<gpui::ElementId>, label: &'static str) -> Stateful<Div> {
    crate::controls::text_button(div().id(id), label)
}

/// Whether a deferred page focus still applies on activation: only while GPUI focus is on
/// the window root (`Some(true)`) or nowhere (`None`), not on a control the owner chose.
fn deferred_focus_applies(deferred: bool, root_focused: Option<bool>) -> bool {
    deferred && root_focused != Some(false)
}

impl BrowserView {
    /// Moving to a monitor with another scale (WM_DPICHANGED) resizes the window, so the
    /// bounds watch re-sizes the taskbar and title-bar icons for the new DPI here.
    fn refresh_icon_for_scale(&mut self, window: &Window) {
        let scale = window.scale_factor();
        if scale == self.icon_scale || self.parent_hwnd == 0 {
            return;
        }
        self.icon_scale = scale;
        show_app_icon(self.parent_hwnd);
    }
}

/// Gives a Syrinx window the hawk icon at its DPI. If that fails, GPUI's window class still
/// shows the same icon at the default size, so the failure is reported but not fatal.
fn show_app_icon(hwnd: isize) {
    if let Err(error) = syrinx_platform::app_icon::apply(hwnd) {
        eprintln!("{error}");
    }
}

pub(crate) fn native_handle(window: &Window) -> Result<isize, &'static str> {
    let handle =
        HasWindowHandle::window_handle(window).map_err(|_| "The native window is unavailable.")?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
        _ => Err("Syrinx requires a Windows window."),
    }
}

fn search_policy(text: &str, scope: Scope) -> SearchPolicy {
    // These are preview tuning values; the core policy remains an explicit caller input.
    // A chip or prefix narrows the rows, so it may show more of them.
    let limit = if scope == Scope::Everything { 10 } else { 24 };
    SearchPolicy {
        limit,
        max_typos: u16::from(text.chars().count() >= 4),
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 120,
            frequency: 30,
            recency: 30,
            recency_window_secs: NonZeroU64::new(604_800).expect("positive recency window"),
        },
    }
}

#[cfg(test)]
mod palette_shortcut_tests {
    use futures::FutureExt;

    use super::*;

    #[test]
    fn a_deferred_page_focus_yields_to_a_control_the_owner_chose() {
        assert!(
            deferred_focus_applies(true, Some(true)),
            "the root still has focus"
        );
        assert!(deferred_focus_applies(true, None), "nothing has focus");
        assert!(
            !deferred_focus_applies(true, Some(false)),
            "a clicked chrome control keeps the keys"
        );
        assert!(
            !deferred_focus_applies(false, Some(true)),
            "nothing was deferred"
        );
        assert!(!deferred_focus_applies(false, None));
    }

    #[test]
    fn saved_activation_reaches_the_worker_even_from_a_stale_snapshot() {
        let saved = syrinx_core::SavedPageId::new(3).expect("synthetic saved page");
        let tab = TabId::new(5).expect("synthetic tab");
        // An empty snapshot stands in for one taken before a queued close or save landed.
        let stale = TabModelSnapshot::default();
        assert!(matches!(
            activation_command(&stale, ActivationTarget::Saved(saved)),
            Some(SessionCommand::OpenSaved(id)) if id == saved
        ));
        assert!(activation_command(&stale, ActivationTarget::Tab(tab)).is_none());
    }

    #[test]
    fn every_registry_command_runs_a_distinct_existing_shortcut() {
        let mut shortcuts = Vec::new();
        for (_, command) in CommandRegistry::browser().iter() {
            let CommandAction::Browser(action) = command.action else {
                panic!("built-in commands are browser actions");
            };
            // Clear data and updates run directly, not through a page shortcut.
            let Some(shortcut) = browser_shortcut(action) else {
                assert!(matches!(
                    action,
                    BrowserAction::ClearBrowsingData
                        | BrowserAction::InstallUpdateFromFile
                        | BrowserAction::CheckForUpdates
                ));
                continue;
            };
            assert!(
                !shortcuts.contains(&shortcut),
                "two commands share a handler"
            );
            shortcuts.push(shortcut);
        }
        // Every native shortcut except the palette, fullscreen, Settings and the window's
        // sidebar and peek keys is reachable as a command: ten navigation actions, history and
        // six page essentials.
        assert_eq!(shortcuts.len(), 17);
        assert!(shortcuts.contains(&BrowserShortcut::History));
        for window_key in [
            BrowserShortcut::Palette,
            BrowserShortcut::Fullscreen,
            BrowserShortcut::ToggleSidebar,
            BrowserShortcut::Settings,
        ] {
            assert!(!shortcuts.contains(&window_key));
        }
    }

    #[test]
    fn history_from_the_palette_keeps_the_focus_it_set() {
        assert!(!command_returns_focus_to_page(BrowserAction::ShowHistory));
        assert!(!command_returns_focus_to_page(BrowserAction::FindInPage));
        assert!(!command_returns_focus_to_page(BrowserAction::EditAddress));
        assert!(
            !command_returns_focus_to_page(BrowserAction::ShowDownloads),
            "the downloads flyout takes the keys"
        );
        assert!(command_returns_focus_to_page(BrowserAction::ZoomIn));
    }

    #[test]
    fn only_find_history_and_settings_leave_page_fullscreen_first() {
        assert!(needs_chrome(BrowserShortcut::Find));
        assert!(needs_chrome(BrowserShortcut::FindNext));
        assert!(needs_chrome(BrowserShortcut::FindPrevious));
        assert!(needs_chrome(BrowserShortcut::History));
        assert!(needs_chrome(BrowserShortcut::Settings));
        for shortcut in [
            BrowserShortcut::ZoomIn,
            BrowserShortcut::ZoomOut,
            BrowserShortcut::ZoomReset,
            BrowserShortcut::Fullscreen,
            BrowserShortcut::CloseTab,
        ] {
            assert!(!needs_chrome(shortcut));
        }
    }

    #[test]
    fn removed_popup_refocus_does_not_invalidate_queued_completion() {
        for shortcut in [BrowserShortcut::Address, BrowserShortcut::Palette] {
            for terminal in [
                PaletteEvent::Submit {
                    text: "https://queued.example.test/".to_owned(),
                    force_search: false,
                },
                PaletteEvent::Submit {
                    text: "synthetic search".to_owned(),
                    force_search: true,
                },
                PaletteEvent::Activate(Target::Tab(TabId::new(1).expect("synthetic tab"))),
                PaletteEvent::Run(CommandAction::Browser(BrowserAction::NewTab)),
                PaletteEvent::Closed,
                PaletteEvent::ClickedAway,
            ] {
                let (sender, mut receiver) = unbounded();
                sender
                    .unbounded_send(PaletteEvent::Shortcut(shortcut))
                    .expect("queue refocus before completion");
                sender.unbounded_send(terminal).expect("queue completion");
                drop(sender);
                let popup_id = 7;
                let mut retained_popup = Some(popup_id);
                let mut completed = false;
                while let Some(event) = receiver.next().now_or_never().expect("queued event") {
                    // The browser rejects an event once its originating popup identity is lost.
                    if retained_popup != Some(popup_id) {
                        break;
                    }
                    match event {
                        PaletteEvent::Shortcut(shortcut) => {
                            dispatch_palette_shortcut(shortcut, false, |_| {
                                // A stale open_palette call would retire this identity too early.
                                retained_popup = None;
                            });
                        }
                        PaletteEvent::Submit { .. }
                        | PaletteEvent::Activate(_)
                        | PaletteEvent::Run(_)
                        | PaletteEvent::Closed
                        | PaletteEvent::ClickedAway => {
                            completed = true;
                            retained_popup = None;
                        }
                        PaletteEvent::Query { .. } => panic!("unexpected query in refocus race"),
                    }
                }
                assert!(
                    completed,
                    "queued completion must survive the obsolete refocus"
                );
                assert_eq!(retained_popup, None);
            }
        }
    }

    #[test]
    fn popup_liveness_only_suppresses_obsolete_refocus_actions() {
        let ordinary = [
            BrowserShortcut::NewTab,
            BrowserShortcut::CloseTab,
            BrowserShortcut::NextTab,
            BrowserShortcut::PreviousTab,
            BrowserShortcut::ReopenTab,
            BrowserShortcut::SavePage,
            BrowserShortcut::Back,
            BrowserShortcut::Forward,
            BrowserShortcut::Reload,
        ];
        for popup_live in [false, true] {
            let mut dispatched = Vec::new();
            for shortcut in ordinary {
                dispatch_palette_shortcut(shortcut, popup_live, |shortcut| {
                    dispatched.push(shortcut);
                });
            }
            assert_eq!(dispatched, ordinary);
        }
        let mut refocused = Vec::new();
        for shortcut in [BrowserShortcut::Address, BrowserShortcut::Palette] {
            dispatch_palette_shortcut(shortcut, true, |shortcut| {
                refocused.push(shortcut);
            });
        }
        assert_eq!(
            refocused,
            [BrowserShortcut::Address, BrowserShortcut::Palette]
        );
    }
}

#[cfg(test)]
mod row_close_key_tests {
    use gpui::{Action, KeyContext, Keymap, Keystroke};

    use super::*;

    /// The actions Ctrl W runs, in the order GPUI tries them, under these key contexts.
    fn ctrl_w(contexts: &[&str]) -> Vec<Box<dyn Action>> {
        let keymap = Keymap::new(window_bindings());
        let contexts: Vec<KeyContext> = contexts
            .iter()
            .map(|context| KeyContext::parse(context).expect("synthetic key context"))
            .collect();
        let keystroke = Keystroke::parse("ctrl-w").expect("ctrl-w parses");
        let (bindings, pending) = keymap.bindings_for_input(&[keystroke], &contexts);
        assert!(!pending, "no longer binding starts with Ctrl W");
        bindings
            .iter()
            .map(|binding| binding.action().boxed_clone())
            .collect()
    }

    #[test]
    fn ctrl_w_on_a_focused_row_closes_the_rows_page_first() {
        for window in ["Syrinx", "SyrinxPeek"] {
            let actions = ctrl_w(&[window, folder_editor::ROW_CONTEXT]);
            assert!(
                actions[0].partial_eq(&folder_editor::CloseRowPage),
                "{window}: {actions:?}"
            );
        }
    }

    #[test]
    fn ctrl_w_without_a_focused_row_closes_the_active_page() {
        // The page path (WebView2's accelerator) never reaches the keymap; these are the
        // chrome's own keys when the window root or another control has focus.
        for contexts in [&["Syrinx"][..], &["Syrinx", "SyrinxFind"], &[]] {
            let actions = ctrl_w(contexts);
            assert!(
                actions[0].partial_eq(&CloseTab),
                "{contexts:?}: {actions:?}"
            );
            assert!(
                !actions
                    .iter()
                    .any(|action| action.partial_eq(&folder_editor::CloseRowPage)),
                "{contexts:?}"
            );
        }
    }
}

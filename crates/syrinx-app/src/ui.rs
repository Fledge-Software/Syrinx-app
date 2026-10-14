//! Connects the shared session to browser chrome without running storage or search on the UI thread.

mod clear_data;
mod customize;
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
mod navigation_intents;
mod navigation_status;
mod page_card;
mod page_lifecycle;
mod palette_acceptance;
mod palette_popup;
mod panes;
mod peek;
mod peek_state;
mod permissions;
mod popups;
mod process_recovery;
mod recovery;
mod runtime_notice;
mod saved_moves;
mod security;
mod sidebar_rows;
mod sidebar_view;
mod site_permissions;
mod suspended_fixture;
mod tile_editor;
mod tiles;
mod title_bar;

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
};

use futures::{StreamExt, channel::mpsc::unbounded};
use gpui::StatefulInteractiveElement;
use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext, Bounds, Context, Div, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyBinding, MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement, Render, Stateful, Styled,
    Subscription, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions, actions, div,
    point, px, rgb, size,
};
use gpui_component::Root;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use syrinx_core::{
    ActivationTarget, BrowserAction, CommandAction, CommandContext, CommandRegistry, Route,
    SavedFolderId, TabId, TabModelSnapshot, route,
};
use syrinx_platform::browser::{
    BrowserCommand, BrowserEvent, BrowserHost, BrowserShortcut, PageBounds, set_overlay_owner,
};
use syrinx_platform::instance::LinkFeed;
use syrinx_search::{BlankQuery, BoostWeights, Scope, SearchPolicy, Target};
use syrinx_ui::{
    SidebarSizing,
    focus::{Direction, Pane},
    metrics::chrome::TITLE_BAR_HEIGHT,
};
use title_bar::title_bar;

pub(crate) use panes::{FocusNextControl, FocusPreviousControl};

use home_palette::HomePaletteIntent;
use navigation_intents::NavigationIntents;
use navigation_status::NavigationStatus;

use crate::{
    navigation,
    palette::{self, PaletteEvent, PaletteView},
    session::{self, SessionCommand, SessionEvent, SessionHandle},
    theme,
};

const TOOLBAR_HEIGHT: f32 = 48.;
const FOOTER_HEIGHT: f32 = 28.;

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
        ToggleSidebar,
        DismissPeek,
        Fullscreen
    ]
);

/// Binds each browser command's shortcut from the command registry, plus Ctrl K for the palette.
pub fn bind_keys(cx: &mut App) {
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
    // The sidebar toggle and peek dismissal are window keys, not palette commands.
    bindings.extend([
        KeyBinding::new("ctrl-alt-s", ToggleSidebar, None),
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
    cx.bind_keys(bindings);
    crate::palette::bind_keys(cx);
    import::bind_keys(cx);
    essentials::bind_zoom_keys(cx);
    find::bind_keys(cx);
    folder_editor::bind_keys(cx);
    panes::bind_keys(cx);
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

/// Runs a browser command through the same handler as its keyboard shortcut; history and clear
/// browsing data have no native page shortcut, so they open their page-card views directly.
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
        BrowserAction::ShowHistory | BrowserAction::ClearBrowsingData => return None,
    })
}

/// How this launch runs: normally, or as one of the unattended synthetic checks.
pub struct LaunchChecks {
    pub smoke: bool,
    /// P1-15's measurement mode: this many synthetic tabs suspended in a fresh profile.
    pub suspended_tabs: usize,
    /// P1-33's palette acceptance run.
    pub acceptance: Option<crate::acceptance::AcceptancePlan>,
}

pub struct BrowserView {
    fullscreen: Option<(u64, bool)>,
    crashes: process_recovery::CrashState,
    fullscreen_pages: fullscreen::FullscreenPages,
    suspended_fixture: suspended_fixture::SuspendedFixture,
    connections: std::collections::HashMap<u64, syrinx_platform::browser::ConnectionState>,
    find: Option<find::FindBar>,
    find_sequence: u64,
    zoom_levels: std::collections::HashMap<u64, f64>,
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
    shown: Option<TabId>,
    bounds: Option<PageBounds>,
    sidebar_sizing: SidebarSizing,
    collapsed_folders: HashSet<SavedFolderId>,
    /// The expanded folders as last stored in settings (P1-21).
    expanded_saved: BTreeSet<SavedFolderId>,
    folder_editor: Option<folder_editor::FolderEditor>,
    sidebar_edge_focus: FocusHandle,
    panes: panes::PaneFocus,
    /// Ends a sidebar drag when the window deactivates; dropping it unsubscribes.
    _drag_activation_watch: Subscription,
    message: String,
    failure: Option<String>,
    focus: FocusHandle,
    /// The open palette entry; `None` while the kept popup is hidden.
    palette: Option<palette_popup::PalettePopup>,
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
    /// Keeps keyboard focus as Customize's permission list opens, closes and rereads (DW-7).
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
    outcome: Arc<AtomicU8>,
}

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
        } = checks;
        let mut view = Self {
            fullscreen: None,
            crashes: process_recovery::CrashState::default(),
            fullscreen_pages: fullscreen::FullscreenPages::default(),
            suspended_fixture: suspended_fixture::SuspendedFixture::new(suspended_tabs),
            connections: std::collections::HashMap::new(),
            find: None,
            find_sequence: 0,
            zoom_levels: std::collections::HashMap::new(),
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
            shown: None,
            bounds: None,
            sidebar_sizing: SidebarSizing::default(),
            collapsed_folders: HashSet::new(),
            expanded_saved: BTreeSet::new(),
            folder_editor: None,
            // A tracked handle takes its Tab stop from the handle, not from `tab_index`.
            sidebar_edge_focus: cx.focus_handle().tab_stop(true),
            panes: panes::PaneFocus::new(cx),
            _drag_activation_watch: cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.sidebar_drag_abandoned(window, cx);
                } else {
                    // Returning from our own popup must preserve Escape's edge dismissal.
                    if !this.peek_monitoring {
                        this.peek_state.rearm();
                    }
                    this.watch_peek(window, cx);
                    this.focus_page_after_activation(window, cx);
                }
            }),
            message: "Opening your workspace…".to_owned(),
            failure: None,
            focus: cx.focus_handle(),
            palette: None,
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
            outcome,
        };
        window.focus(&view.focus, cx);
        match native_handle(window) {
            Ok(handle) => {
                view.parent_hwnd = handle;
                show_app_icon(handle);
            }
            Err(error) => {
                view.fail(error);
                return view;
            }
        }
        view.start_session(data_dir.join("State"), window, cx);
        view.start_browser(data_dir.join("WebView2"), window, cx);
        if let Some(links) = links {
            view.start_link_feed(links, window, cx);
        }
        view.start_acceptance(window, cx);
        view
    }

    fn start_session(&mut self, directory: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let started = if self.acceptance.is_some() {
            session::spawn_acceptance(directory)
        } else {
            session::spawn(directory)
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
                if view
                    .update_in(cx, |view, window, cx| view.session_event(event, window, cx))
                    .is_err()
                {
                    return;
                }
            }
            if view
                .update_in(cx, |view, window, cx| {
                    if !view.session_stopped {
                        view.fail("The session worker stopped unexpectedly.");
                        view.session_event(SessionEvent::Stopped, window, cx);
                    }
                })
                .is_err()
            {
                eprintln!("Session event stream closed after its window was removed.");
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
                if view
                    .update_in(cx, |view, window, cx| view.browser_event(event, window, cx))
                    .is_err()
                {
                    return;
                }
            }
            if view
                .update_in(cx, |view, window, cx| {
                    if !view.browser_stopped {
                        view.fail("The page host stopped unexpectedly.");
                        view.browser_event(BrowserEvent::Stopped, window, cx);
                    }
                })
                .is_err()
            {
                eprintln!("Browser event stream closed after its window was removed.");
            }
        })
        .detach();
    }

    fn session_event(&mut self, event: SessionEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            SessionEvent::RecoveryOffered(prompt) => {
                if !self.closing {
                    self.recovery = Some(recovery::RecoveryCard::new(prompt));
                    self.message =
                        "A verified backup is available. Confirm before continuing recovery."
                            .to_owned();
                }
            }
            SessionEvent::HistoryRepairOffered(prompt) => {
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
            } => {
                self.runtime_settings_loaded(&settings);
                self.restore_folder_expansion(&settings);
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
                self.model = snapshot.model;
                self.save_folder_expansion();
                self.crashes.retain(&self.model);
                self.zoom_levels
                    .retain(|id, _| self.model.tabs.iter().any(|tab| tab.id.get() == *id));
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
                self.sync_pages();
                // Work that waited for the session to accept commands.
                self.open_forwarded_links();
                self.save_runtime_version();
                self.restore_fullscreen_if_hidden(window, cx);
                if initial {
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
                let unattended =
                    self.smoke || self.suspended_fixture.enabled() || self.acceptance.is_some();
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
            SessionEvent::Stopped => {
                self.abandon_import_write();
                self.home.site_permissions.session_lost();
                if !self.closing && self.failure.is_none() {
                    self.fail("Local storage stopped. Restart Syrinx before continuing.");
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
                self.page_crashed(id, window);
                self.recovery_focus(window, cx);
            }
            BrowserEvent::PageRestored { id } => {
                self.crashes.restored(id);
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
            BrowserEvent::ZoomChanged { id, factor } => {
                self.zoom_levels.insert(id, factor);
            }
            BrowserEvent::FindCompleted { id, request, count } => {
                self.find_completed(id, request, count)
            }
            BrowserEvent::DownloadDialog { .. } => {}
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
                self.resize_pages(window);
                self.sync_pages();
            }
            BrowserEvent::RuntimeVersion(version) => self.runtime_version_observed(version),
            BrowserEvent::Title { id, title } => {
                self.acceptance_title(id, &title);
                self.metadata(id, Some(title), None);
            }
            BrowserEvent::CloseRequested { id } => {
                if let Some(command) = page_lifecycle::observed_close(id, self.closing) {
                    self.command(command);
                }
            }
            BrowserEvent::Created { id } => {
                if let Ok(id) = TabId::new(id)
                    && page_lifecycle::created(id, &mut self.pending_pages, &mut self.loaded)
                {
                    self.sync_pages();
                    self.finish_smoke(cx);
                }
            }
            BrowserEvent::CreationFailed { id, error } => {
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
            BrowserEvent::NavigationCompleted { id, success } => {
                // History retracts a visit whose navigation failed.
                if let Some(tab) = self.model.tabs.iter().find(|tab| tab.id.get() == id) {
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
                self.forget_find(id, true);
                self.metadata(id, None, Some(url));
            }
            BrowserEvent::Loading { .. } => {}
            BrowserEvent::DataCleared { request, outcome } => {
                self.browser_data_cleared(request, outcome);
            }
            // The app never switches capture on before P1-26, so no capture reaches here.
            BrowserEvent::PageText { .. } => {}
            BrowserEvent::Shortcut(shortcut) => self.shortcut(shortcut, window, cx),
            BrowserEvent::PageFocused { id } => self.page_took_focus(id, window, cx),
            BrowserEvent::PopupRequested { id, request, url } => {
                self.request_popup(id, request, url);
            }
            BrowserEvent::PopupBlocked { id } => {
                if self.active_page_feedback(id) {
                    self.message = "This popup could not be opened.".to_owned();
                }
            }
            BrowserEvent::Error { tab, error } => {
                if tab.is_none() {
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
        ) {
            Ok(queued) => queued,
            Err(_) => {
                self.fail("The session worker is unavailable.");
                false
            }
        }
    }

    fn page_command(&mut self, command: BrowserCommand) -> bool {
        if !page_command_allowed(self.closing, &command) {
            return false;
        }
        if let BrowserCommand::Create { url, .. } | BrowserCommand::Navigate { url, .. } = &command
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
            if self.page_command(BrowserCommand::Close { id: id.get() }) {
                self.loaded.remove(&id);
                self.pending_pages.remove(&id);
            }
        }
        if let Some((id, url)) = &desired
            && !self.loaded.contains(id)
            && !self.pending_pages.contains(id)
        {
            let queued = self.page_command(BrowserCommand::Create {
                id: id.get(),
                url: url.clone(),
            });
            if queued {
                self.pending_pages.insert(*id);
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
            self.shown = next;
            if self.palette.is_none()
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

    fn switch_tab(&mut self, backwards: bool) {
        self.command(SessionCommand::CycleActive { backwards });
    }

    fn close_tab(&mut self) {
        self.command(SessionCommand::CloseActive);
    }

    /// Shows recent history in the page card, or returns to the active page.
    fn toggle_history(&mut self) {
        if self.history.is_open() {
            self.history.close();
        } else if !self.closing && self.session_revision.is_some() {
            let read = self.history.open();
            self.command(read);
        }
        self.sync_pages();
    }

    fn shortcut(&mut self, shortcut: BrowserShortcut, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        match shortcut {
            BrowserShortcut::DismissPeek(generation) => {
                self.dismiss_peek(generation, window, cx);
            }
            BrowserShortcut::ToggleSidebar => self.toggle_sidebar(window, cx),
            // Only a focused page sends these; GPUI's own F6 binding covers the chrome.
            BrowserShortcut::FocusNextPane => self.cycle_pane(Direction::Forward, true, window, cx),
            BrowserShortcut::FocusPreviousPane => {
                self.cycle_pane(Direction::Backward, true, window, cx)
            }
            BrowserShortcut::LeavePageBackward => self.leave_page_backward(window, cx),
            BrowserShortcut::Fullscreen => self.fullscreen_shortcut(window),
            BrowserShortcut::Find => self.open_find(window, cx),
            BrowserShortcut::Downloads
            | BrowserShortcut::Print
            | BrowserShortcut::ZoomIn
            | BrowserShortcut::ZoomOut
            | BrowserShortcut::ZoomReset => self.essential_shortcut(shortcut),
            BrowserShortcut::NewTab => self.command(SessionCommand::NewTab {
                address: "about:blank".to_owned(),
            }),
            BrowserShortcut::CloseTab => self.close_tab(),
            BrowserShortcut::Address => self.open_palette(true, window, cx),
            BrowserShortcut::Palette => self.open_palette(false, window, cx),
            BrowserShortcut::NextTab => self.switch_tab(false),
            BrowserShortcut::PreviousTab => self.switch_tab(true),
            BrowserShortcut::ReopenTab => self.command(SessionCommand::ReopenClosed),
            BrowserShortcut::SavePage => self.command(SessionCommand::SaveActive),
            BrowserShortcut::Back => {
                if let Some(id) = self.shown {
                    self.page_command(BrowserCommand::Back { id: id.get() });
                }
            }
            BrowserShortcut::Forward => {
                if let Some(id) = self.shown {
                    self.page_command(BrowserCommand::Forward { id: id.get() });
                }
            }
            BrowserShortcut::Reload => {
                if let Some(id) = self.shown {
                    self.page_command(BrowserCommand::Reload { id: id.get() });
                }
            }
        }
    }

    fn palette_event(&mut self, event: PaletteEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let finishes_entry = event.finishes_entry();
        if finishes_entry {
            self.pending_query = None;
            self.home_palette.finish_entry();
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
                self.palette = None;
                self.start_bookmark_import(window, cx);
            }
            PaletteEvent::Submit { text, force_search } => {
                self.palette = None;
                self.navigate(text, force_search);
                self.focus_page(window, cx);
            }
            PaletteEvent::Activate(target) => {
                self.palette = None;
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
                self.palette = None;
                self.run_command(action, window, cx);
            }
            PaletteEvent::Shortcut(shortcut) => {
                let popup_live = self.palette_entry_live(cx);
                dispatch_palette_shortcut(shortcut, popup_live, |shortcut| {
                    self.shortcut(shortcut, window, cx);
                });
            }
            PaletteEvent::Closed => {
                self.palette = None;
                self.focus_page(window, cx);
            }
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
            Some(command) => self.command(command),
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
            Some(shortcut) => self.shortcut(shortcut, window, cx),
            None if action == BrowserAction::ClearBrowsingData => self.open_clear_data(),
            None => self.toggle_history(),
        }
        // Edit address reopens the palette and Find focuses its bar; the rest return to the page.
        if !matches!(
            action,
            BrowserAction::EditAddress | BrowserAction::FindInPage
        ) {
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

    fn focus_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let main = Window::window_handle(window) == self.main_window;
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
                if tab.metadata.address == "about:blank" && !self.popup_pages.contains(&tab.id) {
                    String::new()
                } else {
                    tab.metadata.address.clone()
                }
            })
    }

    pub fn request_close(&mut self, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.stop_link_feed();
        self.retire_peek(cx);
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
        let viewport = window.viewport_size();
        let bounds = page_card::page_card_bounds(page_card::CardFrame {
            viewport_width: f32::from(viewport.width),
            viewport_height: f32::from(viewport.height),
            sidebar_width: self.sidebar_sizing.width(),
            notice_height: self.notice_height(),
            permission_height: self.permission_height(),
            find_height: self.find_height(),
            fullscreen: self.fullscreen.is_some(),
            scale: window.scale_factor(),
        });
        if self.bounds != Some(bounds) && self.page_command(BrowserCommand::Resize(bounds)) {
            self.bounds = Some(bounds);
        }
    }

    /// The navigation toolbar: the address pane F6 visits first.
    fn toolbar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        // Shown and announced decoded where safe, host kept as punycode; Ctrl L edits the raw address.
        let readable = syrinx_core::readable_address(&self.active_address());
        let bar = div()
            .w_full()
            .min_w_0()
            .flex_none()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .h(px(if self.sidebar_sizing.width() == 0. {
                TITLE_BAR_HEIGHT
            } else {
                TOOLBAR_HEIGHT
            }))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .bg(theme::ground())
            .child(
                button("back", "‹")
                    .aria_label("Back")
                    .aria_keyshortcuts("Alt+ArrowLeft")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(id) = this.shown {
                            this.page_command(BrowserCommand::Back { id: id.get() });
                        }
                        cx.notify();
                    })),
            )
            .child(
                button("forward", "›")
                    .aria_label("Forward")
                    .aria_keyshortcuts("Alt+ArrowRight")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(id) = this.shown {
                            this.page_command(BrowserCommand::Forward { id: id.get() });
                        }
                        cx.notify();
                    })),
            )
            .child(
                button("reload", "↻")
                    .aria_label("Reload")
                    .aria_keyshortcuts("Control+R")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(id) = this.shown {
                            this.page_command(BrowserCommand::Reload { id: id.get() });
                        }
                        cx.notify();
                    })),
            )
            .child(self.connection_display())
            .child(
                div()
                    .id("address")
                    .role(gpui::Role::Button)
                    .aria_label("Search or enter address")
                    .aria_value(readable.clone())
                    .aria_keyshortcuts("Control+L")
                    .track_focus(self.address_field_focus())
                    .focus(crate::controls::focus_ring)
                    .min_w_0()
                    .flex_1()
                    .h(px(34.))
                    .px_3()
                    .rounded_md()
                    .bg(theme::line())
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .cursor_pointer()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_palette(true, window, cx)),
                    )
                    .child(if readable.is_empty() {
                        "Search or enter address · Ctrl K".to_owned()
                    } else {
                        readable
                    }),
            )
            .child(self.essential_controls(cx))
            .child(
                button("save", "Save page").on_click(cx.listener(|this, _, _, cx| {
                    this.command(SessionCommand::SaveActive);
                    cx.notify();
                })),
            )
            .child(
                button("history", "History").on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_history();
                    cx.notify();
                })),
            );
        self.as_pane(bar, Pane::Address, gpui::Role::Toolbar)
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.session_revision.is_none() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(title_bar(div().child("Syrinx"), window.is_maximized()))
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
        } else if self.shown.is_none() {
            self.home_page(cx)
        } else {
            div().flex_1().h_full()
        };
        let chrome = div()
            .size_full()
            .key_context("Syrinx")
            .bg(theme::window_fill())
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
            .on_action(cx.listener(|this, _: &DismissPeek, window, cx| {
                if let Some(generation) = this.peek.as_ref().map(|peek| peek.generation) {
                    this.dismiss_peek(generation, window, cx);
                } else {
                    cx.propagate();
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
                cx.listener(|this, _: &Address, window, cx| this.open_palette(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Palette, window, cx| this.open_palette(false, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NextTab, _, cx| {
                this.switch_tab(false);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PreviousTab, _, cx| {
                this.switch_tab(true);
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
                if let Some(id) = this.shown {
                    this.page_command(BrowserCommand::Back { id: id.get() });
                }
            }))
            .on_action(cx.listener(|this, _: &Forward, _, _| {
                if let Some(id) = this.shown {
                    this.page_command(BrowserCommand::Forward { id: id.get() });
                }
            }))
            .on_action(cx.listener(|this, _: &Reload, _, _| {
                if let Some(id) = this.shown {
                    this.page_command(BrowserCommand::Reload { id: id.get() });
                }
            }))
            .on_action(cx.listener(|this, _: &ShowHistory, _, cx| {
                this.toggle_history();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ClearBrowsingData, _, cx| {
                this.open_clear_data();
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
                if self.sidebar_sizing.width() == 0. {
                    div()
                        .w(px(page_card::titlebar_navigation_width(f32::from(
                            window.viewport_size().width,
                        ))))
                        .min_w_0()
                        .child(self.toolbar(cx))
                } else {
                    div().h_full().flex().items_center().child("Syrinx")
                },
                window.is_maximized(),
            ))
            .when(self.sidebar_sizing.width() > 0., |root| {
                root.child(self.toolbar(cx))
            })
            .children(self.bar_pane(
                self.runtime_banner(cx),
                Pane::RuntimeNotice,
                gpui::Role::Status,
            ))
            .children(self.bar_pane(self.permission_bar(cx), Pane::Permission, gpui::Role::Alert))
            .children(self.bar_pane(self.find_bar(cx), Pane::Find, gpui::Role::Search))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .min_h_0()
                    .child(self.as_pane(self.sidebar(cx), Pane::Sidebar, gpui::Role::Navigation))
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
                    .px_3()
                    .bg(theme::ground())
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(theme::icon())
                    .child(self.failure.as_ref().unwrap_or(&self.message).clone()),
            );
        self.essential_actions(chrome, cx)
    }
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

fn queue_session_command(
    closing: bool,
    ready: bool,
    home: &mut HomePaletteIntent,
    sender: &std::sync::mpsc::Sender<SessionCommand>,
    command: SessionCommand,
) -> Result<bool, std::sync::mpsc::SendError<SessionCommand>> {
    if !session_command_allowed(closing, ready, &command) {
        return Ok(false);
    }
    home.send(sender, command)?;
    Ok(true)
}

fn page_command_allowed(closing: bool, command: &BrowserCommand) -> bool {
    !closing || matches!(command, BrowserCommand::Shutdown)
}

/// A labelled chrome button, reachable with Tab and named by its label; glyph buttons
/// override the name with `aria_label`.
fn button(id: impl Into<gpui::ElementId>, label: &'static str) -> Stateful<Div> {
    let frame = div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(theme::line())
        .cursor_pointer()
        .child(label);
    crate::controls::push_button(frame, label)
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

fn native_handle(window: &Window) -> Result<isize, &'static str> {
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
            // History and clear data open in the page card, not through a page shortcut.
            let Some(shortcut) = browser_shortcut(action) else {
                assert!(matches!(
                    action,
                    BrowserAction::ShowHistory | BrowserAction::ClearBrowsingData
                ));
                continue;
            };
            assert!(
                !shortcuts.contains(&shortcut),
                "two commands share a handler"
            );
            shortcuts.push(shortcut);
        }
        // Every native shortcut except the palette, fullscreen and the window's sidebar and
        // peek keys is reachable as a command: ten navigation actions and six page essentials.
        assert_eq!(shortcuts.len(), 16);
        for window_key in [
            BrowserShortcut::Palette,
            BrowserShortcut::Fullscreen,
            BrowserShortcut::ToggleSidebar,
        ] {
            assert!(!shortcuts.contains(&window_key));
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
                        | PaletteEvent::Closed => {
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

//! Connects the shared session to browser chrome without running storage or search on the UI thread.

mod customize;
mod favorites;
mod home;
mod home_palette;
mod navigation_intents;
mod navigation_status;
mod page_card;
mod page_lifecycle;
mod peek;
mod peek_state;
mod recovery;
mod sidebar_rows;
mod sidebar_view;
mod tile_editor;
mod tiles;
mod title_bar;

#[cfg(test)]
mod shutdown_tests;

use std::{
    cell::RefCell,
    collections::HashSet,
    num::NonZeroU64,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

use atlas_core::{SavedFolderId, TabId, TabModelSnapshot};
use atlas_platform::browser::{
    BrowserCommand, BrowserEvent, BrowserHost, BrowserShortcut, PageBounds, set_overlay_owner,
};
use atlas_search::{BlankQuery, BoostWeights, Scope, SearchPolicy, Target};
use atlas_ui::{SidebarSizing, metrics::chrome::TITLE_BAR_HEIGHT};
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
use title_bar::title_bar;

use home_palette::HomePaletteIntent;
use navigation_intents::NavigationIntents;
use navigation_status::NavigationStatus;

use crate::{
    navigation,
    palette::{PaletteEvent, PaletteView},
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
        ToggleSidebar,
        DismissPeek
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-t", NewTab, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("ctrl-l", Address, None),
        KeyBinding::new("ctrl-k", Palette, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
        KeyBinding::new("ctrl-shift-t", ReopenTab, None),
        KeyBinding::new("ctrl-d", SavePage, None),
        KeyBinding::new("alt-left", Back, None),
        KeyBinding::new("alt-right", Forward, None),
        KeyBinding::new("ctrl-r", Reload, None),
        KeyBinding::new("ctrl-alt-s", ToggleSidebar, None),
        KeyBinding::new("escape", DismissPeek, Some("Atlas")),
        KeyBinding::new("escape", crate::palette::Dismiss, Some("AtlasPeek")),
    ]);
    crate::palette::bind_keys(cx);
}

struct PalettePopup {
    window: WindowHandle<Root>,
    view: Entity<PaletteView>,
}

pub struct BrowserView {
    model: TabModelSnapshot,
    session_revision: Option<u64>,
    recovery: Option<recovery::RecoveryCard>,
    session: Option<SessionHandle>,
    browser: Option<BrowserHost>,
    session_stopped: bool,
    browser_stopped: bool,
    browser_ready: bool,
    closing: bool,
    loaded: HashSet<TabId>,
    pending_pages: HashSet<TabId>,
    navigation_intents: NavigationIntents,
    navigation_status: NavigationStatus,
    shown: Option<TabId>,
    bounds: Option<PageBounds>,
    sidebar_sizing: SidebarSizing,
    collapsed_folders: HashSet<SavedFolderId>,
    sidebar_edge_focus: FocusHandle,
    /// Ends a sidebar drag when the window deactivates; dropping it unsubscribes.
    _drag_activation_watch: Subscription,
    message: String,
    failure: Option<String>,
    focus: FocusHandle,
    palette: Option<PalettePopup>,
    main_window: gpui::AnyWindowHandle,
    peek: Option<peek::PeekPopup>,
    peek_state: peek_state::PeekState,
    peek_monitoring: bool,
    peek_generation: u64,
    _peek_bounds_watch: Subscription,
    home_palette: HomePaletteIntent,
    home: home::HomeState,
    parent_hwnd: isize,
    query_sequence: u64,
    pending_query: Option<(u64, u64)>,
    smoke: bool,
    smoke_navigation: bool,
    smoke_waiting: bool,
    outcome: Arc<AtomicU8>,
}

impl BrowserView {
    pub fn new(
        data_dir: PathBuf,
        smoke: bool,
        outcome: Arc<AtomicU8>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            model: TabModelSnapshot::default(),
            session_revision: None,
            recovery: None,
            session: None,
            browser: None,
            session_stopped: true,
            browser_stopped: true,
            browser_ready: false,
            closing: false,
            loaded: HashSet::new(),
            pending_pages: HashSet::new(),
            navigation_intents: NavigationIntents::default(),
            navigation_status: NavigationStatus::default(),
            shown: None,
            bounds: None,
            sidebar_sizing: SidebarSizing::default(),
            collapsed_folders: HashSet::new(),
            sidebar_edge_focus: cx.focus_handle(),
            _drag_activation_watch: cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.sidebar_drag_abandoned(cx);
                }
            }),
            message: "Opening your workspace…".to_owned(),
            failure: None,
            focus: cx.focus_handle(),
            palette: None,
            main_window: Window::window_handle(window),
            peek: None,
            peek_state: peek_state::PeekState::default(),
            peek_monitoring: false,
            peek_generation: 0,
            _peek_bounds_watch: cx.observe_window_bounds(window, |this, window, cx| {
                let was_open = this.peek.is_some();
                this.retire_peek(cx);
                if was_open && window.is_window_active() {
                    this.focus_page(window);
                }
            }),
            home_palette: HomePaletteIntent::default(),
            home: home::HomeState::default(),
            parent_hwnd: 0,
            query_sequence: 0,
            pending_query: None,
            smoke,
            smoke_navigation: false,
            smoke_waiting: false,
            outcome,
        };
        window.focus(&view.focus, cx);
        match native_handle(window) {
            Ok(handle) => view.parent_hwnd = handle,
            Err(error) => {
                view.fail(error);
                return view;
            }
        }
        view.start_session(data_dir.join("State"), window, cx);
        view.start_browser(data_dir.join("WebView2"), window, cx);
        view
    }

    fn start_session(&mut self, directory: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let (session, mut events) = match session::spawn(directory) {
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
            SessionEvent::Preferences {
                settings,
                destinations,
            } => self.restore_preferences(&settings, destinations, window, cx),
            SessionEvent::Destinations(destinations) => self.destinations_changed(destinations),
            SessionEvent::State(snapshot) => {
                self.recovery = None;
                // Restored sessions already have saved revisions; only the first state is startup.
                let previous_revision = self.session_revision.replace(snapshot.revision);
                let initial = previous_revision.is_none();
                let changed = previous_revision != Some(snapshot.revision);
                let saved_message = if initial
                    && !snapshot.model.tabs.is_empty()
                    && snapshot.previous_shutdown == atlas_store::ShutdownState::Unclean
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
                self.model = snapshot.model;
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
                if let Some(id) = snapshot.navigate
                    && !self.closing
                {
                    self.navigate_committed(id);
                }
                self.sync_pages();
                // Only committed model changes invalidate an open palette's catalogue snapshot.
                if changed
                    && !self.closing
                    && let Some(palette) = &self.palette
                {
                    self.pending_query = None;
                    palette.view.update(cx, |palette, cx| palette.refresh(cx));
                }
                if self.smoke && self.model.tabs.is_empty() && !self.closing {
                    self.command(SessionCommand::NewTab {
                        address: "about:blank".to_owned(),
                    });
                }
                if new_home && !self.closing && !self.smoke {
                    self.open_palette(false, window, cx);
                }
            }
            SessionEvent::Search { request_id, result } => {
                if let Some((pending, local)) = self.pending_query
                    && pending == request_id
                    && let Some(palette) = &self.palette
                {
                    let failed = result.is_err();
                    let accepted = palette
                        .view
                        .update(cx, |palette, cx| palette.receive(local, result, cx));
                    if self.smoke && accepted && failed {
                        self.outcome.store(2, Ordering::Release);
                    }
                }
            }
            SessionEvent::Failure { error, fatal } => {
                if self.smoke {
                    self.outcome.store(2, Ordering::Release);
                }
                if fatal {
                    self.fail(format!("Storage stopped: {error}"));
                } else {
                    self.message = error.to_string();
                }
            }
            SessionEvent::Stopped => {
                if !self.closing && self.failure.is_none() {
                    self.fail("Local storage stopped. Restart Atlas before continuing.");
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
            BrowserEvent::Ready => {
                self.browser_ready = true;
                self.resize_pages(window);
                self.sync_pages();
            }
            BrowserEvent::Title { id, title } => self.metadata(id, Some(title), None),
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
                if let Ok(id) = TabId::new(id) {
                    self.pending_pages.remove(&id);
                    self.navigation_status.forget(id);
                }
                if active {
                    self.message = format!("{error}. Select the tab to retry.");
                }
                if self.smoke {
                    self.outcome.store(2, Ordering::Release);
                    self.request_close(cx);
                }
            }
            BrowserEvent::NavigationCompleted { id, success } => {
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
            BrowserEvent::ProcessFailed { kind, .. } => {
                self.fail(format!(
                    "The page engine reported failure {kind}. Restart Atlas if the page does not recover."
                ));
                if self.smoke {
                    self.request_close(cx);
                }
            }
            BrowserEvent::Url { id, url } => self.metadata(id, None, Some(url)),
            BrowserEvent::Loading { .. } => {}
            BrowserEvent::Shortcut(shortcut) => self.shortcut(shortcut, window, cx),
            BrowserEvent::PopupBlocked { id } => {
                if self.active_page_feedback(id) {
                    self.message = "This preview cannot open this site's popup yet.".to_owned();
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
                    self.fail("The page host stopped. Restart Atlas before continuing.");
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
        let ready = self.session_revision.is_some();
        let Some(session) = &self.session else {
            if session_command_allowed(self.closing, ready, &command) {
                self.message = "Local storage is unavailable.".to_owned();
            }
            return;
        };
        if queue_session_command(
            self.closing,
            ready,
            &mut self.home_palette,
            &session.commands,
            command,
        )
        .is_err()
        {
            self.fail("The session worker is unavailable.");
        }
    }

    fn page_command(&mut self, command: BrowserCommand) -> bool {
        if !page_command_allowed(self.closing, &command) {
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
        let desired =
            self.navigation_intents
                .desired_page(&self.model, self.browser_ready, self.smoke);
        if !self.browser_ready {
            return;
        }
        let removed = page_lifecycle::removed(&self.model, &self.loaded, &self.pending_pages);
        for id in removed {
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
        let next = desired
            .map(|(id, _)| id)
            .filter(|id| self.loaded.contains(id));
        if next != self.shown && self.page_command(BrowserCommand::ShowActive(next.map(TabId::get)))
        {
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

    fn shortcut(&mut self, shortcut: BrowserShortcut, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        match shortcut {
            BrowserShortcut::DismissPeek(generation) => {
                self.dismiss_peek(generation, window, cx);
            }
            BrowserShortcut::ToggleSidebar => self.toggle_sidebar(window, cx),
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

    fn open_palette(&mut self, address: bool, window: &mut Window, cx: &mut Context<Self>) {
        if Window::window_handle(window) != self.main_window {
            self.queue_peek_action(peek::PeekAction::Palette(address), window, cx);
            return;
        }
        self.retire_peek(cx);
        if self.closing || self.session_revision.is_none() {
            return;
        }
        if let Some(palette) = &self.palette {
            let view = palette.view.clone();
            if palette
                .window
                .update(cx, |_, popup, cx| {
                    popup.activate_window();
                    view.update(cx, |palette, cx| palette.focus_input(popup, cx));
                })
                .is_err()
            {
                self.palette = None;
                self.pending_query = None;
                self.home_palette.finish_entry();
                self.message =
                    "The palette window is no longer available. Open it again.".to_owned();
                cx.notify();
            }
            return;
        }
        // A new popup restarts its local request IDs; results from its predecessor are obsolete.
        self.pending_query = None;
        let initial = if address {
            self.active_address()
        } else {
            String::new()
        };
        let (events, mut receiver) = unbounded();
        let parent = self.parent_hwnd;
        let retained = Rc::new(RefCell::new(None));
        let popup_retained = retained.clone();
        let ownership_error = Rc::new(RefCell::new(None));
        let popup_error = ownership_error.clone();
        let main_bounds = window.bounds();
        let width = f32::from(main_bounds.size.width).min(680.);
        let popup_bounds = Bounds::new(
            point(
                main_bounds.origin.x + px((f32::from(main_bounds.size.width) - width) / 2.),
                main_bounds.origin.y + px(100.),
            ),
            size(px(width), px(480.)),
        );
        let popup = cx.open_window(
            WindowOptions {
                kind: WindowKind::PopUp,
                titlebar: None,
                window_bounds: Some(WindowBounds::Windowed(popup_bounds)),
                focus: true,
                ..Default::default()
            },
            move |popup, cx| {
                popup.set_window_title("Atlas command palette");
                let ownership = native_handle(popup)
                    .map_err(str::to_owned)
                    .and_then(|handle| {
                        set_overlay_owner(handle, parent).map_err(|error| error.to_string())
                    });
                if let Err(error) = ownership {
                    *popup_error.borrow_mut() = Some(error);
                }
                let close_events = events.clone();
                popup.on_window_should_close(cx, move |_, _| {
                    if close_events.unbounded_send(PaletteEvent::Closed).is_err() {
                        eprintln!("Palette closed after its browser receiver stopped.");
                    }
                    true
                });
                let palette = cx.new(|cx| PaletteView::new(initial, events, popup, cx));
                *popup_retained.borrow_mut() = Some(palette.clone());
                let root = cx.new(|cx| Root::new(palette.clone(), popup, cx));
                popup.defer(cx, move |popup, cx| {
                    popup.activate_window();
                    palette.update(cx, |palette, cx| palette.focus_input(popup, cx));
                });
                root
            },
        );
        match popup {
            Ok(handle) => {
                if let Some(error) = ownership_error.borrow_mut().take() {
                    self.message = error;
                    if handle
                        .update(cx, |_, window, _| window.remove_window())
                        .is_err()
                    {
                        self.message = "Cannot close the failed palette window.".to_owned();
                    }
                    return;
                }
                self.palette = Some(PalettePopup {
                    window: handle,
                    view: retained.borrow_mut().take().expect("opened palette view"),
                });
                self.message = "Palette open".to_owned();
            }
            Err(_) => {
                self.message = "Cannot open the command palette.".to_owned();
                return;
            }
        }
        let palette_id = self
            .palette
            .as_ref()
            .expect("opened palette")
            .view
            .entity_id();
        cx.spawn_in(window, async move |view, cx| {
            while let Some(event) = receiver.next().await {
                let closed = event.finishes_entry();
                // A late event from a closed popup must not act on a replacement popup.
                let delivered = view.update_in(cx, |view, window, cx| {
                    if view
                        .palette
                        .as_ref()
                        .is_none_or(|palette| palette.view.entity_id() != palette_id)
                    {
                        return false;
                    }
                    view.palette_event(event, window, cx);
                    true
                });
                if !matches!(delivered, Ok(true)) {
                    break;
                }
                if closed {
                    break;
                }
            }
        })
        .detach();
    }

    fn palette_event(&mut self, event: PaletteEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        if event.finishes_entry() {
            self.pending_query = None;
            self.home_palette.finish_entry();
        }
        match event {
            PaletteEvent::Query { request_id, text } => {
                self.query_sequence = self
                    .query_sequence
                    .checked_add(1)
                    .expect("search request IDs exhausted");
                self.pending_query = Some((self.query_sequence, request_id));
                self.command(SessionCommand::Query {
                    request_id: self.query_sequence,
                    policy: search_policy(&text),
                    text,
                    scope: Scope::Everything,
                });
            }
            PaletteEvent::Submit { text, force_search } => {
                self.palette = None;
                self.navigate(text, force_search);
                self.focus_page(window);
            }
            PaletteEvent::Activate(target) => {
                self.palette = None;
                match target {
                    Target::Tab(id) => self.command(SessionCommand::FocusTab(id)),
                    Target::Saved(id) => self.command(SessionCommand::OpenSaved(id)),
                    _ => {
                        self.message =
                            "This destination is not available in the preview.".to_owned()
                    }
                }
                self.focus_page(window);
            }
            PaletteEvent::Shortcut(shortcut) => {
                let popup_live = self.palette.as_ref().is_some_and(|palette| {
                    let handle: gpui::AnyWindowHandle = palette.window.into();
                    cx.windows().contains(&handle)
                });
                dispatch_palette_shortcut(shortcut, popup_live, |shortcut| {
                    self.shortcut(shortcut, window, cx);
                });
            }
            PaletteEvent::Closed => {
                self.palette = None;
                self.focus_page(window);
            }
        }
        cx.notify();
    }

    fn focus_page(&mut self, window: &mut Window) {
        if self.closing {
            return;
        }
        window.activate_window();
        if let Some(id) = self.shown {
            self.page_command(BrowserCommand::Focus { id: id.get() });
        }
    }

    fn active_address(&self) -> String {
        self.model
            .tabs
            .iter()
            .find(|tab| Some(tab.id) == self.model.active_id)
            .map_or_else(String::new, |tab| {
                if tab.metadata.address == "about:blank" {
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
        self.retire_peek(cx);
        self.retire_palette(cx);
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

    fn retire_palette(&mut self, cx: &mut Context<Self>) {
        // Invalidate queued popup events before removing a still-open native window.
        self.pending_query = None;
        self.home_palette.finish_entry();
        let Some(palette) = self.palette.take() else {
            return;
        };
        let handle: gpui::AnyWindowHandle = palette.window.into();
        // Enter can already have removed the window while its Submit event is queued.
        if !cx.windows().contains(&handle) {
            return;
        }
        if handle
            .update(cx, |_, window, _| window.remove_window())
            .is_err()
        {
            self.fail("Cannot close the command palette during shutdown.");
        }
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
            scale: window.scale_factor(),
        });
        if self.bounds != Some(bounds) && self.page_command(BrowserCommand::Resize(bounds)) {
            self.bounds = Some(bounds);
        }
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> Div {
        div()
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
            .child(button("back", "‹").on_click(cx.listener(|this, _, _, cx| {
                if let Some(id) = this.shown {
                    this.page_command(BrowserCommand::Back { id: id.get() });
                }
                cx.notify();
            })))
            .child(
                button("forward", "›").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(id) = this.shown {
                        this.page_command(BrowserCommand::Forward { id: id.get() });
                    }
                    cx.notify();
                })),
            )
            .child(
                button("reload", "↻").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(id) = this.shown {
                        this.page_command(BrowserCommand::Reload { id: id.get() });
                    }
                    cx.notify();
                })),
            )
            .child(
                div()
                    .id("address")
                    .role(gpui::Role::Button)
                    .aria_label("Search or enter address")
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
                    .child(if self.active_address().is_empty() {
                        "Search or enter address · Ctrl K".to_owned()
                    } else {
                        self.active_address()
                    }),
            )
            .child(
                button("save", "Save page").on_click(cx.listener(|this, _, _, cx| {
                    this.command(SessionCommand::SaveActive);
                    cx.notify();
                })),
            )
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.session_revision.is_none() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(title_bar(div().child("Atlas")))
                .child(self.startup_panel(cx));
        }
        if self.browser_ready && !self.closing {
            self.resize_pages(window);
        }
        let page = if self.shown.is_none() {
            self.home_page(cx)
        } else {
            div().flex_1().h_full()
        };
        div()
            .size_full()
            .key_context("Atlas")
            .bg(theme::window_fill())
            .text_color(theme::text())
            .text_sm()
            .font_family("Segoe UI Variable Text")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.sidebar_drag_moved(event, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    this.sidebar_drag_released(f32::from(event.position.x), cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    this.sidebar_drag_released(f32::from(event.position.x), cx);
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
            .child(title_bar(if self.sidebar_sizing.width() == 0. {
                div()
                    .w(px(page_card::titlebar_navigation_width(f32::from(
                        window.viewport_size().width,
                    ))))
                    .min_w_0()
                    .child(self.toolbar(cx))
            } else {
                div().h_full().flex().items_center().child("Atlas")
            }))
            .when(self.sidebar_sizing.width() > 0., |root| {
                root.child(self.toolbar(cx))
            })
            .child(
                div()
                    .flex_1()
                    .flex()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(self.sidebar_edge(cx))
                    .child(page),
            )
            .child(
                div()
                    .h(px(FOOTER_HEIGHT))
                    .px_3()
                    .bg(theme::ground())
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(theme::icon())
                    .child(self.failure.as_ref().unwrap_or(&self.message).clone()),
            )
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
            SessionCommand::ConfirmRecovery { .. } | SessionCommand::Shutdown
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

fn button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(theme::line())
        .cursor_pointer()
        .child(label)
}

fn native_handle(window: &Window) -> Result<isize, &'static str> {
    let handle =
        HasWindowHandle::window_handle(window).map_err(|_| "The native window is unavailable.")?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
        _ => Err("Atlas requires a Windows window."),
    }
}

fn search_policy(text: &str) -> SearchPolicy {
    // These are preview tuning values; the core policy remains an explicit caller input.
    SearchPolicy {
        limit: 8,
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

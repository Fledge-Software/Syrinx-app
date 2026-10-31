//! One page tab: a WebKit view in a GTK window embedded as an X11 child of Syrinx's window.
//!
//! No IPC handler, user script or custom user agent is ever added (AGENTS.md, Google's pages).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_ulong,
    rc::Rc,
    sync::atomic::Ordering,
};

use gtk::{gdk, glib, prelude::*};
use syrinx_core::{HistoryChange, HistoryDirection, HistoryReport};
use syrinx_store::PermissionKind;
use webkit2gtk::{
    BackForwardListExt, FindControllerExt, FindOptions, GeolocationPermissionRequest, LoadEvent,
    NavigationPolicyDecision, NavigationType, NotificationPermissionRequest, PermissionRequest,
    PermissionRequestExt, PolicyDecisionType, SettingsExt, URIRequestExt,
    UserMediaPermissionRequest, UserMediaPermissionRequestExt, WebProcessTerminationReason,
    WebView, WebViewExt,
};
use x11_dl::xlib;

use super::{
    BrowserCommand, BrowserError, BrowserEvent, BrowserShortcut, CallbackLife, ConnectionScheme,
    ConnectionState, ConnectionStatus, PageAction, PageBounds, RENDER_PROCESS_EXITED,
    RENDER_PROCESS_UNRESPONSIVE,
    engine::{PendingPopup, Shared},
    history_state::{Pending, Started},
    keys::{peek_escape, shortcut_for_modifiers},
    next_zoom, validate_url,
};

/// Permission prompts waiting on the owner, by request number.
type Prompts = Rc<RefCell<HashMap<u64, PermissionRequest>>>;

pub(super) struct Page {
    id: u64,
    view: WebView,
    window: gtk::Window,
    embed: Embed,
    shared: Shared,
    /// Retired when the page closes, so its late events never reach the UI.
    life: CallbackLife,
    prompts: Prompts,
    find: Rc<FindState>,
}

impl Page {
    pub(super) fn new(
        id: u64,
        view: WebView,
        bounds: PageBounds,
        shared: Shared,
    ) -> Result<Self, BrowserError> {
        configure(&view);
        let life = CallbackLife::new();
        let shared = Shared {
            events: shared.events.for_page(life.clone()),
            ..shared
        };
        let embed = Embed::new(shared.parent, bounds)?;
        let window = embed.gtk_window();
        window.add(&view);
        window.show_all();
        let page = Self {
            id,
            view,
            window,
            embed,
            shared,
            life,
            prompts: Rc::default(),
            find: Rc::default(),
        };
        page.resize(bounds);
        page.show(false);
        page.watch();
        Ok(page)
    }

    pub(super) fn view(&self) -> &WebView {
        &self.view
    }

    pub(super) fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        validate_url(url)?;
        self.view.load_uri(url);
        Ok(())
    }

    pub(super) fn resize(&self, bounds: PageBounds) {
        self.embed.place(bounds);
        let width = i32::try_from(bounds.width).unwrap_or(i32::MAX).max(1);
        let height = i32::try_from(bounds.height).unwrap_or(i32::MAX).max(1);
        self.window.resize(width, height);
        self.window
            .size_allocate(&gtk::Allocation::new(0, 0, width, height));
    }

    pub(super) fn show(&self, visible: bool) {
        if visible {
            self.window.show_all();
        } else {
            self.window.hide();
        }
        self.embed.map(visible);
    }

    pub(super) fn apply(&self, command: BrowserCommand) -> Result<(), BrowserError> {
        match command {
            BrowserCommand::Navigate { url, .. } => self.navigate(&url)?,
            BrowserCommand::Reload { .. } => self.view.reload(),
            BrowserCommand::GoToOffset {
                direction, steps, ..
            } => self.go_to_offset(direction, steps)?,
            BrowserCommand::Focus { .. } => self.focus(),
            BrowserCommand::SetMuted { muted, .. } => self.view.set_is_muted(muted),
            BrowserCommand::PageAction { action, .. } => self.page_action(action)?,
            BrowserCommand::ResolvePermission {
                request, allowed, ..
            } => self.resolve(request, allowed)?,
            BrowserCommand::ExitFullscreen { .. } => self.exit_fullscreen(),
            _ => return Err(BrowserError::InvalidTab),
        }
        Ok(())
    }

    /// Steps `steps` entries in one jump, as Windows' run of Back or Forward calls does.
    fn go_to_offset(&self, direction: HistoryDirection, steps: u32) -> Result<(), BrowserError> {
        let list = self
            .view
            .back_forward_list()
            .ok_or(BrowserError::Navigation)?;
        let steps = i32::try_from(steps).map_err(|_| BrowserError::Navigation)?;
        let offset = match direction {
            HistoryDirection::Back => -steps,
            HistoryDirection::Forward => steps,
        };
        let item = list.nth_item(offset).ok_or(BrowserError::Navigation)?;
        self.view.go_to_back_forward_list_item(&item);
        Ok(())
    }

    fn focus(&self) {
        self.embed.focus();
        self.view.grab_focus();
    }

    fn page_action(&self, action: PageAction) -> Result<(), BrowserError> {
        match action {
            PageAction::Find {
                request,
                text,
                case_sensitive,
            } => self.find(request, &text, case_sensitive),
            PageAction::FindNext => self.step_find(true),
            PageAction::FindPrevious => self.step_find(false),
            PageAction::StopFind => self.stop_find(),
            PageAction::Print => print(&self.view),
            action @ (PageAction::ZoomIn
            | PageAction::ZoomOut
            | PageAction::ZoomReset
            | PageAction::SetZoom(_)) => {
                let next = next_zoom(self.view.zoom_level(), &action);
                self.view.set_zoom_level(next);
            }
            // WebKitGTK has no download dialog of its own; Syrinx's flyout is the only one.
            PageAction::Downloads | PageAction::CloseDownloads => {}
            PageAction::SetDownloadFolder(_) => return Err(BrowserError::PageControl),
        }
        Ok(())
    }

    fn find(&self, request: u64, text: &str, case_sensitive: bool) {
        if text.is_empty() {
            self.stop_find();
            return;
        }
        let Some(controller) = self.view.find_controller() else {
            return;
        };
        self.find.request.set(Some(request));
        self.find.active.set(0);
        let mut options = FindOptions::WRAP_AROUND;
        if !case_sensitive {
            options |= FindOptions::CASE_INSENSITIVE;
        }
        controller.count_matches(text, options.bits(), u32::MAX);
        controller.search(text, options.bits(), u32::MAX);
    }

    fn step_find(&self, forward: bool) {
        let Some(controller) = self.view.find_controller() else {
            return;
        };
        let count = self.find.count.get();
        if count == 0 || self.find.request.get().is_none() {
            return;
        }
        let active = self.find.active.get();
        let next = if forward {
            (active + 1) % count
        } else {
            (active + count - 1) % count
        };
        self.find.active.set(next);
        if forward {
            controller.search_next();
        } else {
            controller.search_previous();
        }
        self.find.report(self.id, &self.shared);
    }

    fn stop_find(&self) {
        self.find.request.set(None);
        if let Some(controller) = self.view.find_controller() {
            controller.search_finish();
        }
    }

    fn resolve(&self, request: u64, allowed: bool) -> Result<(), BrowserError> {
        let prompt = self
            .prompts
            .borrow_mut()
            .remove(&request)
            .ok_or(BrowserError::Permission)?;
        if allowed {
            prompt.allow();
        } else {
            prompt.deny();
        }
        self.shared.events.emit(BrowserEvent::PermissionFinished {
            id: self.id,
            request,
        });
        Ok(())
    }

    /// Sends the page an Escape press, which leaves element fullscreen as the owner's own would.
    fn exit_fullscreen(&self) {
        let Some(window) = self.view.window() else {
            return;
        };
        let mut event = gdk::Event::new(gdk::EventType::KeyPress);
        // SAFETY: a fresh key event; its window gains a reference that the event releases.
        unsafe {
            let key = event.as_ptr().cast::<gdk::ffi::GdkEventKey>();
            (*key).window = window.as_ptr();
            gtk::glib::gobject_ffi::g_object_ref(window.as_ptr().cast());
            (*key).keyval = *gdk::keys::constants::Escape;
            (*key).send_event = 1;
        }
        gtk::main_do_event(&mut event);
    }

    /// Connects the host-side signals; none of them adds anything to the page.
    fn watch(&self) {
        self.watch_metadata();
        self.watch_loading();
        self.watch_keys();
        self.watch_audio();
        self.watch_find();
        self.watch_popups();
        self.watch_permissions();
        self.watch_processes();
    }

    fn watch_metadata(&self) {
        let (id, events) = (self.id, self.shared.events.clone());
        self.view.connect_title_notify(move |view| {
            let title = view.title().map(String::from).unwrap_or_default();
            events.emit(BrowserEvent::Title { id, title });
        });
        let (id, events) = (self.id, self.shared.events.clone());
        self.view.connect_zoom_level_notify(move |view| {
            events.emit(BrowserEvent::ZoomChanged {
                id,
                factor: view.zoom_level(),
            });
        });
        let (id, events) = (self.id, self.shared.events.clone());
        self.view.connect_is_loading_notify(move |view| {
            events.emit(BrowserEvent::Loading {
                id,
                loading: view.is_loading(),
            });
        });
        let (id, events) = (self.id, self.shared.events.clone());
        self.view.connect_focus_in_event(move |_, _| {
            events.emit(BrowserEvent::PageFocused { id });
            glib::Propagation::Proceed
        });
        let (id, events) = (self.id, self.shared.events.clone());
        self.view.connect_close(move |_| {
            events.emit(BrowserEvent::CloseRequested { id });
        });
        let (id, events) = (self.id, self.shared.events.clone());
        self.view.connect_enter_fullscreen(move |_| {
            events.emit(BrowserEvent::Fullscreen { id, active: true });
            // Handled: the page card grows instead of WebKit fullscreening the embedded window.
            true
        });
        let (id, events) = (self.id, self.shared.events.clone());
        self.view.connect_leave_fullscreen(move |_| {
            events.emit(BrowserEvent::Fullscreen { id, active: false });
            true
        });
    }

    fn watch_loading(&self) {
        let watch = LoadWatch {
            id: self.id,
            shared: self.shared.clone(),
            state: Rc::default(),
        };
        let starting = watch.clone();
        self.view.connect_decide_policy(move |_, decision, kind| {
            if kind == PolicyDecisionType::NavigationAction {
                starting.started(decision);
            }
            // Let WebKit make its own decision; Syrinx only observes.
            false
        });
        let changing = watch.clone();
        self.view.connect_load_changed(move |view, event| {
            changing.load_changed(view, event);
        });
        let failing = watch.clone();
        self.view.connect_load_failed(move |_, _, _, error| {
            failing.failed(error.matches(webkit2gtk::NetworkError::Cancelled));
            false
        });
        let rejecting = watch.clone();
        self.view
            .connect_load_failed_with_tls_errors(move |view, _, _, _| {
                rejecting.certificate_rejected(view);
                // Handled: the page is refused and WebKit offers no way past the error.
                true
            });
        self.view
            .connect_uri_notify(move |view| watch.address_changed(view));
    }

    fn watch_keys(&self) {
        let shared = self.shared.clone();
        self.view.connect_key_press_event(move |_, key| {
            let generation = shared.peek_generation.load(Ordering::Acquire);
            let Some(shortcut) = shortcut_for_key(key, generation) else {
                return glib::Propagation::Proceed;
            };
            shared.events.emit(BrowserEvent::Shortcut(shortcut));
            glib::Propagation::Stop
        });
    }

    fn watch_audio(&self) {
        for property in ["is-muted", "is-playing-audio"] {
            let (id, events) = (self.id, self.shared.events.clone());
            self.view
                .connect_notify_local(Some(property), move |view, _| {
                    events.emit(BrowserEvent::AudioChanged {
                        id,
                        muted: view.is_muted(),
                        playing: view.is_playing_audio(),
                    });
                });
        }
    }

    fn watch_find(&self) {
        let Some(controller) = self.view.find_controller() else {
            return;
        };
        let (id, shared, find) = (self.id, self.shared.clone(), self.find.clone());
        controller.connect_counted_matches(move |_, count| {
            find.count.set(count);
            find.report(id, &shared);
        });
        let (id, shared, find) = (self.id, self.shared.clone(), self.find.clone());
        controller.connect_failed_to_find_text(move |_| {
            find.count.set(0);
            find.report(id, &shared);
        });
    }

    fn watch_popups(&self) {
        let (id, shared) = (self.id, self.shared.clone());
        self.view.connect_create(move |opener, action| {
            if !shared.events.accepts_callbacks() {
                return None;
            }
            let url = action
                .clone()
                .request()
                .and_then(|request| request.uri())
                .map(String::from)
                .unwrap_or_default();
            if validate_url(&url).is_err() {
                shared.events.emit(BrowserEvent::PopupBlocked { id });
                return None;
            }
            // Created now and related to its opener, so `window.opener` works once adopted.
            let view = WebView::with_related_view(opener);
            let request = shared.next_request();
            shared.popups.borrow_mut().insert(
                request,
                PendingPopup {
                    opener: id,
                    view: view.clone(),
                },
            );
            shared
                .events
                .emit(BrowserEvent::PopupRequested { id, request, url });
            Some(view.upcast())
        });
    }

    fn watch_permissions(&self) {
        let (id, shared, prompts) = (self.id, self.shared.clone(), self.prompts.clone());
        self.view.connect_permission_request(move |view, request| {
            let Some(kind) = permission_kind(request) else {
                // Kinds Syrinx has no prompt for are refused, never granted silently.
                request.deny();
                return true;
            };
            let number = shared.next_request();
            prompts.borrow_mut().insert(number, request.clone());
            let uri = view.uri().map(String::from).unwrap_or_default();
            shared.events.emit(BrowserEvent::PermissionRequested {
                id,
                request: number,
                uri,
                kind,
            });
            true
        });
    }

    fn watch_processes(&self) {
        let (id, events) = (self.id, self.shared.events.clone());
        self.view.connect_web_process_terminated(move |_, reason| {
            if reason == WebProcessTerminationReason::TerminatedByApi {
                return;
            }
            events.emit(BrowserEvent::ProcessFailed {
                id,
                kind: RENDER_PROCESS_EXITED,
            });
        });
        let (id, events) = (self.id, self.shared.events.clone());
        self.view
            .connect_is_web_process_responsive_notify(move |view| {
                if view.is_web_process_responsive() {
                    return;
                }
                events.emit(BrowserEvent::ProcessFailed {
                    id,
                    kind: RENDER_PROCESS_UNRESPONSIVE,
                });
            });
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        self.life.retire();
        // Refuse prompts still open, so the page's requests do not wait forever.
        for (_, prompt) in self.prompts.borrow_mut().drain() {
            prompt.deny();
        }
        self.view.stop_loading();
        // SAFETY: the window is ours alone; destroying it releases the view inside it.
        unsafe { self.window.destroy() };
    }
}

/// Page settings Syrinx owns; the user agent is never changed.
fn configure(view: &WebView) {
    let Some(settings) = WebViewExt::settings(view) else {
        return;
    };
    settings.set_enable_developer_extras(false);
    settings.set_javascript_can_open_windows_automatically(false);
    settings.set_enable_write_console_messages_to_stdout(false);
}

fn print(view: &WebView) {
    let operation = webkit2gtk::PrintOperation::new(view);
    let _response = webkit2gtk::PrintOperationExt::run_dialog(&operation, None::<&gtk::Window>);
}

/// Which Syrinx permission a WebKit request asks for; `None` for kinds Syrinx never offers.
fn permission_kind(request: &PermissionRequest) -> Option<PermissionKind> {
    if request.is::<GeolocationPermissionRequest>() {
        return Some(PermissionKind::Location);
    }
    if request.is::<NotificationPermissionRequest>() {
        return Some(PermissionKind::Notifications);
    }
    let media = request.downcast_ref::<UserMediaPermissionRequest>()?;
    if media.is_for_video_device() {
        Some(PermissionKind::Camera)
    } else if media.is_for_audio_device() {
        Some(PermissionKind::Microphone)
    } else {
        None
    }
}

/// The shared Windows table decides; GDK keys are translated to its virtual-key codes first.
fn shortcut_for_key(key: &gdk::EventKey, peek_generation: u64) -> Option<BrowserShortcut> {
    let state = key.state();
    let control = state.contains(gdk::ModifierType::CONTROL_MASK);
    let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
    let alt = state.contains(gdk::ModifierType::MOD1_MASK);
    // AltGr arrives as Mod5 (ISO Level 3), which the Windows table calls the right Alt.
    let right_alt = state.contains(gdk::ModifierType::MOD5_MASK);
    let code = virtual_key(*key.keyval())?;
    peek_escape(code, control, shift, alt, peek_generation)
        .or_else(|| shortcut_for_modifiers(code, control, shift, alt, right_alt))
}

/// The Windows virtual-key code for a GDK key value, for the keys Syrinx's table names.
fn virtual_key(keyval: u32) -> Option<u32> {
    use gdk::keys::constants as key;
    let lower = gdk::keys::Key::from(keyval).to_lower();
    let value = *lower;
    match value {
        letter @ 0x61..=0x7a => Some(letter - 0x20),
        digit @ 0x30..=0x39 => Some(digit),
        _ if lower == key::Tab || lower == key::ISO_Left_Tab => Some(0x09),
        _ if lower == key::Escape => Some(0x1B),
        _ if lower == key::Left => Some(0x25),
        _ if lower == key::Right => Some(0x27),
        _ if lower == key::F3 => Some(0x72),
        _ if lower == key::F5 => Some(0x74),
        _ if lower == key::F6 => Some(0x75),
        _ if lower == key::F11 => Some(0x7A),
        _ if lower == key::comma => Some(0xBC),
        _ if lower == key::equal || lower == key::plus => Some(0xBB),
        _ if lower == key::minus || lower == key::underscore => Some(0xBD),
        _ if lower == key::KP_Add => Some(0x6B),
        _ if lower == key::KP_Subtract => Some(0x6D),
        _ if lower == key::KP_0 => Some(0x60),
        _ => None,
    }
}

/// The current find request, its match count and which match Syrinx last stepped to.
#[derive(Default)]
struct FindState {
    request: Cell<Option<u64>>,
    count: Cell<u32>,
    active: Cell<u32>,
}

impl FindState {
    fn report(&self, id: u64, shared: &Shared) {
        let Some(request) = self.request.get() else {
            return;
        };
        let count = self.count.get();
        let active = if count == 0 {
            -1
        } else {
            i32::try_from(self.active.get()).unwrap_or(-1)
        };
        shared.events.emit(BrowserEvent::FindResult {
            id,
            request,
            count: i32::try_from(count).unwrap_or(i32::MAX),
            active,
        });
    }
}

/// Load, connection and history facts for one page, kept between WebKit's signals.
#[derive(Default)]
struct LoadState {
    history: Cell<Pending>,
    navigation: Cell<u64>,
    /// Between `Started` and `Committed`, address changes belong to the load, not the page.
    provisional: Cell<bool>,
    failed: Cell<Option<bool>>,
    rejected: Cell<bool>,
}

#[derive(Clone)]
struct LoadWatch {
    id: u64,
    shared: Shared,
    state: Rc<LoadState>,
}

impl LoadWatch {
    fn started(&self, decision: &webkit2gtk::PolicyDecision) {
        let Some(decision) = decision.downcast_ref::<NavigationPolicyDecision>() else {
            return;
        };
        let Some(action) = webkit2gtk::NavigationPolicyDecisionExt::navigation_action(decision)
        else {
            return;
        };
        let started = match action.navigation_type() {
            NavigationType::BackForward => Started::BackOrForward,
            NavigationType::Reload => Started::Reload,
            _ => Started::NewDocument {
                user_initiated: action.is_user_gesture(),
            },
        };
        let navigation = self.state.navigation.get() + 1;
        self.state.navigation.set(navigation);
        self.update(|pending| pending.start(navigation, started));
    }

    fn load_changed(&self, view: &WebView, event: LoadEvent) {
        match event {
            LoadEvent::Started => {
                self.state.provisional.set(true);
                self.state.failed.set(None);
                self.state.rejected.set(false);
                self.connection(view, ConnectionStatus::Loading);
            }
            LoadEvent::Committed => {
                self.state.provisional.set(false);
                self.emit_url(view);
                self.update(|pending| Pending {
                    new_document: Some(true),
                    ..pending
                });
                self.report(view);
            }
            LoadEvent::Finished => self.finished(view),
            _ => {}
        }
    }

    fn finished(&self, view: &WebView) {
        self.state.provisional.set(false);
        let failure = self.state.failed.take();
        let success = failure.is_none() && !self.state.rejected.get();
        let navigation = self.state.navigation.get();
        self.update(|pending| pending.complete(navigation, success));
        if !self.state.rejected.get() {
            let status = if success {
                ConnectionStatus::Loaded
            } else {
                ConnectionStatus::Failed
            };
            self.connection(view, status);
        }
        self.shared.events.emit(BrowserEvent::NavigationCompleted {
            id: self.id,
            success,
            cancelled: failure == Some(true),
        });
    }

    fn failed(&self, cancelled: bool) {
        self.state.failed.set(Some(cancelled));
    }

    fn certificate_rejected(&self, view: &WebView) {
        self.state.rejected.set(true);
        self.state.failed.set(Some(false));
        self.connection(view, ConnectionStatus::CertificateRejected);
    }

    /// An address change outside a load is the page's own: a fragment or the History API.
    fn address_changed(&self, view: &WebView) {
        if self.state.provisional.get() {
            return;
        }
        self.emit_url(view);
        if view.is_loading() {
            return;
        }
        self.update(|pending| Pending {
            new_document: Some(false),
            ..pending
        });
        self.report(view);
    }

    fn report(&self, view: &WebView) {
        let (changes, next) = self.state.history.get().reports();
        self.state.history.set(next);
        let address = view.uri().map(String::from).unwrap_or_default();
        for change in changes {
            self.emit_history(view, change, &address);
        }
    }

    fn emit_history(&self, view: &WebView, change: HistoryChange, address: &str) {
        self.shared.events.emit(BrowserEvent::History {
            id: self.id,
            report: HistoryReport {
                change,
                address: address.to_owned(),
                can_go_back: view.can_go_back(),
                can_go_forward: view.can_go_forward(),
            },
        });
    }

    fn emit_url(&self, view: &WebView) {
        let url = view.uri().map(String::from).unwrap_or_default();
        self.shared
            .events
            .emit(BrowserEvent::Url { id: self.id, url });
    }

    fn connection(&self, view: &WebView, status: ConnectionStatus) {
        let uri = view.uri().map(String::from).unwrap_or_default();
        let scheme = if uri.starts_with("https://") {
            ConnectionScheme::Https
        } else if uri.starts_with("http://") {
            ConnectionScheme::Http
        } else {
            ConnectionScheme::Other
        };
        self.shared.events.emit(BrowserEvent::Connection {
            id: self.id,
            state: ConnectionState { scheme, status },
        });
    }

    fn update(&self, change: impl FnOnce(Pending) -> Pending) {
        self.state.history.set(change(self.state.history.get()));
    }
}

/// The X11 container window that holds one page's GTK window inside Syrinx's window.
struct Embed {
    xlib: xlib::Xlib,
    display: *mut xlib::Display,
    gdk_display: gdkx11::X11Display,
    container: c_ulong,
}

impl Embed {
    /// Creates the container as a child of `parent` on GTK's own X connection, as wry does.
    fn new(parent: isize, bounds: PageBounds) -> Result<Self, BrowserError> {
        let parent = c_ulong::try_from(parent).map_err(|_| BrowserError::InvalidWindow)?;
        let xlib = xlib::Xlib::open().map_err(|_| BrowserError::CreatePage)?;
        let gdk_display = gdk::Display::default()
            .and_then(|display| display.downcast::<gdkx11::X11Display>().ok())
            .ok_or(BrowserError::CreatePage)?;
        let display = unsafe { gdkx11::ffi::gdk_x11_display_get_xdisplay(gdk_display.as_ptr()) }
            .cast::<xlib::Display>();
        let width = bounds.width.max(1);
        let height = bounds.height.max(1);
        let container = unsafe {
            (xlib.XCreateSimpleWindow)(display, parent, bounds.x, bounds.y, width, height, 0, 0, 0)
        };
        if container == 0 {
            return Err(BrowserError::CreatePage);
        }
        Ok(Self {
            xlib,
            display,
            gdk_display,
            container,
        })
    }

    /// A GTK window whose GDK window is the container, so widgets draw inside Syrinx.
    fn gtk_window(&self) -> gtk::Window {
        let foreign = gdkx11::X11Window::foreign_new_for_display(&self.gdk_display, self.container);
        let foreign: gdk::Window = foreign.upcast();
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        // A widget that adopts a GDK window must register it, or GTK asserts on unrealize.
        window.connect_realize(move |window| {
            window.set_window(foreign.clone());
            window.register_window(&foreign);
        });
        window.set_has_window(true);
        window.realize();
        window
    }

    fn place(&self, bounds: PageBounds) {
        unsafe {
            (self.xlib.XMoveResizeWindow)(
                self.display,
                self.container,
                bounds.x,
                bounds.y,
                bounds.width.max(1),
                bounds.height.max(1),
            );
            (self.xlib.XFlush)(self.display);
        }
    }

    fn map(&self, visible: bool) {
        unsafe {
            if visible {
                (self.xlib.XMapRaised)(self.display, self.container);
            } else {
                (self.xlib.XUnmapWindow)(self.display, self.container);
            }
            (self.xlib.XFlush)(self.display);
        }
    }

    fn focus(&self) {
        unsafe {
            (self.xlib.XSetInputFocus)(
                self.display,
                self.container,
                xlib::RevertToParent,
                xlib::CurrentTime,
            );
            (self.xlib.XFlush)(self.display);
        }
    }
}

impl Drop for Embed {
    fn drop(&mut self) {
        unsafe {
            (self.xlib.XDestroyWindow)(self.display, self.container);
            (self.xlib.XFlush)(self.display);
        }
    }
}

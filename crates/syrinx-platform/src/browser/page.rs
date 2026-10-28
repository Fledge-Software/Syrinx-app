//! Hosts ordinary page views and forwards metadata and browser shortcuts without page scripts.

use std::{cell::Cell, num::NonZeroIsize};

use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use webview2_com::{
    AcceleratorKeyPressedEventHandler, CoTaskMemPWSTR, FocusChangedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
        COREWEBVIEW2_MOVE_FOCUS_REASON, COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT,
        COREWEBVIEW2_MOVE_FOCUS_REASON_PREVIOUS, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
        COREWEBVIEW2_PHYSICAL_KEY_STATUS, COREWEBVIEW2_PROCESS_FAILED_KIND,
        COREWEBVIEW2_WEB_ERROR_STATUS, COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED,
        ICoreWebView2Environment, ICoreWebView2Settings4,
    },
    MoveFocusRequestedEventHandler, NavigationCompletedEventHandler, ProcessFailedEventHandler,
    SourceChangedEventHandler,
};
use windows::{
    Win32::{
        Foundation::E_POINTER,
        UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_RMENU, VK_SHIFT},
    },
    core::{BOOL, Interface, PWSTR},
};
use wry::{
    PageLoadEvent, Rect, WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows,
    dpi::{PhysicalPosition, PhysicalSize},
};

use super::{
    BrowserCommand, BrowserError, BrowserEvent, BrowserShortcut, EventSink, NativeSink, PageBounds,
    placement::Placement,
};

struct Parent(NonZeroIsize);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // The UI retains the real parent until this worker reports Stopped.
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(self.0)))
        })
    }
}

/// The worker-wide channels and shared state every page is built with.
pub(super) struct PageContext {
    pub(super) events: EventSink,
    pub(super) shortcuts: NativeSink,
    pub(super) popups: super::popups::PopupRequests,
    pub(super) downloads: super::downloads::DownloadOperations,
    pub(super) text: super::page_text::TextContext,
    /// An adopted popup whose opener showed an excluded or tainted document.
    pub(super) starts_tainted: bool,
}

pub(super) struct Page {
    destination: std::rc::Rc<std::cell::RefCell<String>>,
    lifecycle: super::lifecycle::Lifecycle,
    permissions: super::permissions::Permissions,
    essentials: super::essentials::Essentials,
    text: Option<super::page_text::PageText>,
    view: WebView,
    placement: Cell<Placement>,
}

impl Page {
    pub(super) fn new(
        id: u64,
        parent: isize,
        environment: &ICoreWebView2Environment,
        bounds: PageBounds,
        context: PageContext,
    ) -> Result<Self, BrowserError> {
        let PageContext {
            events,
            shortcuts,
            popups,
            downloads,
            text: text_context,
            starts_tainted,
        } = context;
        let destination = std::rc::Rc::new(std::cell::RefCell::new("about:blank".to_owned()));
        let life = super::lifecycle::CallbackLife::new();
        let events = events.for_page(life.clone());
        let lifecycle = super::lifecycle::Lifecycle::new(id, life, events.clone());
        let parent = Parent(NonZeroIsize::new(parent).ok_or(BrowserError::InvalidWindow)?);
        let titles = events.clone();
        let loading = events.clone();
        let closes = events.clone();
        let view = WebViewBuilder::new()
            .with_environment(environment.clone())
            .with_additional_browser_args("--enable-features=msSmartScreenProtection")
            .with_visible(false)
            .with_focused(false)
            .with_devtools(false)
            .with_clipboard(false)
            .with_default_context_menus(true)
            .with_bounds(rect(bounds))
            .with_document_title_changed_handler(move |title| {
                titles.emit(BrowserEvent::Title { id, title })
            })
            .with_on_page_load_handler(move |state, _url| {
                loading.emit(BrowserEvent::Loading {
                    id,
                    loading: matches!(state, PageLoadEvent::Started),
                })
            })
            .with_window_close_requested_handler(move || {
                // Native teardown waits for the persisted model, outside this callback.
                closes.emit(BrowserEvent::CloseRequested { id });
            })
            .build_as_child(&parent)
            .map_err(|_| BrowserError::CreatePage)?;

        // Configure saving before the first real navigation; no IPC or initialization scripts exist.
        let settings = unsafe { view.webview().Settings() }.map_err(|_| BrowserError::Settings)?;
        let settings: ICoreWebView2Settings4 =
            settings.cast().map_err(|_| BrowserError::Settings)?;
        unsafe { settings.SetIsPasswordAutosaveEnabled(false) }
            .map_err(|_| BrowserError::Settings)?;
        // Unhandled page keys retain the engine's default browser actions.
        unsafe { settings.SetAreBrowserAcceleratorKeysEnabled(true) }
            .map_err(|_| BrowserError::Settings)?;
        super::security::register(&view, id, events.clone())?;
        popups.register(&view, id, events.clone())?;
        super::lifecycle::register_fullscreen(&view, id, events.clone())?;
        register_source(&view, id, events.clone(), destination.clone())?;
        register_navigation(&view, id, events.clone())?;
        super::history::register(&view, id, events.clone())?;
        lifecycle.register_navigation(&view)?;
        lifecycle.register_audio(&view)?;
        register_process_failure(&view, id, events.clone(), shortcuts.clone())?;
        let permissions = super::permissions::Permissions::register(&view, id, events.clone())?;
        let essentials = super::essentials::Essentials::register(&view, id, events.clone())?;
        super::downloads::register(&view, id, events.detached(), downloads)?;
        register_focus_arrival(&view, id, events.clone())?;
        register_focus_departure(&view, shortcuts.clone(), id, events.clone())?;
        // Observes headers and navigations only while capture is on; never a request filter.
        // A failure is reported and leaves the page unread; it never stops the tab opening.
        let text = match super::page_text::PageText::register(
            &view,
            id,
            text_context,
            starts_tainted,
            super::page_text::Channels {
                events: events.clone(),
                native: shortcuts.clone(),
            },
        ) {
            Ok(text) => text,
            Err(error) => {
                events.emit(BrowserEvent::Error {
                    tab: Some(id),
                    error,
                });
                None
            }
        };
        register_shortcuts(&view, shortcuts, id, events)?;
        Ok(Self {
            destination,
            lifecycle,
            permissions,
            essentials,
            text,
            view,
            placement: Cell::new(Placement::new(bounds)),
        })
    }

    pub(super) fn webview(&self) -> webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2 {
        self.view.webview()
    }

    #[cfg(test)]
    pub(super) fn synthetic_failure(&self, id: u64, kind: i32) -> BrowserEvent {
        BrowserEvent::Callback {
            ticket: self.lifecycle.life.ticket(true),
            event: Box::new(BrowserEvent::ProcessFailed { id, kind }),
        }
    }

    fn withdraw_syrinx_request(&self) {
        if let Some(text) = &self.text {
            text.syrinx_navigation_cancelled();
        }
    }

    /// The taint a rebuilt page starts with after a crash; an unobserved page fails closed.
    pub(super) fn tainted(&self) -> bool {
        self.text.as_ref().is_none_or(|text| text.tainted())
    }

    /// Whether a popup this page opens starts tainted; a page Syrinx does not observe gives
    /// no evidence, so its popups do (fail closed).
    pub(super) fn popup_tainted(&self) -> bool {
        self.text
            .as_ref()
            .is_none_or(|text| text.popup_tainted(&self.view))
    }

    pub(super) fn destination(&self) -> String {
        self.destination.borrow().clone()
    }

    /// Loads `url` as Syrinx's own navigation, which clears an excluded site's taint (P1-25).
    pub(super) fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        if let Some(text) = &self.text {
            text.syrinx_navigation(url);
        }
        self.load(url)
    }

    /// Reloads a crashed tab's address without counting as Syrinx's navigation, so the taint
    /// it was rebuilt with stays.
    pub(super) fn restore(&self, url: &str) -> Result<(), BrowserError> {
        self.load(url)
    }

    fn load(&self, url: &str) -> Result<(), BrowserError> {
        validate_url(url)?;
        self.lifecycle.resume(&self.view)?;
        // Mark the load first, so a hidden page cannot be suspended before WebView2 starts it.
        self.lifecycle.navigation_requested();
        if self.view.load_url(url).is_err() {
            self.lifecycle.navigation_refused();
            self.withdraw_syrinx_request();
            return Err(BrowserError::Navigation);
        }
        *self.destination.borrow_mut() = url.to_owned();
        Ok(())
    }

    /// Shows or hides the page. A page about to appear is resumed first, then moved to the
    /// card's bounds, then made visible; hiding needs no bounds.
    pub(super) fn show(&self, visible: bool, card: PageBounds) -> Result<(), BrowserError> {
        let place = || match self.placement.get().on_show(visible, card) {
            Some(bounds) => self.set_bounds(bounds),
            None => Ok(()),
        };
        self.lifecycle.show(&self.view, visible, place)?;
        self.update_placement(|placement| placement.shown(visible));
        if let Some(text) = self.text.as_ref().filter(|_| visible) {
            text.resumed();
        }
        Ok(())
    }

    pub(super) fn tick(&self) -> Result<(), BrowserError> {
        // Suspension is requested first, so a capture never starts beside a pending TrySuspend.
        self.lifecycle.tick(&self.view)?;
        let suspending = self.lifecycle.suspension_requested();
        self.text
            .as_ref()
            .map_or(Ok(()), |text| text.tick(&self.view, suspending))
    }
    pub(super) fn suspend_due(&self) -> Option<std::time::Instant> {
        self.lifecycle.due()
    }
    pub(super) fn text_due(&self) -> Option<std::time::Instant> {
        self.text.as_ref().and_then(super::page_text::PageText::due)
    }

    /// Follows the card if this page is shown; hidden pages catch up when shown.
    pub(super) fn resize(&self, card: PageBounds) -> Result<(), BrowserError> {
        match self.placement.get().on_resize(card) {
            Some(bounds) => self.set_bounds(bounds),
            None => Ok(()),
        }
    }

    fn set_bounds(&self, bounds: PageBounds) -> Result<(), BrowserError> {
        self.view
            .set_bounds(rect(bounds))
            .map_err(|_| BrowserError::Bounds)?;
        self.update_placement(|placement| placement.applied(bounds));
        Ok(())
    }

    fn update_placement(&self, change: impl FnOnce(&mut Placement)) {
        let mut placement = self.placement.get();
        change(&mut placement);
        self.placement.set(placement);
    }

    pub(super) fn apply(&self, command: BrowserCommand) -> Result<(), BrowserError> {
        if matches!(
            command,
            BrowserCommand::GoToOffset { .. } | BrowserCommand::Reload { .. }
        ) {
            self.lifecycle.resume(&self.view)?;
            // Back, forward and reload replace a pending Syrinx navigation request.
            self.withdraw_syrinx_request();
        }
        match command {
            BrowserCommand::ExitFullscreen { .. } => super::lifecycle::exit_fullscreen(&self.view),
            BrowserCommand::PageAction { action, .. } => self.essentials.apply(&self.view, action),
            BrowserCommand::ResolvePermission {
                request, allowed, ..
            } => self.permissions.resolve(request, allowed),
            BrowserCommand::Navigate { url, .. } => self.navigate(&url),
            BrowserCommand::Focus { .. } => self.view.focus().map_err(|_| BrowserError::Focus),
            // Not resumed first: muting a suspended page works and leaves it frozen (essentials fixture).
            BrowserCommand::SetMuted { muted, .. } => self.essentials.set_muted(&self.view, muted),
            BrowserCommand::GoToOffset {
                direction, steps, ..
            } => super::history::step(&self.view.webview(), direction, steps),
            BrowserCommand::Reload { .. } => {
                unsafe { self.view.webview().Reload() }.map_err(|_| BrowserError::Navigation)
            }
            _ => Err(BrowserError::InvalidTab),
        }
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        self.lifecycle.life.retire();
        self.permissions.clear();
    }
}

pub(super) fn validate_url(url: &str) -> Result<(), BrowserError> {
    if url.contains('\0')
        || !(url.starts_with("https://") || url.starts_with("http://") || url == "about:blank")
    {
        return Err(BrowserError::InvalidUrl);
    }
    Ok(())
}

fn rect(bounds: PageBounds) -> Rect {
    Rect {
        position: PhysicalPosition::new(bounds.x, bounds.y).into(),
        size: PhysicalSize::new(bounds.width, bounds.height).into(),
    }
}

fn register_source(
    view: &WebView,
    id: u64,
    events: EventSink,
    destination: std::rc::Rc<std::cell::RefCell<String>>,
) -> Result<(), BrowserError> {
    let handler = SourceChangedEventHandler::create(Box::new(move |core, _args| {
        let core = core.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let mut raw = PWSTR::null();
        let result = unsafe { core.Source(&mut raw) };
        let owned = CoTaskMemPWSTR::from(raw);
        if let Err(error) = result {
            events.emit(BrowserEvent::Error {
                tab: Some(id),
                error: BrowserError::Metadata,
            });
            return Err(error);
        }
        let url = owned.to_string();
        if events.accepts_callbacks() && validate_url(&url).is_ok() {
            *destination.borrow_mut() = url.clone();
        }
        events.emit(BrowserEvent::Url { id, url });
        Ok(())
    }));
    let mut token = 0;
    unsafe { view.webview().add_SourceChanged(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_navigation(view: &WebView, id: u64, events: EventSink) -> Result<(), BrowserError> {
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_core, args| {
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let mut success = BOOL::default();
        if let Err(error) = unsafe { args.IsSuccess(&mut success) } {
            events.emit(BrowserEvent::Error {
                tab: Some(id),
                error: BrowserError::Navigation,
            });
            return Err(error);
        }
        // A superseded or stopped navigation also fails, but nothing went wrong (AN-01); an
        // unreadable status is reported as an ordinary failure.
        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
        let cancelled = !success.as_bool()
            && unsafe { args.WebErrorStatus(&mut status) }.is_ok()
            && status == COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED;
        // wry's Finished event also covers failed navigation attempts; report the native result.
        events.emit(BrowserEvent::NavigationCompleted {
            id,
            success: success.as_bool(),
            cancelled,
        });
        Ok(())
    }));
    let mut token = 0;
    unsafe { view.webview().add_NavigationCompleted(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_process_failure(
    view: &WebView,
    id: u64,
    events: EventSink,
    native: NativeSink,
) -> Result<(), BrowserError> {
    let handler = ProcessFailedEventHandler::create(Box::new(move |_core, args| {
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
        if let Err(error) = unsafe { args.ProcessFailedKind(&mut kind) } {
            events.emit(BrowserEvent::Error {
                tab: Some(id),
                error: BrowserError::Metadata,
            });
            return Err(error);
        }
        // Forward only the structural kind; frame details and process descriptions stay unread.
        if let Err(error) = native.post(BrowserEvent::ProcessFailed { id, kind: kind.0 }, &events) {
            events.emit(BrowserEvent::Error {
                tab: Some(id),
                error,
            });
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { view.webview().add_ProcessFailed(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_shortcuts(
    view: &WebView,
    shortcuts: NativeSink,
    id: u64,
    events: EventSink,
) -> Result<(), BrowserError> {
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_controller, args| {
        if !events.accepts_callbacks() {
            return Ok(());
        }
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
        unsafe { args.KeyEventKind(&mut kind) }?;
        if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
            && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
        {
            return Ok(());
        }
        let mut key = 0;
        unsafe { args.VirtualKey(&mut key) }?;
        let generation = shortcuts
            .peek_generation
            .load(std::sync::atomic::Ordering::Acquire);
        let Some(shortcut) = keyboard_shortcut(key, generation) else {
            return Ok(());
        };
        let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
        unsafe {
            args.PhysicalKeyStatus(&mut status)?;
            args.SetHandled(true)?;
            if status.WasKeyDown.as_bool() {
                return Ok(());
            }
            // Accelerator callbacks block WebView2: defer all actions to our message loop.
            if let Err(error) = shortcuts.post(BrowserEvent::Shortcut(shortcut), &events) {
                events.emit(BrowserEvent::Error {
                    tab: Some(id),
                    error,
                });
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        view.controller()
            .add_AcceleratorKeyPressed(&handler, &mut token)
    }
    .map_err(|_| BrowserError::EventRegistration)
}

/// Tells Syrinx when the page takes keyboard focus, so the chrome stops showing a focus ring.
fn register_focus_arrival(view: &WebView, id: u64, events: EventSink) -> Result<(), BrowserError> {
    let handler = FocusChangedEventHandler::create(Box::new(move |_controller, _args| {
        events.emit(BrowserEvent::PageFocused { id });
        Ok(())
    }));
    let mut token = 0;
    unsafe { view.controller().add_GotFocus(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

/// Tab past the page's last (or Shift Tab before its first) element moves to Syrinx's
/// neighbouring pane, as F6 does. Like the accelerator handler it only marks and posts.
fn register_focus_departure(
    view: &WebView,
    shortcuts: NativeSink,
    id: u64,
    events: EventSink,
) -> Result<(), BrowserError> {
    let handler = MoveFocusRequestedEventHandler::create(Box::new(move |_controller, args| {
        if !events.accepts_callbacks() {
            return Ok(());
        }
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let mut reason = COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC;
        unsafe { args.Reason(&mut reason) }?;
        let Some(shortcut) = departure_shortcut(reason) else {
            return Ok(());
        };
        unsafe { args.SetHandled(true) }?;
        if let Err(error) = shortcuts.post(BrowserEvent::Shortcut(shortcut), &events) {
            events.emit(BrowserEvent::Error {
                tab: Some(id),
                error,
            });
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        view.controller()
            .add_MoveFocusRequested(&handler, &mut token)
    }
    .map_err(|_| BrowserError::EventRegistration)
}

/// Which pane move a page's focus departure asks for; a programmatic request stays put.
fn departure_shortcut(reason: COREWEBVIEW2_MOVE_FOCUS_REASON) -> Option<BrowserShortcut> {
    match reason {
        COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT => Some(BrowserShortcut::FocusNextPane),
        COREWEBVIEW2_MOVE_FOCUS_REASON_PREVIOUS => Some(BrowserShortcut::LeavePageBackward),
        _ => None,
    }
}

fn keyboard_shortcut(key: u32, peek_generation: u64) -> Option<BrowserShortcut> {
    let control = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0;
    let shift = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
    let alt = unsafe { GetKeyState(i32::from(VK_MENU.0)) } < 0;
    let right_alt = unsafe { GetKeyState(i32::from(VK_RMENU.0)) } < 0;
    peek_escape(key, control, shift, alt, peek_generation)
        .or_else(|| shortcut_for_modifiers(key, control, shift, alt, right_alt))
}

/// Preserve page Escape unless this exact popup was visible when the key arrived.
fn peek_escape(
    key: u32,
    control: bool,
    shift: bool,
    alt: bool,
    generation: u64,
) -> Option<BrowserShortcut> {
    (key == 0x1B && !control && !shift && !alt && generation != 0)
        .then_some(BrowserShortcut::DismissPeek(generation))
}

fn shortcut_for_modifiers(
    key: u32,
    control: bool,
    shift: bool,
    alt: bool,
    right_alt: bool,
) -> Option<BrowserShortcut> {
    if key == 0x53 && control && alt && !shift && !right_alt {
        return Some(BrowserShortcut::ToggleSidebar);
    }
    if alt && !control && !shift {
        return match key {
            0x25 => Some(BrowserShortcut::Back),
            0x27 => Some(BrowserShortcut::Forward),
            _ => None,
        };
    }
    // F3 and Shift F3 step Syrinx's find bar, so WebView2's own never opens (#82).
    if key == 0x72 && !control && !alt {
        return Some(if shift {
            BrowserShortcut::FindPrevious
        } else {
            BrowserShortcut::FindNext
        });
    }
    if key == 0x7A && !control && !alt && !shift {
        return Some(BrowserShortcut::Fullscreen);
    }
    if key == 0x74 && !control && !alt && !shift {
        return Some(BrowserShortcut::Reload);
    }
    // F6 and Shift F6 leave the page for Syrinx's next or previous pane (P1-36).
    if key == 0x75 && !control && !alt {
        return Some(if shift {
            BrowserShortcut::FocusPreviousPane
        } else {
            BrowserShortcut::FocusNextPane
        });
    }
    if !control || alt {
        return None;
    }
    // Find, history and zoom are Syrinx's even over a focused page, as in Chrome (owner,
    // 2026-10-18). Print (Ctrl P) is not part of that decision and stays the page's under
    // the 2026-10-12 rule.
    if let Some(shortcut) = find_history_or_zoom(key, shift) {
        return Some(shortcut);
    }
    match (key, shift) {
        (0x4A, false) => Some(BrowserShortcut::Downloads),
        (0x54, false) => Some(BrowserShortcut::NewTab),
        (0x54, true) => Some(BrowserShortcut::ReopenTab),
        (0x57, false) => Some(BrowserShortcut::CloseTab),
        (0x4C, false) => Some(BrowserShortcut::Address),
        (0x4B, false) => Some(BrowserShortcut::Palette),
        (0x44, false) => Some(BrowserShortcut::SavePage),
        (0x52, false) => Some(BrowserShortcut::Reload),
        (0x09, false) => Some(BrowserShortcut::NextTab),
        (0x09, true) => Some(BrowserShortcut::PreviousTab),
        // VK_OEM_COMMA opens Settings, following Chrome and Edge (#91). The owner decided the
        // gear; taking Ctrl , from a focused page awaits the owner's confirmation.
        (0xBC, false) => Some(BrowserShortcut::Settings),
        _ => None,
    }
}

/// Ctrl F, Ctrl G (find next, #82), Ctrl H and the zoom keys. Plus and minus also count
/// shifted and on the keypad, as Chrome's table does; `VK_OEM_PLUS` and `VK_OEM_MINUS`
/// follow Chrome's key-code table, not the printed keys: Ctrl = zooms in on a US keyboard,
/// and on layouts without - on that key (such as French AZERTY) Ctrl - falls back to
/// WebView2's own zoom.
fn find_history_or_zoom(key: u32, shift: bool) -> Option<BrowserShortcut> {
    match (key, shift) {
        (0x46, false) => Some(BrowserShortcut::Find),
        (0x47, false) => Some(BrowserShortcut::FindNext),
        (0x48, false) => Some(BrowserShortcut::History),
        (0xBB | 0x6B, _) => Some(BrowserShortcut::ZoomIn),
        (0xBD | 0x6D, _) => Some(BrowserShortcut::ZoomOut),
        (0x30 | 0x60, false) => Some(BrowserShortcut::ZoomReset),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_is_forwarded_only_for_a_live_peek_without_modifiers() {
        assert_eq!(peek_escape(0x1B, false, false, false, 0), None);
        assert_eq!(
            peek_escape(0x1B, false, false, false, 7),
            Some(BrowserShortcut::DismissPeek(7))
        );
        for (control, shift, alt) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            assert_eq!(peek_escape(0x1B, control, shift, alt, 7), None);
        }
        assert_eq!(peek_escape(0x41, false, false, false, 7), None);
    }

    #[test]
    fn sidebar_pin_requires_exactly_control_alt_s() {
        for control in [false, true] {
            for alt in [false, true] {
                for shift in [false, true] {
                    let expected =
                        (control && alt && !shift).then_some(BrowserShortcut::ToggleSidebar);
                    assert_eq!(
                        shortcut_for_modifiers(0x53, control, shift, alt, false),
                        expected
                    );
                }
            }
        }
    }

    #[test]
    fn f6_moves_between_panes_and_shift_reverses() {
        assert_eq!(
            shortcut_for_modifiers(0x75, false, false, false, false),
            Some(BrowserShortcut::FocusNextPane)
        );
        assert_eq!(
            shortcut_for_modifiers(0x75, false, true, false, false),
            Some(BrowserShortcut::FocusPreviousPane)
        );
        for (control, alt) in [(true, false), (false, true), (true, true)] {
            assert_eq!(
                shortcut_for_modifiers(0x75, control, false, alt, false),
                None
            );
        }
    }

    #[test]
    fn tabbing_out_of_a_page_moves_panes_but_scripted_focus_stays() {
        assert_eq!(
            departure_shortcut(COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT),
            Some(BrowserShortcut::FocusNextPane)
        );
        assert_eq!(
            departure_shortcut(COREWEBVIEW2_MOVE_FOCUS_REASON_PREVIOUS),
            Some(BrowserShortcut::LeavePageBackward)
        );
        assert_eq!(
            departure_shortcut(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC),
            None
        );
    }

    #[test]
    fn control_comma_opens_settings_over_a_focused_page() {
        assert_eq!(
            shortcut_for_modifiers(0xBC, true, false, false, false),
            Some(BrowserShortcut::Settings)
        );
        // Shifted, with Alt or AltGr, or alone, the comma stays the page's.
        for (control, shift, alt, right_alt) in [
            (true, true, false, false),
            (true, false, true, false),
            (true, false, true, true),
            (false, false, false, false),
        ] {
            assert_eq!(
                shortcut_for_modifiers(0xBC, control, shift, alt, right_alt),
                None
            );
        }
    }

    #[test]
    fn altgr_s_stays_with_the_page() {
        assert_eq!(shortcut_for_modifiers(0x53, true, false, true, true), None);
        assert_eq!(
            shortcut_for_modifiers(0x53, true, false, true, false),
            Some(BrowserShortcut::ToggleSidebar)
        );
    }

    #[test]
    fn page_navigation_refuses_non_web_schemes_and_embedded_nul() {
        for url in [
            "file:///synthetic.txt",
            "javascript:1",
            "data:text/plain,synthetic",
            "about:settings",
            "https://safe.example.test/\0https://other.example.test/",
            "example.test",
            "",
        ] {
            assert_eq!(validate_url(url), Err(BrowserError::InvalidUrl));
        }
        for url in [
            "http://synthetic.example.test/",
            "https://synthetic.example.test/caf%C3%A9",
            "about:blank",
        ] {
            assert_eq!(validate_url(url), Ok(()));
        }
    }

    #[test]
    fn page_shortcuts_distinguish_browser_actions_from_shifted_variants() {
        for (key, shift, expected) in [
            (0x54, false, BrowserShortcut::NewTab),
            (0x54, true, BrowserShortcut::ReopenTab),
            (0x09, false, BrowserShortcut::NextTab),
            (0x09, true, BrowserShortcut::PreviousTab),
            (0x57, false, BrowserShortcut::CloseTab),
            (0x4C, false, BrowserShortcut::Address),
            (0x4B, false, BrowserShortcut::Palette),
            (0x44, false, BrowserShortcut::SavePage),
            (0x52, false, BrowserShortcut::Reload),
            (0x4A, false, BrowserShortcut::Downloads),
        ] {
            assert_eq!(
                shortcut_for_modifiers(key, true, shift, false, false),
                Some(expected)
            );
            assert_eq!(
                shortcut_for_modifiers(key, false, shift, false, false),
                None
            );
            assert_eq!(shortcut_for_modifiers(key, true, shift, true, false), None);
        }
        assert_eq!(
            shortcut_for_modifiers(0x25, false, false, true, false),
            Some(BrowserShortcut::Back)
        );
        assert_eq!(
            shortcut_for_modifiers(0x27, false, false, true, false),
            Some(BrowserShortcut::Forward)
        );
        assert_eq!(
            shortcut_for_modifiers(0x74, false, false, false, false),
            Some(BrowserShortcut::Reload)
        );
    }

    #[test]
    fn find_history_and_zoom_keys_reach_syrinx_before_a_focused_page() {
        for (key, shift, expected) in [
            (0x46, false, BrowserShortcut::Find),
            (0x48, false, BrowserShortcut::History),
            // Ctrl = and Ctrl + share a key; the keypad has its own.
            (0xBB, false, BrowserShortcut::ZoomIn),
            (0xBB, true, BrowserShortcut::ZoomIn),
            (0x6B, false, BrowserShortcut::ZoomIn),
            (0x6B, true, BrowserShortcut::ZoomIn),
            (0xBD, false, BrowserShortcut::ZoomOut),
            (0xBD, true, BrowserShortcut::ZoomOut),
            (0x6D, false, BrowserShortcut::ZoomOut),
            (0x6D, true, BrowserShortcut::ZoomOut),
            (0x30, false, BrowserShortcut::ZoomReset),
            (0x60, false, BrowserShortcut::ZoomReset),
        ] {
            assert_eq!(
                shortcut_for_modifiers(key, true, shift, false, false),
                Some(expected)
            );
            // Without Ctrl, or with Alt too (AltGr included), the key stays the page's.
            assert_eq!(
                shortcut_for_modifiers(key, false, shift, false, false),
                None
            );
            assert_eq!(shortcut_for_modifiers(key, true, shift, true, false), None);
            assert_eq!(shortcut_for_modifiers(key, true, shift, true, true), None);
        }
    }

    #[test]
    fn find_next_and_previous_keys_step_syrinx_find_not_webview2s_bar() {
        // F3, Shift F3 and Ctrl G (#82); Syrinx takes them so WebView2's own bar never opens.
        for (key, control, shift, expected) in [
            (0x72, false, false, BrowserShortcut::FindNext),
            (0x72, false, true, BrowserShortcut::FindPrevious),
            (0x47, true, false, BrowserShortcut::FindNext),
        ] {
            assert_eq!(
                shortcut_for_modifiers(key, control, shift, false, false),
                Some(expected)
            );
            // With Alt, or AltGr, the key stays the page's.
            assert_eq!(
                shortcut_for_modifiers(key, control, shift, true, false),
                None
            );
            assert_eq!(
                shortcut_for_modifiers(key, control, shift, true, true),
                None
            );
        }
        // Ctrl F3 and a bare G are not find keys; Ctrl Shift G waits on the owner.
        assert_eq!(
            shortcut_for_modifiers(0x72, true, false, false, false),
            None
        );
        assert_eq!(
            shortcut_for_modifiers(0x47, false, false, false, false),
            None
        );
        assert_eq!(shortcut_for_modifiers(0x47, true, true, false, false), None);
    }

    #[test]
    fn print_and_shifted_find_history_and_reset_stay_with_the_page() {
        assert_eq!(
            shortcut_for_modifiers(0x50, true, false, false, false),
            None
        );
        for key in [0x46, 0x48, 0x50, 0x30, 0x60] {
            assert_eq!(shortcut_for_modifiers(key, true, true, false, false), None);
        }
    }

    #[test]
    fn unrelated_and_extra_modified_keys_stay_with_the_page() {
        for key in [0x41, 0x43, 0x53, 0x56, 0x58, 0x5A] {
            assert_eq!(shortcut_for_modifiers(key, true, false, false, false), None);
        }
        for key in [0x57, 0x4C, 0x4B, 0x44, 0x52, 0x46, 0x4A, 0x50, 0x48] {
            assert_eq!(shortcut_for_modifiers(key, true, true, false, false), None);
        }
        for (control, shift, alt) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            assert_eq!(
                shortcut_for_modifiers(0x74, control, shift, alt, false),
                None
            );
        }
        assert_eq!(shortcut_for_modifiers(0x25, true, false, true, false), None);
        assert_eq!(shortcut_for_modifiers(0x27, false, true, true, false), None);
    }
}

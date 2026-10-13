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

pub(super) struct Page {
    destination: std::rc::Rc<std::cell::RefCell<String>>,
    lifecycle: super::lifecycle::Lifecycle,
    permissions: super::permissions::Permissions,
    essentials: super::essentials::Essentials,
    view: WebView,
    placement: Cell<Placement>,
}

impl Page {
    pub(super) fn new(
        id: u64,
        parent: isize,
        environment: &ICoreWebView2Environment,
        bounds: PageBounds,
        events: EventSink,
        shortcuts: NativeSink,
        popups: super::popups::PopupRequests,
    ) -> Result<Self, BrowserError> {
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
        lifecycle.register_navigation(&view)?;
        register_process_failure(&view, id, events.clone(), shortcuts.clone())?;
        let permissions = super::permissions::Permissions::register(&view, id, events.clone())?;
        let essentials = super::essentials::Essentials::register(&view, id, events.clone())?;
        register_focus_arrival(&view, id, events.clone())?;
        register_focus_departure(&view, shortcuts.clone(), id, events.clone())?;
        register_shortcuts(&view, shortcuts, id, events)?;
        Ok(Self {
            destination,
            lifecycle,
            permissions,
            essentials,
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

    pub(super) fn destination(&self) -> String {
        self.destination.borrow().clone()
    }

    pub(super) fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        validate_url(url)?;
        self.lifecycle.resume(&self.view)?;
        // Mark the load first, so a hidden page cannot be suspended before WebView2 starts it.
        self.lifecycle.navigation_requested();
        if self.view.load_url(url).is_err() {
            self.lifecycle.navigation_refused();
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
        Ok(())
    }

    pub(super) fn tick(&self) -> Result<(), BrowserError> {
        self.lifecycle.tick(&self.view)
    }
    pub(super) fn suspend_due(&self) -> Option<std::time::Instant> {
        self.lifecycle.due()
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
            BrowserCommand::Back { .. }
                | BrowserCommand::Forward { .. }
                | BrowserCommand::Reload { .. }
        ) {
            self.lifecycle.resume(&self.view)?;
        }
        match command {
            BrowserCommand::ExitFullscreen { .. } => super::lifecycle::exit_fullscreen(&self.view),
            BrowserCommand::PageAction { action, .. } => self.essentials.apply(&self.view, action),
            BrowserCommand::ResolvePermission {
                request, allowed, ..
            } => self.permissions.resolve(request, allowed),
            BrowserCommand::Navigate { url, .. } => self.navigate(&url),
            BrowserCommand::Focus { .. } => self.view.focus().map_err(|_| BrowserError::Focus),
            BrowserCommand::Back { .. } => self.history(true),
            BrowserCommand::Forward { .. } => self.history(false),
            BrowserCommand::Reload { .. } => {
                unsafe { self.view.webview().Reload() }.map_err(|_| BrowserError::Navigation)
            }
            _ => Err(BrowserError::InvalidTab),
        }
    }

    fn history(&self, backward: bool) -> Result<(), BrowserError> {
        let core = self.view.webview();
        let mut allowed = BOOL::default();
        unsafe {
            if backward {
                core.CanGoBack(&mut allowed)
            } else {
                core.CanGoForward(&mut allowed)
            }
            .map_err(|_| BrowserError::Navigation)?;
            if !allowed.as_bool() {
                return Ok(());
            }
            if backward {
                core.GoBack()
            } else {
                core.GoForward()
            }
            .map_err(|_| BrowserError::Navigation)
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
        // wry's Finished event also covers failed navigation attempts; report the native result.
        events.emit(BrowserEvent::NavigationCompleted {
            id,
            success: success.as_bool(),
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

/// Tells Atlas when the page takes keyboard focus, so the chrome stops showing a focus ring.
fn register_focus_arrival(view: &WebView, id: u64, events: EventSink) -> Result<(), BrowserError> {
    let handler = FocusChangedEventHandler::create(Box::new(move |_controller, _args| {
        events.emit(BrowserEvent::PageFocused { id });
        Ok(())
    }));
    let mut token = 0;
    unsafe { view.controller().add_GotFocus(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

/// Tab past the page's last (or Shift Tab before its first) element moves to Atlas's
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
    if key == 0x7A && !control && !alt && !shift {
        return Some(BrowserShortcut::Fullscreen);
    }
    if key == 0x74 && !control && !alt && !shift {
        return Some(BrowserShortcut::Reload);
    }
    // F6 and Shift F6 leave the page for Atlas's next or previous pane (P1-36).
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
    fn find_history_print_and_zoom_keys_reach_the_page_before_the_engine() {
        // Include shifted plus and the numeric keypad variants.
        for key in [0x46, 0x48, 0x50, 0xBB, 0x6B, 0xBD, 0x6D, 0x30, 0x60] {
            for shift in [false, true] {
                assert_eq!(shortcut_for_modifiers(key, true, shift, false, false), None);
            }
        }
    }

    #[test]
    fn unrelated_and_extra_modified_keys_stay_with_the_page() {
        for key in [0x41, 0x43, 0x46, 0x53, 0x56, 0x58, 0x5A] {
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

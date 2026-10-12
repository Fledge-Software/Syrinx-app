//! Hosts ordinary page views and forwards metadata and browser shortcuts without page scripts.

use std::{cell::Cell, num::NonZeroIsize};

use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use webview2_com::{
    AcceleratorKeyPressedEventHandler, CoTaskMemPWSTR,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
        COREWEBVIEW2_PHYSICAL_KEY_STATUS, COREWEBVIEW2_PROCESS_FAILED_KIND,
        ICoreWebView2Environment, ICoreWebView2Settings4,
    },
    NavigationCompletedEventHandler, ProcessFailedEventHandler, SourceChangedEventHandler,
};
use windows::{
    Win32::{
        Foundation::E_POINTER,
        UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_RMENU, VK_SHIFT},
    },
    core::{BOOL, Interface, PWSTR},
};
use wry::{
    NewWindowResponse, PageLoadEvent, Rect, WebView, WebViewBuilder, WebViewBuilderExtWindows,
    WebViewExtWindows,
    dpi::{PhysicalPosition, PhysicalSize},
};

use super::{
    BrowserCommand, BrowserError, BrowserEvent, BrowserShortcut, EventSink, PageBounds,
    ShortcutSink, placement::Placement,
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
        shortcuts: ShortcutSink,
    ) -> Result<Self, BrowserError> {
        let parent = Parent(NonZeroIsize::new(parent).ok_or(BrowserError::InvalidWindow)?);
        let titles = events.clone();
        let loading = events.clone();
        let popups = events.clone();
        let closes = events.clone();
        let view = WebViewBuilder::new()
            .with_environment(environment.clone())
            .with_additional_browser_args("--enable-features=msSmartScreenProtection")
            .with_visible(false)
            .with_focused(false)
            .with_devtools(false)
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
            .with_new_window_req_handler(move |_url, _features| {
                popups.emit(BrowserEvent::PopupBlocked { id });
                NewWindowResponse::Deny
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
        register_source(&view, id, events.clone())?;
        register_navigation(&view, id, events.clone())?;
        register_process_failure(&view, id, events.clone())?;
        register_shortcuts(&view, shortcuts, id, events)?;
        Ok(Self {
            view,
            placement: Cell::new(Placement::new(bounds)),
        })
    }

    pub(super) fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        validate_url(url)?;
        self.view
            .load_url(url)
            .map_err(|_| BrowserError::Navigation)
    }

    /// Shows or hides the page, first moving a page about to appear to the card's bounds.
    pub(super) fn show(&self, visible: bool, card: PageBounds) -> Result<(), BrowserError> {
        if let Some(bounds) = self.placement.get().on_show(visible, card) {
            self.set_bounds(bounds)?;
        }
        self.view
            .set_visible(visible)
            .map_err(|_| BrowserError::Visibility)?;
        self.update_placement(|placement| placement.shown(visible));
        Ok(())
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
        match command {
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

fn validate_url(url: &str) -> Result<(), BrowserError> {
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

fn register_source(view: &WebView, id: u64, events: EventSink) -> Result<(), BrowserError> {
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
        events.emit(BrowserEvent::Url {
            id,
            url: owned.to_string(),
        });
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
        events.emit(BrowserEvent::ProcessFailed { id, kind: kind.0 });
        Ok(())
    }));
    let mut token = 0;
    unsafe { view.webview().add_ProcessFailed(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_shortcuts(
    view: &WebView,
    shortcuts: ShortcutSink,
    id: u64,
    events: EventSink,
) -> Result<(), BrowserError> {
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_controller, args| {
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
            if let Err(error) = shortcuts.post(shortcut) {
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
    if key == 0x74 && !control && !alt && !shift {
        return Some(BrowserShortcut::Reload);
    }
    if !control || alt {
        return None;
    }
    match (key, shift) {
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
    fn unrelated_and_extra_modified_keys_stay_with_the_page() {
        for key in [0x41, 0x43, 0x46, 0x53, 0x56, 0x58, 0x5A] {
            assert_eq!(shortcut_for_modifiers(key, true, false, false, false), None);
        }
        for key in [0x57, 0x4C, 0x4B, 0x44, 0x52] {
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

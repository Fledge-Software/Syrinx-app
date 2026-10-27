//! Reads an eligible page's rendered text with one DevTools snapshot, never a script (P1-25);
//! the facts eligibility needs are observed from navigation and response events only.

mod headers;
mod reader;
mod snapshot;
mod state;

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Instant, SystemTime},
};

use syrinx_core::{EligibleDocument, Ineligible, PageTextPolicy};
use webview2_com::{
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2_2, ICoreWebView2_3, ICoreWebView2NavigationStartingEventArgs,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler, ProcessFailedEventHandler,
    SourceChangedEventHandler, WebResourceResponseReceivedEventHandler, take_pwstr,
};
use windows::{
    Win32::Foundation::E_POINTER,
    core::{BOOL, HSTRING, Interface, PWSTR, w},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, DiscardReason, EventSink, NativeSink, PageTextOutcome};
use reader::{Arrival, Job, Reader};
use state::{Rules, Start, TextState, without_fragment};

/// What every page on the worker shares: the capture policy (`None` means off), the last
/// exclusion lists, which keep deciding taint while capture is off, and the one parse worker.
#[derive(Clone, Default)]
pub(super) struct TextContext {
    policy: Rc<RefCell<Option<Rc<PageTextPolicy>>>>,
    last: Rc<RefCell<Option<Rc<PageTextPolicy>>>>,
    reader: Reader,
}

impl TextContext {
    pub(super) fn set_policy(&self, policy: Option<PageTextPolicy>) {
        let policy = policy.map(Rc::new);
        if let Some(policy) = &policy {
            *self.last.borrow_mut() = Some(policy.clone());
        }
        *self.policy.borrow_mut() = policy;
    }

    fn policy(&self) -> Option<Rc<PageTextPolicy>> {
        self.policy.borrow().clone()
    }

    /// The lists taint is judged by: the current ones, or the last ones while capture is off.
    fn exclusions(&self) -> Option<Rc<PageTextPolicy>> {
        self.last.borrow().clone()
    }
}

/// One page's capture: its observed state and the channels its results leave by.
pub(super) struct PageText {
    id: u64,
    context: TextContext,
    state: Rc<RefCell<TextState>>,
    events: EventSink,
    native: NativeSink,
}

impl PageText {
    /// Subscribes only when capture is on as the page is created, so a page created with
    /// capture off is never observed or read. An adopted popup passes `starts_tainted`.
    pub(super) fn register(
        view: &WebView,
        id: u64,
        context: TextContext,
        starts_tainted: bool,
        channels: Channels,
    ) -> Result<Option<Self>, BrowserError> {
        if context.policy().is_none() {
            return Ok(None);
        }
        let Channels { events, native } = channels;
        let text = Self {
            id,
            context,
            state: Rc::new(RefCell::new(TextState::seeded(starts_tainted))),
            events,
            native,
        };
        let core = view.webview();
        text.register_navigation_starting(&core)?;
        text.register_responses(&core)?;
        text.register_source(&core)?;
        text.register_completion(&core)?;
        text.register_process_failure(&core)?;
        Ok(Some(text))
    }

    pub(super) fn due(&self) -> Option<Instant> {
        self.state.borrow().due()
    }

    /// Syrinx itself asked the tab to load `address`; only such a navigation clears taint.
    pub(super) fn syrinx_navigation(&self, address: &str) {
        self.state
            .borrow_mut()
            .syrinx_navigation(address, Instant::now());
    }

    /// Withdraws Syrinx's pending request: refused, or replaced by back, forward, reload or
    /// another page action.
    pub(super) fn syrinx_navigation_cancelled(&self) {
        self.state.borrow_mut().syrinx_navigation_cancelled();
    }

    /// This page's taint, carried into its replacement when crash recovery rebuilds it.
    pub(super) fn tainted(&self) -> bool {
        self.state.borrow().tainted()
    }

    /// Whether a popup this page opens starts tainted: the page shows an excluded site or
    /// was itself reached by leaving one. An unreadable address fails closed.
    pub(super) fn popup_tainted(&self, view: &WebView) -> bool {
        if self.state.borrow().tainted() {
            return true;
        }
        let Some(policy) = self.context.exclusions() else {
            return true;
        };
        let Ok(source) = source_of(&view.webview()) else {
            return true;
        };
        let opener = syrinx_core::LeftDocument {
            address: &source,
            tainted: false,
        };
        policy.taints(Some(opener), &[])
    }

    /// A capture deferred while suspending runs once the page is shown again.
    pub(super) fn resumed(&self) {
        self.state.borrow_mut().resumed(Instant::now());
    }

    /// Judges and captures a document whose settle delay has passed. `suspending` is true
    /// while a `TrySuspend` is requested, when `IsSuspended` still reads false.
    pub(super) fn tick(&self, view: &WebView, suspending: bool) -> Result<(), BrowserError> {
        let Some(scheduled) = self.state.borrow_mut().take_due(Instant::now()) else {
            return Ok(());
        };
        let core = view.webview();
        if suspending || is_suspended(&core)? {
            self.state.borrow_mut().defer();
            return Ok(());
        }
        let policy = self.context.policy();
        let verdict = self.state.borrow().judge(policy.as_deref(), scheduled);
        let visited_at = self.state.borrow().visited().and_then(wall_clock);
        match verdict {
            Err(reason) => {
                self.emit(PageTextOutcome::Ineligible(reason));
                Ok(())
            }
            Ok(document) => self.capture(&core, document, visited_at),
        }
    }

    /// The one place a DevTools call is made; it announces its target first, so a test can
    /// count every call and its address.
    fn capture(
        &self,
        core: &ICoreWebView2,
        document: EligibleDocument,
        visited_at: Option<SystemTime>,
    ) -> Result<(), BrowserError> {
        // Step 8 again, at the moment of the call.
        if !document.is_current(self.state.borrow().generation()) {
            self.emit(PageTextOutcome::Ineligible(Ineligible::Stale));
            return Ok(());
        }
        // A navigation that never committed (a 204, a download) leaves the previous,
        // possibly excluded, document on screen; only the judged one may be read.
        if !shows(core, document.address())? {
            self.emit(PageTextOutcome::NotCommitted);
            return Ok(());
        }
        self.emit(PageTextOutcome::Requested {
            address: document.address().to_owned(),
        });
        self.state.borrow_mut().call_started();
        let handler = reader::handler(self.arrival(document, visited_at));
        let parameters = HSTRING::from(snapshot::CAPTURE_PARAMETERS);
        let call = unsafe {
            core.CallDevToolsProtocolMethod(
                w!("DOMSnapshot.captureSnapshot"),
                &parameters,
                &handler,
            )
        };
        if call.is_err() {
            self.state.borrow_mut().call_finished(Instant::now());
            return Err(BrowserError::PageText);
        }
        Ok(())
    }

    /// Drops the response unread if the tab changed or capture was switched off; otherwise
    /// queues it for the parse worker.
    fn arrival(
        &self,
        document: EligibleDocument,
        visited_at: Option<SystemTime>,
    ) -> impl FnOnce(Arrival) + 'static {
        let (id, state, events, native) = self.channels();
        let context = self.context.clone();
        let started = Instant::now();
        move |arrival| {
            let generation = state.borrow_mut().call_finished(Instant::now());
            let current = context.policy().is_some() && document.is_current(generation);
            let outcome = match arrival {
                Arrival::Failed => PageTextOutcome::Failed,
                _ if !current => PageTextOutcome::Superseded,
                Arrival::TooLarge => PageTextOutcome::Discarded(DiscardReason::TooLarge),
                Arrival::Json(json) => {
                    let job = Job {
                        id,
                        json,
                        document,
                        call_time: started.elapsed(),
                        visited_at,
                        events: events.clone(),
                        native,
                    };
                    match context.reader.read(job) {
                        Ok(()) => return,
                        Err(refused) => reader::refused_outcome(refused),
                    }
                }
            };
            events.emit(BrowserEvent::PageText { id, outcome });
        }
    }

    fn channels(&self) -> (u64, Rc<RefCell<TextState>>, EventSink, NativeSink) {
        (
            self.id,
            self.state.clone(),
            self.events.clone(),
            self.native.clone(),
        )
    }

    fn emit(&self, outcome: PageTextOutcome) {
        self.events.emit(BrowserEvent::PageText {
            id: self.id,
            outcome,
        });
    }

    /// Records each requested address, including every redirect hop, and the document a new
    /// navigation leaves, read from the web view's Source (step 6).
    fn register_navigation_starting(&self, core: &ICoreWebView2) -> Result<(), BrowserError> {
        let (id, state, events, _) = self.channels();
        let handler = NavigationStartingEventHandler::create(Box::new(move |core, args| {
            let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let core = core.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let start = match read_start(&core, &args) {
                Ok(start) => start,
                Err(error) => {
                    // Without the hop, the chain is unknown: forget it and taint (fail closed).
                    state.borrow_mut().forget_navigation(Instant::now());
                    report(&events, id);
                    return Err(error);
                }
            };
            let cancelled = state.borrow_mut().navigation_started(start);
            cancelled_notice(&events, id, cancelled);
            Ok(())
        }));
        let mut token = 0;
        unsafe { core.add_NavigationStarting(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)
    }

    /// Reads status and three named headers of the current navigation's responses.
    fn register_responses(&self, core: &ICoreWebView2) -> Result<(), BrowserError> {
        let (id, state, events, _) = self.channels();
        let handler =
            WebResourceResponseReceivedEventHandler::create(Box::new(move |_core, args| {
                let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                let wanted = |address: &str| state.borrow().wants_response(address);
                match headers::observed_response(&args, wanted) {
                    Ok(Some(response)) => state.borrow_mut().record_response(response),
                    Ok(None) => {}
                    Err(error) => {
                        report(&events, id);
                        return Err(error);
                    }
                }
                Ok(())
            }));
        let core: ICoreWebView2_2 = core.cast().map_err(|_| BrowserError::EventRegistration)?;
        let mut token = 0;
        unsafe { core.add_WebResourceResponseReceived(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)
    }

    /// A new document, which inherits taint, or a History API change, which may schedule a
    /// rate-limited capture. Taint is tracked even while capture is off.
    fn register_source(&self, core: &ICoreWebView2) -> Result<(), BrowserError> {
        let (id, state, events, _) = self.channels();
        let context = self.context.clone();
        let handler = SourceChangedEventHandler::create(Box::new(move |core, args| {
            let core = core.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let mut raw = PWSTR::null();
            let mut new_document = BOOL::default();
            let read = unsafe {
                core.Source(&mut raw)
                    .and_then(|()| args.IsNewDocument(&mut new_document))
            };
            let source = take_pwstr(raw);
            if let Err(error) = read {
                state.borrow_mut().forget_navigation(Instant::now());
                report(&events, id);
                return Err(error);
            }
            let Some(policy) = context.exclusions() else {
                state.borrow_mut().advance();
                return Ok(());
            };
            let rules = Rules {
                policy: &policy,
                capture: context.policy().is_some(),
            };
            let cancelled = state.borrow_mut().source_changed(
                source,
                new_document.as_bool(),
                rules,
                Instant::now(),
            );
            cancelled_notice(&events, id, cancelled);
            Ok(())
        }));
        let mut token = 0;
        unsafe { core.add_SourceChanged(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)
    }

    /// Schedules the capture a settle delay after the document finishes loading.
    fn register_completion(&self, core: &ICoreWebView2) -> Result<(), BrowserError> {
        let (id, state, events, _) = self.channels();
        let context = self.context.clone();
        let handler = NavigationCompletedEventHandler::create(Box::new(move |_core, args| {
            let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let mut success = BOOL::default();
            let mut navigation = 0;
            let read = unsafe {
                args.IsSuccess(&mut success)
                    .and_then(|()| args.NavigationId(&mut navigation))
            };
            if let Err(error) = read {
                report(&events, id);
                return Err(error);
            }
            let Some(policy) = context.exclusions() else {
                return Ok(());
            };
            let rules = Rules {
                policy: &policy,
                capture: context.policy().is_some(),
            };
            state.borrow_mut().navigation_completed(
                navigation,
                success.as_bool(),
                rules,
                Instant::now(),
            );
            Ok(())
        }));
        let mut token = 0;
        unsafe { core.add_NavigationCompleted(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)
    }

    fn register_process_failure(&self, core: &ICoreWebView2) -> Result<(), BrowserError> {
        let state = self.state.clone();
        let handler = ProcessFailedEventHandler::create(Box::new(move |_core, _args| {
            state.borrow_mut().process_failed();
            Ok(())
        }));
        let mut token = 0;
        unsafe { core.add_ProcessFailed(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)
    }
}

/// The channels a page's results leave by: its events and the worker's native wake-up.
pub(super) struct Channels {
    pub(super) events: EventSink,
    pub(super) native: NativeSink,
}

/// One hop's facts from `NavigationStarting`, with the web view's Source as the document left.
fn read_start(
    core: &ICoreWebView2,
    args: &ICoreWebView2NavigationStartingEventArgs,
) -> windows::core::Result<Start> {
    let mut raw = PWSTR::null();
    let read = unsafe { args.Uri(&mut raw) };
    let address = take_pwstr(raw);
    read?;
    let mut redirected = BOOL::default();
    let mut navigation = 0;
    unsafe {
        args.IsRedirected(&mut redirected)?;
        args.NavigationId(&mut navigation)?;
    }
    Ok(Start {
        address,
        redirected: redirected.as_bool(),
        navigation,
        source: source_of(core)?,
        at: Instant::now(),
    })
}

/// Whether the web view shows `address` now, ignoring any fragment.
fn shows(core: &ICoreWebView2, address: &str) -> Result<bool, BrowserError> {
    let source = source_of(core).map_err(|_| BrowserError::PageText)?;
    Ok(without_fragment(&source) == without_fragment(address))
}

/// The address the web view shows now.
fn source_of(core: &ICoreWebView2) -> windows::core::Result<String> {
    let mut raw = PWSTR::null();
    let read = unsafe { core.Source(&mut raw) };
    let source = take_pwstr(raw);
    read.map(|()| source)
}

fn is_suspended(core: &ICoreWebView2) -> Result<bool, BrowserError> {
    let core: ICoreWebView2_3 = core.cast().map_err(|_| BrowserError::Suspension)?;
    let mut suspended = BOOL::default();
    unsafe { core.IsSuspended(&mut suspended) }.map_err(|_| BrowserError::Suspension)?;
    Ok(suspended.as_bool())
}

fn cancelled_notice(events: &EventSink, id: u64, cancelled: bool) {
    if cancelled {
        events.emit(BrowserEvent::PageText {
            id,
            outcome: PageTextOutcome::Cancelled,
        });
    }
}

fn report(events: &EventSink, id: u64) {
    events.emit(BrowserEvent::Error {
        tab: Some(id),
        error: BrowserError::PageText,
    });
}

/// The wall-clock time of a monotonic instant; `None` if the clock cannot express it.
fn wall_clock(instant: Instant) -> Option<SystemTime> {
    SystemTime::now().checked_sub(instant.elapsed())
}

//! Reads an eligible page's rendered text with one DevTools snapshot, never a script (P1-25);
//! the facts eligibility needs are observed from navigation and response events only.

mod snapshot;

use std::{cell::RefCell, rc::Rc, time::Instant};

use atlas_core::{
    DocumentFacts, EligibleDocument, Ineligible, ObservedResponse, PageTextPolicy, judge,
    same_document_due,
};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2_2, ICoreWebView2_3,
        ICoreWebView2CallDevToolsProtocolMethodCompletedHandler, ICoreWebView2HttpRequestHeaders,
        ICoreWebView2HttpResponseHeaders, ICoreWebView2WebResourceResponseReceivedEventArgs,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler, ProcessFailedEventHandler,
    SourceChangedEventHandler, WebResourceResponseReceivedEventHandler, take_pwstr,
};
use windows::{
    Win32::Foundation::{E_POINTER, E_UNEXPECTED},
    core::{BOOL, HSTRING, Interface, PWSTR, w},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, CapturedText, EventSink, NativeSink, PageTextOutcome};

/// The capture policy every page on the worker reads; `None` means capture is off.
pub(super) type SharedPolicy = Rc<RefCell<Option<Rc<PageTextPolicy>>>>;

/// The most top-level responses remembered for one navigation.
const RESPONSE_RECORDS: usize = 32;
/// The most values read for one header; more makes the response unreadable (fails closed).
const HEADER_VALUES: usize = 64;

/// What one tab has observed about its current document, all on the page thread.
#[derive(Default)]
struct TextState {
    /// Rises on every navigation start, address change and process failure (step 8).
    generation: u64,
    navigation: Option<u64>,
    /// The address the navigation requested most recently.
    requested: Option<String>,
    /// Addresses requested before `requested` in this navigation.
    redirects: Vec<String>,
    responses: Vec<ObservedResponse>,
    /// The address whose response produced the document.
    document: Option<String>,
    /// The address shown now; differs from `document` after a History API change.
    address: Option<String>,
    loaded: bool,
    due: Option<Instant>,
    /// A capture that fell due while the page was suspended waits for it to resume.
    deferred: bool,
    last_capture: Option<Instant>,
    in_flight: bool,
}

impl TextState {
    /// Invalidates anything judged so far; reports whether a scheduled capture was cancelled.
    fn advance(&mut self) -> bool {
        self.generation += 1;
        self.deferred = false;
        self.due.take().is_some()
    }

    fn navigation_started(&mut self, address: String, redirected: bool, navigation: u64) -> bool {
        let cancelled = self.advance();
        if redirected && self.navigation == Some(navigation) {
            self.redirects.extend(self.requested.take());
        } else {
            self.navigation = Some(navigation);
            self.redirects.clear();
            self.responses.clear();
        }
        self.requested = Some(address);
        self.document = None;
        self.address = None;
        self.loaded = false;
        cancelled
    }

    /// Whether a response is the one the current navigation is waiting for.
    fn wants_response(&self, address: &str) -> bool {
        self.responses.len() < RESPONSE_RECORDS
            && self
                .requested
                .as_deref()
                .is_some_and(|requested| without_fragment(requested) == without_fragment(address))
    }

    fn source_changed(
        &mut self,
        source: String,
        new_document: bool,
        policy: &PageTextPolicy,
    ) -> bool {
        let cancelled = self.advance();
        if new_document {
            self.document = Some(source.clone());
            self.address = Some(source);
            self.loaded = false;
            return cancelled;
        }
        self.address = Some(source);
        if self.loaded && self.document.is_some() {
            let due = same_document_due(self.last_capture, Instant::now(), policy.settle_delay());
            self.due = Some(due);
        }
        cancelled
    }

    fn navigation_completed(&mut self, navigation: u64, success: bool, policy: &PageTextPolicy) {
        if self.navigation != Some(navigation) {
            return;
        }
        // A reload or a visit to the same address raises no SourceChanged (F14 found this);
        // the document is then the last requested address, still judged against its response.
        if success && self.document.is_none() {
            self.document = self.requested.clone();
            self.address = self.requested.clone();
        }
        self.loaded = success && self.document.is_some();
        if self.loaded {
            self.due = Some(Instant::now() + policy.settle_delay());
        }
    }

    fn take_due(&mut self, now: Instant) -> bool {
        if self.in_flight || !self.due.is_some_and(|due| due <= now) {
            return false;
        }
        self.due = None;
        true
    }

    /// Steps 1 to 8, from what this tab observed.
    fn judge(&self, policy: Option<&PageTextPolicy>) -> Result<EligibleDocument, Ineligible> {
        let (Some(address), Some(document)) = (&self.address, &self.document) else {
            return Err(Ineligible::NoResponse);
        };
        // The final requested hop is checked with the earlier ones.
        let redirects: Vec<String> = self
            .redirects
            .iter()
            .chain(self.requested.as_ref())
            .cloned()
            .collect();
        let facts = DocumentFacts {
            address,
            document_address: document,
            redirects: &redirects,
            responses: &self.responses,
            generation: self.generation,
        };
        judge(policy, facts, self.generation)
    }
}

/// One page's capture: its observed state and the channels its results leave by.
pub(super) struct PageText {
    id: u64,
    policy: SharedPolicy,
    state: Rc<RefCell<TextState>>,
    events: EventSink,
    native: NativeSink,
}

impl PageText {
    /// Subscribes only when capture is on as the page is created, so a page created with
    /// capture off is never observed or read.
    pub(super) fn register(
        view: &WebView,
        id: u64,
        policy: SharedPolicy,
        events: EventSink,
        native: NativeSink,
    ) -> Result<Option<Self>, BrowserError> {
        if policy.borrow().is_none() {
            return Ok(None);
        }
        let text = Self {
            id,
            policy,
            state: Rc::new(RefCell::new(TextState::default())),
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
        self.state.borrow().due
    }

    /// A capture deferred while suspended runs once the page is shown again.
    pub(super) fn resumed(&self) {
        let mut state = self.state.borrow_mut();
        if std::mem::take(&mut state.deferred) {
            state.due = Some(Instant::now());
        }
    }

    /// Judges and captures a document whose settle delay has passed.
    pub(super) fn tick(&self, view: &WebView) -> Result<(), BrowserError> {
        if !self.state.borrow_mut().take_due(Instant::now()) {
            return Ok(());
        }
        if is_suspended(&view.webview())? {
            self.state.borrow_mut().deferred = true;
            return Ok(());
        }
        let policy = self.policy.borrow().clone();
        let verdict = self.state.borrow().judge(policy.as_deref());
        match verdict {
            Err(reason) => {
                self.emit(PageTextOutcome::Ineligible(reason));
                Ok(())
            }
            Ok(document) => self.capture(&view.webview(), document),
        }
    }

    /// The one place a DevTools call is made; it announces its target first, so a test can
    /// count every call and its address.
    fn capture(
        &self,
        core: &ICoreWebView2,
        document: EligibleDocument,
    ) -> Result<(), BrowserError> {
        self.emit(PageTextOutcome::Requested {
            address: document.address().to_owned(),
        });
        self.state.borrow_mut().in_flight = true;
        let handler = self.completion(document);
        let parameters = HSTRING::from(snapshot::CAPTURE_PARAMETERS);
        let call = unsafe {
            core.CallDevToolsProtocolMethod(
                w!("DOMSnapshot.captureSnapshot"),
                &parameters,
                &handler,
            )
        };
        if call.is_err() {
            self.state.borrow_mut().in_flight = false;
            return Err(BrowserError::PageText);
        }
        Ok(())
    }

    /// Drops the response unread if the tab changed, otherwise parses it off this thread.
    fn completion(
        &self,
        document: EligibleDocument,
    ) -> ICoreWebView2CallDevToolsProtocolMethodCompletedHandler {
        let (id, state, events, native) = self.channels();
        let started = Instant::now();
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
            let generation = {
                let mut state = state.borrow_mut();
                state.in_flight = false;
                state.last_capture = Some(Instant::now());
                state.generation
            };
            let call_time = started.elapsed();
            let outcome = if result.is_err() {
                Some(PageTextOutcome::Failed)
            } else if !document.is_current(generation) {
                Some(PageTextOutcome::Superseded)
            } else {
                None
            };
            match outcome {
                Some(outcome) => events.emit(BrowserEvent::PageText { id, outcome }),
                None => read_off_thread(id, json, document, call_time, events, native),
            }
            Ok(())
        }))
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

    /// Records each requested address, including every redirect hop (step 6).
    fn register_navigation_starting(&self, core: &ICoreWebView2) -> Result<(), BrowserError> {
        let (id, state, events, _) = self.channels();
        let handler = NavigationStartingEventHandler::create(Box::new(move |_core, args| {
            let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let mut raw = PWSTR::null();
            let mut redirected = BOOL::default();
            let mut navigation = 0;
            let read = unsafe {
                args.Uri(&mut raw)
                    .and_then(|()| args.IsRedirected(&mut redirected))
                    .and_then(|()| args.NavigationId(&mut navigation))
            };
            let address = take_pwstr(raw);
            if let Err(error) = read {
                // Without the hop, the chain is unknown: forget the document so it fails closed.
                state
                    .borrow_mut()
                    .navigation_started(String::new(), false, u64::MAX);
                report(&events, id);
                return Err(error);
            }
            let cancelled =
                state
                    .borrow_mut()
                    .navigation_started(address, redirected.as_bool(), navigation);
            cancelled_notice(&events, id, cancelled);
            Ok(())
        }));
        let mut token = 0;
        unsafe { core.add_NavigationStarting(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)
    }

    /// Reads status and three named headers of the current navigation's responses; no
    /// request filter, no body, and no other header is read.
    fn register_responses(&self, core: &ICoreWebView2) -> Result<(), BrowserError> {
        let (id, state, events, _) = self.channels();
        let handler =
            WebResourceResponseReceivedEventHandler::create(Box::new(move |_core, args| {
                let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                match observed_response(&args, |address| state.borrow().wants_response(address)) {
                    Ok(Some(response)) => state.borrow_mut().responses.push(response),
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

    /// A new document or a History API change; the latter may schedule a rate-limited capture.
    fn register_source(&self, core: &ICoreWebView2) -> Result<(), BrowserError> {
        let (id, state, events, _) = self.channels();
        let policy = self.policy.clone();
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
            let Some(policy) = policy.borrow().clone() else {
                state.borrow_mut().advance();
                return read;
            };
            if let Err(error) = read {
                state
                    .borrow_mut()
                    .navigation_started(String::new(), false, u64::MAX);
                report(&events, id);
                return Err(error);
            }
            let cancelled =
                state
                    .borrow_mut()
                    .source_changed(source, new_document.as_bool(), &policy);
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
        let policy = self.policy.clone();
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
            let Some(policy) = policy.borrow().clone() else {
                return Ok(());
            };
            state
                .borrow_mut()
                .navigation_completed(navigation, success.as_bool(), &policy);
            Ok(())
        }));
        let mut token = 0;
        unsafe { core.add_NavigationCompleted(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)
    }

    fn register_process_failure(&self, core: &ICoreWebView2) -> Result<(), BrowserError> {
        let state = self.state.clone();
        let handler = ProcessFailedEventHandler::create(Box::new(move |_core, _args| {
            let mut state = state.borrow_mut();
            state.advance();
            state.loaded = false;
            Ok(())
        }));
        let mut token = 0;
        unsafe { core.add_ProcessFailed(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)
    }
}

/// Parses on a short-lived thread and posts the outcome back to the page thread.
fn read_off_thread(
    id: u64,
    json: String,
    document: EligibleDocument,
    call_time: std::time::Duration,
    events: EventSink,
    native: NativeSink,
) {
    let response_bytes = json.len();
    let reader_events = events.clone();
    let spawned = std::thread::Builder::new()
        .name("atlas-page-text".into())
        .spawn(move || {
            let started = Instant::now();
            let outcome = match snapshot::read(&json, &document) {
                Ok(read) => PageTextOutcome::Captured(CapturedText {
                    address: document.address().to_owned(),
                    text: read.text,
                    documents: read.documents,
                    truncated: read.truncated,
                    response_bytes,
                    call_time,
                    parse_time: started.elapsed(),
                }),
                Err(reason) => PageTextOutcome::Discarded(reason),
            };
            drop(json);
            if let Err(error) = native.post(BrowserEvent::PageText { id, outcome }, &reader_events)
            {
                // The page thread has stopped, so nothing remains to receive this result.
                eprintln!("{error}");
            }
        });
    if spawned.is_err() {
        events.emit(BrowserEvent::PageText {
            id,
            outcome: PageTextOutcome::Failed,
        });
    }
}

/// Reads one response if `wanted` accepts its address; otherwise reads nothing more.
fn observed_response(
    args: &ICoreWebView2WebResourceResponseReceivedEventArgs,
    wanted: impl Fn(&str) -> bool,
) -> windows::core::Result<Option<ObservedResponse>> {
    let request = unsafe { args.Request() }?;
    let mut raw = PWSTR::null();
    let read = unsafe { request.Uri(&mut raw) };
    let address = take_pwstr(raw);
    read?;
    if !wanted(&address) {
        return Ok(None);
    }
    let destination = request_header(&unsafe { request.Headers() }?, w!("Sec-Fetch-Dest"))?;
    let response = unsafe { args.Response() }?;
    let mut status = 0;
    unsafe { response.StatusCode(&mut status) }?;
    let headers = unsafe { response.Headers() }?;
    let status = u16::try_from(status).map_err(|_| windows::core::Error::from(E_UNEXPECTED))?;
    Ok(Some(ObservedResponse {
        address,
        status,
        content_type: response_header(&headers, w!("Content-Type"))?,
        // WebView2 splits one line's directives but drops a repeated header's later lines
        // (F2); see `docs/p1-25-page-text.md`.
        cache_control: response_headers(&headers, w!("Cache-Control"))?,
        destination,
    }))
}

fn request_header(
    headers: &ICoreWebView2HttpRequestHeaders,
    name: windows::core::PCWSTR,
) -> windows::core::Result<Option<String>> {
    let mut present = BOOL::default();
    unsafe { headers.Contains(name, &mut present) }?;
    if !present.as_bool() {
        return Ok(None);
    }
    let mut raw = PWSTR::null();
    let read = unsafe { headers.GetHeader(name, &mut raw) };
    let value = take_pwstr(raw);
    read.map(|()| Some(value))
}

fn response_header(
    headers: &ICoreWebView2HttpResponseHeaders,
    name: windows::core::PCWSTR,
) -> windows::core::Result<Option<String>> {
    let mut present = BOOL::default();
    unsafe { headers.Contains(name, &mut present) }?;
    if !present.as_bool() {
        return Ok(None);
    }
    let mut raw = PWSTR::null();
    let read = unsafe { headers.GetHeader(name, &mut raw) };
    let value = take_pwstr(raw);
    read.map(|()| Some(value))
}

/// Every value WebView2 reports for a response header, in order.
fn response_headers(
    headers: &ICoreWebView2HttpResponseHeaders,
    name: windows::core::PCWSTR,
) -> windows::core::Result<Vec<String>> {
    let iterator = unsafe { headers.GetHeaders(name) }?;
    let mut values = Vec::new();
    loop {
        let mut current = BOOL::default();
        unsafe { iterator.HasCurrentHeader(&mut current) }?;
        if !current.as_bool() {
            return Ok(values);
        }
        let (mut raw_name, mut raw_value) = (PWSTR::null(), PWSTR::null());
        let read = unsafe { iterator.GetCurrentHeader(&mut raw_name, &mut raw_value) };
        let (_name, value) = (take_pwstr(raw_name), take_pwstr(raw_value));
        read?;
        values.push(value);
        if values.len() > HEADER_VALUES {
            return Err(windows::core::Error::from(E_UNEXPECTED));
        }
        // As in Microsoft's sample, HasCurrentHeader alone decides when the values end.
        let mut _has_next = BOOL::default();
        unsafe { iterator.MoveNext(&mut _has_next) }?;
    }
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

fn without_fragment(address: &str) -> &str {
    address.split('#').next().unwrap_or(address)
}

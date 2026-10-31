//! P1-25 on Linux: observes each page's navigation facts for the shared eligibility check, and
//! reads an eligible page's HTML as sent; nothing is added to or run inside the page.

#[path = "../browser/page_text/state.rs"]
mod state;

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::{Rc, Weak},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant, SystemTime},
};

use gtk::glib::{self, clone::Downgrade};
use syrinx_core::{
    EligibleDocument, Ineligible, LeftDocument, ObservedResponse, PageTextPolicy, SentHtmlRefusal,
    sent_html_text,
};
use webkit2gtk::{URIResponseExt, WebResourceExt, WebView, WebViewExt};

use super::{BrowserEvent, CapturedText, DiscardReason, EventSink, PageTextOutcome};
use state::{Rules, Start, TextState, without_fragment};

/// WebKitGTK has no host-side DOM snapshot, so Linux could only read a page's HTML as the
/// server sent it, a method the owner has not approved (docs/page-text-indexing.md, candidate
/// 4's fallback). Until then every capture that falls due reports capture as off.
pub(super) const READS_SENT_HTML: bool = false;
/// The most pages read at once; a capture beyond it is skipped as busy, as on Windows.
const READERS: usize = 2;
/// The method recorded when the request that began a navigation was not seen; it fails closed
/// as a form submission would.
const UNSEEN_METHOD: &str = "UNSEEN";
/// The most recent navigation requests kept for matching.
const ASKED_RECORDS: usize = 8;

static READING: AtomicUsize = AtomicUsize::new(0);

/// The capture policy (`None` means off) and the last exclusion lists, which keep deciding
/// taint while capture is off; shared by every page.
#[derive(Clone, Default)]
pub(super) struct TextContext {
    policy: Rc<RefCell<Option<Rc<PageTextPolicy>>>>,
    last: Rc<RefCell<Option<Rc<PageTextPolicy>>>>,
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

    /// The policy a due capture is judged by: none, so capture off, until reading is approved.
    fn reading_policy(&self) -> Option<Rc<PageTextPolicy>> {
        self.policy().filter(|_| READS_SENT_HTML)
    }

    fn exclusions(&self) -> Option<Rc<PageTextPolicy>> {
        self.last.borrow().clone()
    }
}

/// One page's observed facts and its capture; cheap to clone into its signal handlers.
#[derive(Clone)]
pub(super) struct PageText(Rc<Inner>);

struct Inner {
    id: u64,
    context: TextContext,
    state: RefCell<TextState>,
    events: EventSink,
    view: glib::WeakRef<WebView>,
    navigation: Cell<u64>,
    /// Between a load's start and its commit, address changes belong to the load.
    provisional: Cell<bool>,
    /// Addresses the current load has named so far, and the latest of them.
    hops: Cell<u32>,
    last_hop: RefCell<String>,
    /// The address of the document the view shows, the one a new navigation leaves.
    committed: RefCell<String>,
    /// The latest requests WebKit asked about, with their methods, to match the load each
    /// starts; frames ask too, so a few are kept.
    asked: RefCell<VecDeque<(String, String)>>,
    /// The method of the request that began the current navigation.
    method: RefCell<String>,
    /// When the wake-up already scheduled will run.
    wake: Cell<Option<Instant>>,
}

impl PageText {
    /// Observes a page only when capture is on as it is created, as on Windows. A reopened
    /// page or a popup passes `starts_tainted`.
    pub(super) fn register(
        view: &WebView,
        id: u64,
        context: &TextContext,
        starts_tainted: bool,
        events: EventSink,
    ) -> Option<Self> {
        context.policy()?;
        Some(Self(Rc::new(Inner {
            id,
            context: context.clone(),
            state: RefCell::new(TextState::seeded(starts_tainted)),
            events,
            view: view.downgrade(),
            navigation: Cell::new(0),
            provisional: Cell::new(false),
            hops: Cell::new(0),
            last_hop: RefCell::default(),
            committed: RefCell::new(view.uri().map(String::from).unwrap_or_default()),
            asked: RefCell::default(),
            method: RefCell::new(UNSEEN_METHOD.to_owned()),
            wake: Cell::new(None),
        })))
    }

    /// Syrinx itself asked the tab to load `address`; only such a navigation clears taint.
    pub(super) fn syrinx_navigation(&self, address: &str) {
        self.0
            .state
            .borrow_mut()
            .syrinx_navigation(address, Instant::now());
    }

    /// Syrinx went back, forward or reloaded instead of loading its request.
    pub(super) fn syrinx_navigation_cancelled(&self) {
        self.0.state.borrow_mut().syrinx_navigation_cancelled();
    }

    /// Whether a popup this page opens starts tainted; an unreadable address fails closed.
    pub(super) fn popup_tainted(&self) -> bool {
        if self.0.state.borrow().tainted() {
            return true;
        }
        let Some(policy) = self.0.context.exclusions() else {
            return true;
        };
        let source = self.0.committed.borrow();
        let opener = LeftDocument {
            address: &source,
            tainted: false,
        };
        policy.taints(Some(opener), &[])
    }

    /// WebKit asks before every navigation, of the page or of a frame; the request is kept
    /// so the load it starts knows its method.
    pub(super) fn asked(&self, address: String, method: String) {
        let mut asked = self.0.asked.borrow_mut();
        if asked.len() == ASKED_RECORDS {
            asked.pop_front();
        }
        asked.push_back((address, method));
    }

    /// The page's main frame began a load. WebKit may name the address only in a later
    /// provisional address change (a link or a page script), so the first hop waits for an
    /// address other than the document being left; a reload's is taken at commit.
    pub(super) fn started(&self, view: &WebView) {
        self.0.navigation.set(self.0.navigation.get() + 1);
        self.0.provisional.set(true);
        self.0.hops.set(0);
        self.provisional_address(view);
    }

    /// The load's address changed before commit: its first address, or a redirect.
    pub(super) fn provisional_address(&self, view: &WebView) {
        let Some(address) = view.uri().map(String::from) else {
            return;
        };
        if self.0.hops.get() == 0 {
            if address != *self.0.committed.borrow() {
                self.first_hop(address);
            }
            return;
        }
        if address != *self.0.last_hop.borrow() {
            self.hop(address, true);
        }
    }

    /// The navigation's first address; its method is that of the request WebKit asked about.
    fn first_hop(&self, address: String) {
        let method = self
            .0
            .asked
            .borrow()
            .iter()
            .rev()
            .find(|(asked, _)| without_fragment(asked) == without_fragment(&address))
            .map_or_else(|| UNSEEN_METHOD.to_owned(), |(_, method)| method.clone());
        self.0.asked.borrow_mut().clear();
        *self.0.method.borrow_mut() = method;
        self.hop(address, false);
    }

    /// A load that commits without ever naming an address leaves its chain unknown, so the
    /// navigation is forgotten and the tab tainted (fail closed).
    fn forget_navigation(&self) {
        let cancelled = self.0.state.borrow_mut().forget_navigation(Instant::now());
        if cancelled {
            self.emit(PageTextOutcome::Cancelled);
        }
    }

    /// The page's web process answers again; a capture it held back runs now.
    pub(super) fn responsive(&self) {
        self.0.state.borrow_mut().resumed(Instant::now());
        self.schedule();
    }

    /// The main document committed: its response is recorded, then the document itself.
    pub(super) fn committed(&self, view: &WebView) {
        self.0.provisional.set(false);
        let Some(address) = view.uri().map(String::from) else {
            self.forget_navigation();
            return;
        };
        if self.0.hops.get() == 0 {
            self.first_hop(address.clone());
        }
        if let Some(response) = self.main_response(view) {
            self.0.state.borrow_mut().record_response(response);
        }
        self.source_changed(address, true);
    }

    /// The page changed its own address without a load: a fragment or the History API.
    pub(super) fn same_document(&self, view: &WebView) {
        if self.0.provisional.get() {
            return;
        }
        let address = view.uri().map(String::from).unwrap_or_default();
        if address == *self.0.committed.borrow() {
            return;
        }
        self.source_changed(address, false);
        self.schedule();
    }

    pub(super) fn finished(&self, success: bool) {
        self.0.provisional.set(false);
        let Some(policy) = self.0.context.exclusions() else {
            return;
        };
        let rules = Rules {
            policy: &policy,
            capture: self.0.context.policy().is_some(),
        };
        let navigation = self.0.navigation.get();
        self.0
            .state
            .borrow_mut()
            .navigation_completed(navigation, success, rules, Instant::now());
        self.schedule();
    }

    pub(super) fn process_failed(&self) {
        self.0.state.borrow_mut().process_failed();
    }

    fn hop(&self, address: String, redirected: bool) {
        self.0.hops.set(self.0.hops.get() + 1);
        self.0.last_hop.replace(address.clone());
        let start = Start {
            address,
            redirected,
            navigation: self.0.navigation.get(),
            source: self.0.committed.borrow().clone(),
            at: Instant::now(),
        };
        let cancelled = self.0.state.borrow_mut().navigation_started(start);
        if cancelled {
            self.emit(PageTextOutcome::Cancelled);
        }
    }

    fn source_changed(&self, address: String, new_document: bool) {
        *self.0.committed.borrow_mut() = address.clone();
        let Some(policy) = self.0.context.exclusions() else {
            self.0.state.borrow_mut().advance();
            return;
        };
        let rules = Rules {
            policy: &policy,
            capture: self.0.context.policy().is_some(),
        };
        let cancelled =
            self.0
                .state
                .borrow_mut()
                .source_changed(address, new_document, rules, Instant::now());
        if cancelled {
            self.emit(PageTextOutcome::Cancelled);
        }
    }

    /// The main resource's status and named headers; its body is not read here.
    fn main_response(&self, view: &WebView) -> Option<ObservedResponse> {
        let response = view.main_resource()?.response()?;
        let headers = response.http_headers();
        let header = |name: &str| {
            headers
                .as_ref()
                .and_then(|headers| headers.list(name))
                .map(String::from)
        };
        Some(ObservedResponse {
            address: response.uri().map(String::from).unwrap_or_default(),
            status: u16::try_from(response.status_code()).unwrap_or(0),
            content_type: header("Content-Type"),
            // libsoup joins a repeated header's lines with commas, so none is lost (cf. F2).
            cache_control: header("Cache-Control").into_iter().collect(),
            // The main resource is the page's own document by definition.
            destination: Some("document".to_owned()),
            method: self.0.method.borrow().clone(),
        })
    }

    /// Arms one wake-up for the next capture that falls due.
    fn schedule(&self) {
        let Some(due) = self.0.state.borrow().due() else {
            return;
        };
        if self.0.wake.get().is_some_and(|wake| wake <= due) {
            return;
        }
        self.0.wake.set(Some(due));
        let weak: Weak<Inner> = Rc::downgrade(&self.0);
        let delay = due.saturating_duration_since(Instant::now());
        glib::timeout_add_local_once(delay, move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let text = Self(inner);
            text.0.wake.set(None);
            text.tick();
            text.schedule();
        });
    }

    /// Judges and reads a document whose settle delay has passed.
    fn tick(&self) {
        let Some(scheduled) = self.0.state.borrow_mut().take_due(Instant::now()) else {
            return;
        };
        // A page whose web process stopped answering is read once it answers again.
        if self
            .0
            .view
            .upgrade()
            .is_some_and(|view| !view.is_web_process_responsive())
        {
            self.0.state.borrow_mut().defer();
            return;
        }
        let policy = self.0.context.reading_policy();
        let verdict = self.0.state.borrow().judge(policy.as_deref(), scheduled);
        let visited_at = self.0.state.borrow().visited().and_then(wall_clock);
        match verdict {
            Err(reason) => self.emit(PageTextOutcome::Ineligible(reason)),
            Ok(document) => self.read(document, visited_at),
        }
    }

    /// The one place a page's body is read; it announces its target first, as on Windows.
    fn read(&self, document: EligibleDocument, visited_at: Option<SystemTime>) {
        let Some(view) = self.0.view.upgrade() else {
            return;
        };
        if !document.is_current(self.0.state.borrow().generation()) {
            self.emit(PageTextOutcome::Ineligible(Ineligible::Stale));
            return;
        }
        // A navigation that never committed leaves the previous document on screen.
        let shown = view.uri().map(String::from).unwrap_or_default();
        let Some(resource) = view
            .main_resource()
            .filter(|_| without_fragment(&shown) == without_fragment(document.address()))
        else {
            self.emit(PageTextOutcome::NotCommitted);
            return;
        };
        self.emit(PageTextOutcome::Requested {
            address: document.address().to_owned(),
        });
        self.0.state.borrow_mut().call_started();
        let text = self.clone();
        let started = Instant::now();
        resource.data(None::<&gtk::gio::Cancellable>, move |body| {
            text.arrived(body, document, started, visited_at);
        });
    }

    /// Drops the body unread if the tab changed or capture was switched off; otherwise
    /// parses it off the interface thread.
    fn arrived(
        &self,
        body: Result<Vec<u8>, glib::Error>,
        document: EligibleDocument,
        started: Instant,
        visited_at: Option<SystemTime>,
    ) {
        let generation = self.0.state.borrow_mut().call_finished(Instant::now());
        self.schedule();
        let current = self.0.context.reading_policy().is_some() && document.is_current(generation);
        let outcome = match body {
            Err(_) => PageTextOutcome::Failed,
            Ok(_) if !current => PageTextOutcome::Superseded,
            Ok(body) => match self.parse(body, document, started.elapsed(), visited_at) {
                Ok(()) => return,
                Err(outcome) => outcome,
            },
        };
        self.emit(outcome);
    }

    /// Parses on a short-lived thread; at most `READERS` run at once.
    fn parse(
        &self,
        body: Vec<u8>,
        document: EligibleDocument,
        call_time: Duration,
        visited_at: Option<SystemTime>,
    ) -> Result<(), PageTextOutcome> {
        if READING.fetch_add(1, Ordering::AcqRel) >= READERS {
            READING.fetch_sub(1, Ordering::AcqRel);
            return Err(PageTextOutcome::Discarded(DiscardReason::Busy));
        }
        let (id, events) = (self.0.id, self.0.events.clone());
        let spawned = std::thread::Builder::new()
            .name("syrinx-page-text".into())
            .spawn(move || {
                let outcome = captured(body, &document, call_time, visited_at);
                READING.fetch_sub(1, Ordering::AcqRel);
                events.emit(BrowserEvent::PageText { id, outcome });
            });
        if spawned.is_err() {
            READING.fetch_sub(1, Ordering::AcqRel);
            return Err(PageTextOutcome::Failed);
        }
        Ok(())
    }

    fn emit(&self, outcome: PageTextOutcome) {
        self.0.events.emit(BrowserEvent::PageText {
            id: self.0.id,
            outcome,
        });
    }
}

/// Steps 10 and 11 on the body, with the measurements Windows reports.
fn captured(
    body: Vec<u8>,
    document: &EligibleDocument,
    call_time: Duration,
    visited_at: Option<SystemTime>,
) -> PageTextOutcome {
    let started = Instant::now();
    let response_bytes = body.len();
    let read = sent_html_text(&body);
    drop(body);
    match read {
        Ok(read) => PageTextOutcome::Captured(CapturedText {
            address: document.address().to_owned(),
            text: read.text,
            documents: 1,
            truncated: read.truncated,
            response_bytes,
            call_time,
            parse_time: started.elapsed(),
            visited_at,
        }),
        Err(refusal) => PageTextOutcome::Discarded(discard_reason(refusal)),
    }
}

fn discard_reason(refusal: SentHtmlRefusal) -> DiscardReason {
    match refusal {
        SentHtmlRefusal::TooLarge => DiscardReason::TooLarge,
        SentHtmlRefusal::NotUtf8 => DiscardReason::Malformed,
        SentHtmlRefusal::SignInOrPayment => DiscardReason::SignInOrPayment,
    }
}

/// The wall-clock time of a monotonic instant; `None` if the clock cannot express it.
fn wall_clock(instant: Instant) -> Option<SystemTime> {
    SystemTime::now().checked_sub(instant.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_stays_off_until_the_owner_approves_it() {
        let context = TextContext::default();
        let policy = PageTextPolicy::new::<&str>(&[]).expect("synthetic policy");
        context.set_policy(Some(policy));
        assert!(context.policy().is_some(), "facts are still observed");
        assert!(
            context.reading_policy().is_none(),
            "but every capture is judged off"
        );
    }

    #[test]
    fn a_refused_body_maps_to_the_windows_discard_reasons() {
        assert_eq!(
            discard_reason(SentHtmlRefusal::SignInOrPayment),
            DiscardReason::SignInOrPayment
        );
        assert_eq!(
            discard_reason(SentHtmlRefusal::TooLarge),
            DiscardReason::TooLarge
        );
        assert_eq!(
            discard_reason(SentHtmlRefusal::NotUtf8),
            DiscardReason::Malformed
        );
    }

    #[test]
    fn a_body_is_read_into_captured_text() {
        let policy = PageTextPolicy::new::<&str>(&[]).expect("synthetic policy");
        let address = "https://plain.example.test/page";
        let mut state = TextState::default();
        let rules = Rules {
            policy: &policy,
            capture: true,
        };
        let now = Instant::now();
        state.navigation_started(Start {
            address: address.to_owned(),
            redirected: false,
            navigation: 1,
            source: "about:blank".to_owned(),
            at: now,
        });
        state.record_response(ObservedResponse {
            address: address.to_owned(),
            status: 200,
            content_type: Some("text/html; charset=utf-8".to_owned()),
            cache_control: Vec::new(),
            destination: Some("document".to_owned()),
            method: "GET".to_owned(),
        });
        state.source_changed(address.to_owned(), true, rules, now);
        state.navigation_completed(1, true, rules, now);
        let scheduled = state
            .take_due(now + Duration::from_secs(3))
            .expect("a capture falls due");
        let document = state.judge(Some(&policy), scheduled).expect("eligible");
        let outcome = captured(
            b"<p>Synthetic words</p>".to_vec(),
            &document,
            Duration::ZERO,
            None,
        );
        let PageTextOutcome::Captured(text) = outcome else {
            panic!("expected captured text");
        };
        assert_eq!(text.text, "Synthetic words");
        assert_eq!(text.address, address);
    }
}

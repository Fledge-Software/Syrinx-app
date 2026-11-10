//! P1-25 on Linux: observes each page's navigation facts for the shared eligibility check and
//! taint rules. No page is read: the owner has not approved a reading method (#219).

#[path = "../browser/page_text/state.rs"]
mod state;

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::{Rc, Weak},
    time::Instant,
};

use gtk::glib::{self, clone::Downgrade};
use syrinx_core::{Ineligible, LeftDocument, ObservedResponse, PageTextPolicy};
use webkit2gtk::{URIResponseExt, WebResourceExt, WebView, WebViewExt};

use super::{BrowserEvent, EventSink, PageTextOutcome};
use state::{Rules, Start, TextState, without_fragment};

/// The method recorded when the request that began a navigation was not seen; it fails closed
/// as a form submission would.
const UNSEEN_METHOD: &str = "UNSEEN";
/// The most recent navigation requests kept for matching.
const ASKED_RECORDS: usize = 8;

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

    /// Judges a document whose settle delay has passed. Reading a page is not approved on
    /// Linux (#219, `docs/dev/linux-page-text-proposal.md`), so it is judged with capture off
    /// and only the reason it was not read is reported.
    fn tick(&self) {
        let Some(scheduled) = self.0.state.borrow_mut().take_due(Instant::now()) else {
            return;
        };
        // A page whose web process stopped answering is judged once it answers again.
        if self
            .0
            .view
            .upgrade()
            .is_some_and(|view| !view.is_web_process_responsive())
        {
            self.0.state.borrow_mut().defer();
            return;
        }
        let reason = match self.0.state.borrow().judge(None, scheduled) {
            Err(reason) => reason,
            // Unreachable without a policy; reported as capture off all the same.
            Ok(_) => Ineligible::CaptureOff,
        };
        self.emit(PageTextOutcome::Ineligible(reason));
    }

    fn emit(&self, outcome: PageTextOutcome) {
        self.0.events.emit(BrowserEvent::PageText {
            id: self.0.id,
            outcome,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_due_capture_is_judged_with_capture_off() {
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
            .take_due(now + std::time::Duration::from_secs(3))
            .expect("a capture falls due");
        assert_eq!(
            state.judge(None, scheduled).err(),
            Some(Ineligible::CaptureOff)
        );
    }
}

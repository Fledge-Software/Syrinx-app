//! Pairs WebKitGTK's count-only Find replies with the Syrinx request each one answers.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};

use super::{BrowserEvent, Shared};

/// The most operations left unanswered before Next and Previous are refused.
///
/// Searches are never refused: the latest query must be kept, and each is one change to the query.
const MAX_IN_FLIGHT: usize = 64;

/// One native Find operation sent to WebKit and not yet answered.
#[derive(Clone, Copy)]
enum Sent {
    Search { request: u64 },
    Step { request: u64, forward: bool },
}

impl Sent {
    fn request(self) -> u64 {
        match self {
            Self::Search { request } | Self::Step { request, .. } => request,
        }
    }
}

/// The current request, its match count once WebKit has answered, and the operations in flight.
///
/// `found-text` and `failed-to-find-text` carry no request identity, so each answers the oldest
/// entry in `sent`. This rests on WebKit answering one page's operations exactly once each, in
/// send order: observed under Xvfb, including a web-process crash, whose pending operations
/// come back as failures before Syrinx sees the crash. Nothing clears `sent`, since a reply
/// still owed would then pair with a later operation; a cross-site process swap with replies
/// in flight is assumed, not yet observed, to keep that order (#245).
pub(super) struct FindState {
    request: Cell<Option<u64>>,
    count: Cell<Option<u32>>,
    active: Cell<u32>,
    sent: RefCell<VecDeque<Sent>>,
    steps_count_matches: bool,
}

impl FindState {
    /// `steps_count_matches` says whether WebKit's step replies carry the real total.
    pub(super) fn new(steps_count_matches: bool) -> Self {
        Self {
            request: Cell::new(None),
            count: Cell::new(None),
            active: Cell::new(0),
            sent: RefCell::default(),
            steps_count_matches,
        }
    }

    /// Make `request` current; an empty query completes at once and sends nothing.
    pub(super) fn begin(&self, request: u64, text: &str, id: u64, shared: &Shared) -> bool {
        self.request.set(Some(request));
        self.count.set(None);
        self.active.set(0);
        if text.is_empty() {
            emit(id, shared, request, 0, 0);
            self.request.set(None);
            return false;
        }
        true
    }

    /// Record the current request's search; call this just before sending it to WebKit.
    pub(super) fn sending_search(&self) {
        if let Some(request) = self.request.get() {
            self.sent.borrow_mut().push_back(Sent::Search { request });
        }
    }

    /// Record a Next or Previous to send, unless none can match or too many are unanswered.
    pub(super) fn sending_step(&self, forward: bool) -> bool {
        let Some(request) = self.request.get() else {
            return false;
        };
        if self.count.get() == Some(0) || self.sent.borrow().len() >= MAX_IN_FLIGHT {
            return false;
        }
        self.sent
            .borrow_mut()
            .push_back(Sent::Step { request, forward });
        true
    }

    /// Retire the current request; replies still in flight are matched and dropped.
    pub(super) fn stop(&self) {
        self.request.set(None);
    }

    /// Apply WebKit's next reply to the operation it answers, and report it if still current.
    pub(super) fn answered(&self, count: u32, id: u64, shared: &Shared) {
        let Some(sent) = self.sent.borrow_mut().pop_front() else {
            return;
        };
        let Some(request) = self
            .request
            .get()
            .filter(|&current| current == sent.request())
        else {
            return;
        };
        match sent {
            Sent::Search { .. } => {
                self.count.set(Some(count));
                self.active.set(0);
            }
            Sent::Step { forward, .. } => self.step(forward, count),
        }
        // Unreachable while replies come in send order: a step's search always answers first.
        let Some(count) = self.count.get() else {
            return;
        };
        emit(
            id,
            shared,
            request,
            count,
            self.active.get().saturating_add(1),
        );
    }

    /// Move to the next or previous match, taking the step's own count where it is real.
    fn step(&self, forward: bool, count: u32) {
        if count == 0 || self.steps_count_matches {
            self.count.set(Some(count));
        }
        let Some(total) = self.count.get().filter(|&total| total > 0) else {
            return;
        };
        // A recount on a changed page can leave fewer matches than the last position.
        let active = self.active.get() % total;
        self.active.set(if forward {
            (active + 1) % total
        } else {
            (active + total - 1) % total
        });
    }
}

/// Whether this WebKitGTK answers Next and Previous with the real match count.
///
/// Before 2.54, `webKitFindControllerPerform` (`WebKitFindController.cpp`) asks only a search
/// to count, so `FindController.cpp` answers every step with a count of one.
pub(super) fn steps_count_matches() -> bool {
    // SAFETY: both functions only return constants compiled into the loaded library.
    let (major, minor) = unsafe {
        (
            webkit2gtk::ffi::webkit_get_major_version(),
            webkit2gtk::ffi::webkit_get_minor_version(),
        )
    };
    (major, minor) >= (2, 54)
}

/// Emit one one-based Find report; no match reports position -1.
fn emit(id: u64, shared: &Shared, request: u64, count: u32, position: u32) {
    let active = if count == 0 {
        -1
    } else {
        i32::try_from(position).unwrap_or(i32::MAX)
    };
    shared.events.emit(BrowserEvent::FindResult {
        id,
        request,
        count: i32::try_from(count).unwrap_or(i32::MAX),
        active,
    });
}

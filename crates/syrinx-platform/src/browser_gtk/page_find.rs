//! Pairs WebKitGTK's count-only Find replies with the Syrinx request each one answers.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};

use super::{BrowserEvent, Shared};

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
/// WebKit answers one page's operations in the order they were sent, exactly once each, so the
/// oldest entry in `sent` is the one the next `found-text` or `failed-to-find-text` answers (#245).
#[derive(Default)]
pub(super) struct FindState {
    request: Cell<Option<u64>>,
    count: Cell<Option<u32>>,
    active: Cell<u32>,
    sent: RefCell<VecDeque<Sent>>,
}

impl FindState {
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

    /// Record the current request's search; call this before WebKit can answer it.
    pub(super) fn searching(&self) {
        if let Some(request) = self.request.get() {
            self.sent.borrow_mut().push_back(Sent::Search { request });
        }
    }

    /// Record a Next or Previous for the current request, unless it is known to have no match.
    pub(super) fn stepping(&self, forward: bool) -> bool {
        let Some(request) = self.request.get() else {
            return false;
        };
        if self.count.get() == Some(0) {
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

    /// Move to the next or previous match; a step's own count is ignored unless it found none.
    ///
    /// WebKitGTK before 2.54 answers every Next and Previous with a count of one, so the total
    /// comes from the search's reply (`FindController.cpp`, `DetermineMatchIndex`).
    fn step(&self, forward: bool, count: u32) {
        if count == 0 {
            self.count.set(Some(0));
            return;
        }
        let Some(total) = self.count.get().filter(|&total| total > 0) else {
            return;
        };
        let active = self.active.get() % total;
        self.active.set(if forward {
            (active + 1) % total
        } else {
            (active + total - 1) % total
        });
    }
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

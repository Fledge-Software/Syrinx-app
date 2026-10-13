//! Stamps when palette frames are presented; only P1-33's acceptance run creates a probe.
//!
//! A stamp is taken in a callback deferred from the palette's render. GPUI runs deferred
//! callbacks after the update that drew and presented the frame, so each stamp follows
//! `Present` returning. The compositor may show the frame up to one refresh later.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

/// One palette opening: from the open action to its first presented frame.
#[derive(Clone, Copy, Debug)]
pub struct OpenTiming {
    /// When `open_palette` called GPUI's `open_window`, after its own preparation.
    pub requested: Option<Instant>,
    /// When the native popup window existed and its view was about to be built.
    pub created: Option<Instant>,
    /// The first render, inside window creation, before anything is presented.
    pub rendered: Option<Instant>,
    pub presented: Option<Instant>,
}

/// One keystroke: from the posted key to the frame showing its ranked rows.
#[derive(Clone, Copy, Debug)]
pub struct KeyTiming {
    /// The palette's request identity for the text this key produced.
    pub request: Option<u64>,
    /// The frame that first showed the changed input text.
    pub echoed: Option<Instant>,
    /// When the browser view queued the worker query.
    pub sent: Option<Instant>,
    /// The worker's own catalogue time for that query.
    pub worker: Option<Duration>,
    /// When the palette accepted the reply.
    pub replied: Option<Instant>,
    /// The frame that first showed the reply's rows.
    pub presented: Option<Instant>,
    /// A newer request replaced this key's request before its reply arrived.
    pub superseded: bool,
    /// A burst follows the newest request to its rows instead of the first one.
    follow_latest: bool,
}

/// Shared between the browser view and its palette; cloning shares the same state.
#[derive(Clone, Default)]
pub struct PresentProbe(Rc<RefCell<ProbeState>>);

#[derive(Default)]
struct ProbeState {
    open: Option<OpenTiming>,
    key: Option<KeyTiming>,
    armed: Stamps,
    /// Stamps whose frame has rendered and now waits for its present.
    pending: Stamps,
    /// Every palette request started, and every reply the palette accepted.
    queries: u64,
    replies: u64,
}

/// Which measurements the next presented frame completes.
#[derive(Clone, Copy, Default)]
struct Stamps {
    open: bool,
    echo: bool,
    rows: bool,
}

impl Stamps {
    fn any(self) -> bool {
        self.open || self.echo || self.rows
    }
}

impl PresentProbe {
    /// Starts timing an opening just before the open action runs.
    pub fn begin_open(&self) {
        let mut state = self.0.borrow_mut();
        state.open = Some(OpenTiming {
            requested: None,
            created: None,
            rendered: None,
            presented: None,
        });
        state.armed = Stamps::default();
        state.pending = Stamps::default();
    }

    /// Called just before `open_palette` calls GPUI's `open_window`.
    pub fn window_requested(&self, at: Instant) {
        if let Some(open) = self.0.borrow_mut().open.as_mut() {
            open.requested.get_or_insert(at);
        }
    }

    /// Called once the native popup exists, before its view is built.
    pub fn window_created(&self, at: Instant) {
        if let Some(open) = self.0.borrow_mut().open.as_mut() {
            open.created.get_or_insert(at);
        }
    }

    /// Called at the start of the first frame after window creation, which presents it.
    pub fn arm_open(&self) {
        self.0.borrow_mut().armed.open = true;
    }

    pub fn open(&self) -> Option<OpenTiming> {
        self.0.borrow().open
    }

    /// Starts timing a keystroke just before it is posted to the palette window.
    pub fn begin_key(&self) {
        self.start_key(false);
    }

    /// Starts a burst: whichever request is newest when the typing stops is the one timed.
    pub fn begin_burst(&self) {
        self.start_key(true);
    }

    fn start_key(&self, follow_latest: bool) {
        let mut state = self.0.borrow_mut();
        state.key = Some(KeyTiming {
            request: None,
            echoed: None,
            sent: None,
            worker: None,
            replied: None,
            presented: None,
            superseded: false,
            follow_latest,
        });
        state.armed = Stamps::default();
        state.pending = Stamps::default();
    }

    /// Requests started and replies accepted so far, for counting superseded requests.
    pub fn counts(&self) -> (u64, u64) {
        let state = self.0.borrow();
        (state.queries, state.replies)
    }

    pub fn key(&self) -> Option<KeyTiming> {
        self.0.borrow().key
    }

    /// Ends the keystroke, so a later palette's queries cannot attach to it.
    pub fn take_key(&self) -> Option<KeyTiming> {
        let mut state = self.0.borrow_mut();
        state.armed = Stamps::default();
        state.pending = Stamps::default();
        state.key.take()
    }

    /// The palette started request `request`; a later one before the reply supersedes it.
    pub fn queried(&self, request: u64) {
        let mut state = self.0.borrow_mut();
        state.queries += 1;
        let Some(key) = state.key.as_mut() else {
            return;
        };
        if key.follow_latest {
            // Anything already measured belonged to an older request.
            key.superseded |= key.request.is_some();
            key.request = Some(request);
            key.sent = None;
            key.worker = None;
            key.replied = None;
            key.presented = None;
            state.armed.rows = false;
            state.pending.rows = false;
            return;
        }
        if key.replied.is_some() {
            return;
        }
        let first = key.request.is_none();
        key.superseded |= !first;
        key.request = Some(request);
        state.armed.echo |= first;
    }

    /// The browser view queued the worker query for palette request `request`.
    pub fn sent(&self, request: u64, at: Instant) {
        self.update_key(request, |key| key.sent = Some(at));
    }

    pub fn worker(&self, request: u64, elapsed: Duration) {
        self.update_key(request, |key| key.worker = Some(elapsed));
    }

    /// The palette accepted the reply for `request`; its next presented frame shows the rows.
    pub fn replied(&self, request: u64, at: Instant) {
        let mut state = self.0.borrow_mut();
        state.replies += 1;
        let Some(key) = state.key.as_mut() else {
            return;
        };
        if key.request != Some(request) || key.replied.is_some() {
            return;
        }
        key.replied = Some(at);
        state.armed.rows = true;
    }

    /// Called from the palette's render; returns whether a present stamp should be deferred.
    pub fn rendered(&self, at: Instant) -> bool {
        let mut state = self.0.borrow_mut();
        if let Some(open) = state.open.as_mut()
            && open.rendered.is_none()
        {
            open.rendered = Some(at);
        }
        let armed = std::mem::take(&mut state.armed);
        state.pending.open |= armed.open;
        state.pending.echo |= armed.echo;
        state.pending.rows |= armed.rows;
        armed.any()
    }

    /// Completes every measurement whose frame has now been presented.
    pub fn presented(&self, at: Instant) {
        let mut state = self.0.borrow_mut();
        let pending = std::mem::take(&mut state.pending);
        if pending.open
            && let Some(open) = state.open.as_mut()
        {
            open.presented.get_or_insert(at);
        }
        let Some(key) = state.key.as_mut() else {
            return;
        };
        if pending.echo {
            key.echoed.get_or_insert(at);
        }
        if pending.rows {
            key.presented.get_or_insert(at);
        }
    }

    fn update_key(&self, request: u64, change: impl FnOnce(&mut KeyTiming)) {
        let mut state = self.0.borrow_mut();
        if let Some(key) = state.key.as_mut()
            && key.request == Some(request)
        {
            change(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn later(start: Instant, milliseconds: u64) -> Instant {
        start + Duration::from_millis(milliseconds)
    }

    #[test]
    fn an_opening_completes_only_on_the_frame_after_window_creation() {
        let probe = PresentProbe::default();
        let start = Instant::now();
        probe.begin_open();
        probe.window_requested(later(start, 1));
        probe.window_created(later(start, 2));
        // Window creation renders without presenting, so nothing is pending yet.
        assert!(!probe.rendered(later(start, 3)));
        probe.presented(later(start, 4));
        assert!(probe.open().expect("opening").presented.is_none());

        probe.arm_open();
        assert!(probe.rendered(later(start, 10)));
        probe.presented(later(start, 12));
        let open = probe.open().expect("opening");
        assert_eq!(open.requested, Some(later(start, 1)));
        assert_eq!(open.created, Some(later(start, 2)));
        assert_eq!(open.rendered, Some(later(start, 3)));
        assert_eq!(open.presented, Some(later(start, 12)));
    }

    #[test]
    fn a_keystroke_records_echo_round_trip_and_rows_in_order() {
        let probe = PresentProbe::default();
        let start = Instant::now();
        probe.begin_key();
        probe.queried(7);
        assert!(probe.rendered(later(start, 1)));
        probe.presented(later(start, 2));
        probe.sent(7, later(start, 1));
        probe.worker(7, Duration::from_millis(3));
        // Replies for other requests are ignored.
        probe.replied(6, later(start, 4));
        assert!(probe.key().expect("key").replied.is_none());
        probe.replied(7, later(start, 5));
        assert!(probe.rendered(later(start, 15)));
        probe.presented(later(start, 17));
        let key = probe.key().expect("key");
        assert_eq!(key.echoed, Some(later(start, 2)));
        assert_eq!(key.worker, Some(Duration::from_millis(3)));
        assert_eq!(key.replied, Some(later(start, 5)));
        assert_eq!(key.presented, Some(later(start, 17)));
        assert!(!key.superseded);
    }

    #[test]
    fn a_refresh_before_the_reply_moves_the_key_to_the_newer_request() {
        let probe = PresentProbe::default();
        let start = Instant::now();
        probe.begin_key();
        probe.queried(7);
        probe.queried(8);
        probe.replied(7, later(start, 4));
        assert!(probe.key().expect("key").replied.is_none());
        probe.replied(8, later(start, 6));
        let key = probe.key().expect("key");
        assert!(key.superseded);
        assert_eq!(key.replied, Some(later(start, 6)));
    }

    #[test]
    fn a_burst_times_only_the_newest_request_and_counts_the_replaced_ones() {
        let probe = PresentProbe::default();
        let start = Instant::now();
        probe.begin_burst();
        probe.queried(1);
        probe.replied(1, later(start, 5));
        assert!(probe.rendered(later(start, 6)));
        probe.queried(2);
        // The older request's frame presents after the newer query; it must not count.
        probe.presented(later(start, 8));
        assert!(probe.key().expect("burst").presented.is_none());
        probe.queried(3);
        probe.replied(3, later(start, 20));
        assert!(probe.rendered(later(start, 21)));
        probe.presented(later(start, 22));
        let key = probe.key().expect("burst");
        assert_eq!(key.request, Some(3));
        assert_eq!(key.presented, Some(later(start, 22)));
        assert!(key.superseded);
        assert_eq!(probe.counts(), (3, 2));
    }

    #[test]
    fn a_frame_without_armed_measurements_defers_nothing() {
        let probe = PresentProbe::default();
        assert!(!probe.rendered(Instant::now()));
    }
}

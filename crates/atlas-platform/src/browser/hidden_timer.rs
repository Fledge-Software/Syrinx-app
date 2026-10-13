//! Decides when a hidden page may be suspended, so a page is never frozen mid-load.

use std::time::{Duration, Instant};

/// How long a hidden page stays awake after its navigation finishes or it is hidden idle.
pub(super) const GRACE: Duration = Duration::from_secs(5);
/// A load that never finishes must not keep a hidden page awake forever.
pub(super) const LOADING_LIMIT: Duration = Duration::from_secs(60);

/// A main-frame navigation WebView2 has reported starting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Loading {
    id: u64,
    started: Instant,
}

/// When the page may next be suspended: never while shown, `GRACE` after it settles while
/// hidden, and no later than `LOADING_LIMIT` after a load that is still running began.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct HiddenTimer {
    visible: bool,
    /// The confirmed navigation in flight, if any.
    loading: Option<Loading>,
    /// When Atlas asked for a navigation that WebView2 has not yet reported starting.
    requested: Option<Instant>,
    /// A suspension was requested or succeeded; only a resume or refusal clears it.
    suspending: bool,
    due: Option<Instant>,
}

impl HiddenTimer {
    /// A new page starts hidden and idle.
    pub(super) fn new(now: Instant) -> Self {
        Self {
            visible: false,
            loading: None,
            requested: None,
            suspending: false,
            due: Some(now + GRACE),
        }
    }

    pub(super) fn visible(&self) -> bool {
        self.visible
    }

    pub(super) fn due(&self) -> Option<Instant> {
        self.due
    }

    pub(super) fn suspending(&self) -> bool {
        self.suspending
    }

    /// The page was shown or hidden.
    pub(super) fn shown(&mut self, visible: bool, now: Instant) {
        self.visible = visible;
        self.rearm(now);
    }

    /// Atlas asked for a navigation; WebView2 reports its id when it starts.
    pub(super) fn navigation_requested(&mut self, now: Instant) {
        self.requested = Some(now);
        self.rearm(now);
    }

    /// Atlas's navigation request failed before WebView2 accepted it; any earlier load stays.
    pub(super) fn navigation_refused(&mut self, now: Instant) {
        self.requested = None;
        self.rearm(now);
    }

    /// WebView2 started a main-frame navigation, including redirects and page-initiated ones.
    pub(super) fn navigation_started(&mut self, id: u64, now: Instant) {
        let requested = self.live_request(now);
        let started = match (requested, self.loading) {
            // Atlas's own request superseded whatever was loading; the new load starts there.
            (Some(requested), _) => requested,
            // A redirect keeps its navigation id, and its first start, so the limit still applies.
            (None, Some(loading)) if loading.id == id => loading.started,
            // A hidden page's own navigation loop stays bounded by the first start.
            (None, Some(loading)) if !self.visible => loading.started,
            // Otherwise it is a new navigation, even if a superseded load has not reported back.
            _ => now,
        };
        self.loading = Some(Loading { id, started });
        self.requested = None;
        self.rearm(now);
    }

    /// A navigation finished, successfully or not. A superseded one's late completion is ignored.
    pub(super) fn navigation_completed(&mut self, id: u64, now: Instant) {
        match self.loading {
            Some(loading) if loading.id == id => self.loading = None,
            Some(_) => return,
            None => {}
        }
        self.rearm(now);
    }

    /// Resuming, or a refused suspension, restarts the countdown, never sooner than `GRACE`.
    pub(super) fn restart(&mut self, now: Instant) {
        self.suspending = false;
        self.rearm(now);
        // A load past its limit would otherwise retry a refused suspension immediately.
        self.due = self.due.map(|due| due.max(now + GRACE));
    }

    /// A suspension request is in progress or done; nothing is due until a resume or refusal.
    pub(super) fn suspension_requested(&mut self) {
        self.suspending = true;
        self.due = None;
    }

    pub(super) fn suspension_due(&self, now: Instant) -> bool {
        !self.visible && self.due.is_some_and(|due| due <= now)
    }

    /// A request WebView2 never started (a same-document jump, say) expires at the limit.
    fn live_request(&self, now: Instant) -> Option<Instant> {
        self.requested
            .filter(|requested| *requested + LOADING_LIMIT > now)
    }

    fn rearm(&mut self, now: Instant) {
        self.requested = self.live_request(now);
        let in_flight = [self.loading.map(|loading| loading.started), self.requested]
            .into_iter()
            .flatten()
            .min();
        self.due = match (self.visible || self.suspending, in_flight) {
            (true, _) => None,
            (false, Some(started)) => Some(started + LOADING_LIMIT),
            (false, None) => Some(now + GRACE),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden_loading(now: Instant, id: u64) -> HiddenTimer {
        let mut timer = HiddenTimer::new(now);
        timer.navigation_requested(now);
        timer.navigation_started(id, now);
        timer
    }

    #[test]
    fn a_hidden_idle_page_is_due_after_the_grace() {
        let start = Instant::now();
        let timer = HiddenTimer::new(start);
        assert!(!timer.suspension_due(start + GRACE - Duration::from_millis(1)));
        assert!(timer.suspension_due(start + GRACE));
    }

    #[test]
    fn a_loading_hidden_page_waits_for_completion_then_the_grace() {
        let start = Instant::now();
        let mut timer = hidden_loading(start, 7);
        // A slow load passes the old five-second point without becoming due.
        assert!(!timer.suspension_due(start + Duration::from_secs(30)));
        let finished = start + Duration::from_secs(30);
        timer.navigation_completed(7, finished);
        assert!(!timer.suspension_due(finished + GRACE - Duration::from_millis(1)));
        assert!(timer.suspension_due(finished + GRACE));
    }

    #[test]
    fn a_failed_navigation_also_settles_the_page() {
        let start = Instant::now();
        let mut timer = hidden_loading(start, 3);
        timer.navigation_completed(3, start + Duration::from_secs(2));
        assert_eq!(timer.due(), Some(start + Duration::from_secs(2) + GRACE));
    }

    #[test]
    fn a_new_navigation_while_hidden_rearms_the_countdown() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        let started = start + Duration::from_secs(4);
        timer.navigation_started(9, started);
        assert!(!timer.suspension_due(start + GRACE));
        timer.navigation_completed(9, started + Duration::from_secs(1));
        assert_eq!(timer.due(), Some(started + Duration::from_secs(1) + GRACE));
    }

    #[test]
    fn a_superseded_navigation_completion_does_not_settle_the_current_one() {
        let start = Instant::now();
        let mut timer = hidden_loading(start, 1);
        timer.navigation_started(2, start + Duration::from_secs(1));
        timer.navigation_completed(1, start + Duration::from_secs(2));
        // A hidden page's own new navigation keeps the first start.
        assert_eq!(timer.due(), Some(start + LOADING_LIMIT));
        timer.navigation_completed(2, start + Duration::from_secs(3));
        assert_eq!(timer.due(), Some(start + Duration::from_secs(3) + GRACE));
    }

    #[test]
    fn a_completion_before_webview2_reports_the_requested_id_is_ignored() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.navigation_requested(start);
        // An earlier navigation's late completion cannot settle the requested one.
        timer.navigation_completed(4, start + Duration::from_secs(1));
        assert_eq!(timer.due(), Some(start + LOADING_LIMIT));
    }

    #[test]
    fn a_load_that_never_finishes_is_due_at_the_loading_limit() {
        let start = Instant::now();
        let mut timer = hidden_loading(start, 5);
        // A redirect keeps its navigation id and the first start time.
        timer.navigation_started(5, start + Duration::from_secs(50));
        assert!(!timer.suspension_due(start + LOADING_LIMIT - Duration::from_millis(1)));
        assert!(timer.suspension_due(start + LOADING_LIMIT));
    }

    #[test]
    fn a_new_navigation_over_a_hung_load_is_dated_from_its_own_start() {
        let start = Instant::now();
        let hung = start + LOADING_LIMIT + Duration::from_secs(10);
        // A shown tab's load hangs past the limit, then Atlas navigates it and hides it.
        let mut requested = HiddenTimer::new(start);
        requested.shown(true, start);
        requested.navigation_started(1, start);
        requested.navigation_requested(hung);
        // The new start can arrive before the superseded load's cancelled completion.
        requested.navigation_started(2, hung + Duration::from_millis(100));
        requested.shown(false, hung + Duration::from_millis(500));
        assert_eq!(requested.due(), Some(hung + LOADING_LIMIT));

        // A page-initiated navigation while shown is new too, and keeps its start once hidden.
        let mut clicked = HiddenTimer::new(start);
        clicked.shown(true, start);
        clicked.navigation_started(1, start);
        clicked.navigation_started(2, hung);
        clicked.shown(false, hung + Duration::from_millis(500));
        assert_eq!(clicked.due(), Some(hung + LOADING_LIMIT));
    }

    #[test]
    fn a_hidden_navigation_loop_is_suspended_within_the_limit_of_its_first_start() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.navigation_started(1, start);
        // Script or meta-refresh keeps starting new navigations that never finish.
        for (step, id) in (2..20).enumerate() {
            let now = start + Duration::from_secs(3 * (step as u64 + 1));
            timer.navigation_started(id, now);
            assert_eq!(timer.due(), Some(start + LOADING_LIMIT));
        }
        assert!(timer.suspension_due(start + LOADING_LIMIT));
    }

    #[test]
    fn a_redirect_after_a_request_keeps_the_request_time() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.navigation_requested(start);
        timer.navigation_started(3, start + Duration::from_secs(2));
        timer.navigation_started(3, start + Duration::from_secs(40));
        assert_eq!(timer.due(), Some(start + LOADING_LIMIT));
    }

    #[test]
    fn an_expired_request_does_not_date_a_later_start() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.shown(true, start);
        timer.navigation_requested(start);
        let later = start + LOADING_LIMIT + Duration::from_secs(5);
        timer.navigation_started(8, later);
        timer.shown(false, later);
        assert_eq!(timer.due(), Some(later + LOADING_LIMIT));
    }

    #[test]
    fn a_shown_page_is_never_due_and_hiding_mid_load_keeps_the_limit() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.shown(true, start);
        timer.navigation_started(8, start);
        assert_eq!(timer.due(), None);
        assert!(!timer.suspension_due(start + LOADING_LIMIT * 2));
        timer.shown(false, start + Duration::from_secs(1));
        assert_eq!(timer.due(), Some(start + LOADING_LIMIT));
        timer.navigation_completed(8, start + Duration::from_secs(2));
        assert_eq!(timer.due(), Some(start + Duration::from_secs(2) + GRACE));
    }

    #[test]
    fn a_refused_request_clears_only_an_unconfirmed_navigation() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.navigation_requested(start);
        timer.navigation_refused(start + Duration::from_secs(1));
        assert_eq!(timer.due(), Some(start + Duration::from_secs(1) + GRACE));
        let mut confirmed = hidden_loading(start, 2);
        confirmed.navigation_refused(start + Duration::from_secs(1));
        assert_eq!(confirmed.due(), Some(start + LOADING_LIMIT));
    }

    #[test]
    fn a_pending_suspension_is_not_due_twice_and_restart_rearms() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.suspension_requested();
        assert!(!timer.suspension_due(start + LOADING_LIMIT));
        timer.restart(start + Duration::from_secs(1));
        assert_eq!(timer.due(), Some(start + Duration::from_secs(1) + GRACE));
        // A refused suspension past the loading limit waits a grace before retrying.
        let mut stuck = hidden_loading(start, 1);
        let late = start + LOADING_LIMIT + Duration::from_secs(1);
        stuck.restart(late);
        assert_eq!(stuck.due(), Some(late + GRACE));
    }

    #[test]
    fn navigation_during_or_after_a_suspension_cannot_arm_a_second_one() {
        let start = Instant::now();
        let mut timer = hidden_loading(start, 1);
        let limit = start + LOADING_LIMIT;
        assert!(timer.suspension_due(limit));
        timer.suspension_requested();
        // Pending: a start and a completion arrive before TrySuspend reports back.
        timer.navigation_started(2, limit);
        timer.navigation_completed(2, limit + Duration::from_secs(1));
        assert_eq!(timer.due(), None);
        // Succeeded: later navigation events still arm nothing until a resume.
        timer.navigation_started(3, limit + Duration::from_secs(2));
        timer.navigation_completed(3, limit + Duration::from_secs(3));
        timer.shown(false, limit + Duration::from_secs(4));
        assert!(!timer.suspension_due(limit + LOADING_LIMIT * 2));
        let resumed = limit + Duration::from_secs(10);
        timer.restart(resumed);
        assert_eq!(timer.due(), Some(resumed + GRACE));
    }

    #[test]
    fn an_unconfirmed_request_neither_dates_a_later_load_nor_outlives_the_limit() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.shown(true, start);
        // A same-document jump: Atlas asked, but WebView2 never reported a start.
        timer.navigation_requested(start);
        let later = start + Duration::from_secs(100);
        timer.navigation_started(4, later);
        timer.shown(false, later);
        assert_eq!(timer.due(), Some(later + LOADING_LIMIT));

        let mut stale = HiddenTimer::new(start);
        stale.shown(true, start);
        stale.navigation_requested(start);
        stale.shown(false, later);
        assert_eq!(stale.due(), Some(later + GRACE));
    }

    #[test]
    fn a_refused_request_keeps_the_load_already_in_flight() {
        let start = Instant::now();
        let mut timer = hidden_loading(start, 2);
        timer.navigation_requested(start + Duration::from_secs(1));
        timer.navigation_refused(start + Duration::from_secs(2));
        assert_eq!(timer.due(), Some(start + LOADING_LIMIT));
        let finished = start + Duration::from_secs(3);
        timer.navigation_completed(2, finished);
        assert_eq!(timer.due(), Some(finished + GRACE));
    }
}

//! Decides when a hidden page may be suspended, so a page is never frozen mid-load or mid-song.

use std::time::{Duration, Instant};

/// How long a hidden page stays awake after its navigation finishes or it is hidden idle.
pub(super) const GRACE: Duration = Duration::from_secs(5);
/// A load that never finishes must not keep a hidden page awake forever.
pub(super) const LOADING_LIMIT: Duration = Duration::from_secs(60);
/// How long a hidden page stays awake after its sound stops, so a pause between tracks or a
/// short buffer never puts a listening tab to sleep (#130).
pub const AUDIO_GRACE: Duration = Duration::from_secs(60);
/// The longest wait between two suspension attempts that WebView2 keeps refusing.
pub(super) const REFUSAL_CAP: Duration = Duration::from_secs(240);

/// A main-frame navigation WebView2 has reported starting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Loading {
    id: u64,
    started: Instant,
}

/// Suspensions WebView2 refused in a row since the page was last shown, navigated or resumed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Refusals {
    count: u32,
    last: Instant,
}

/// When the page may next be suspended: never while shown or playing sound, `GRACE` after it
/// settles while hidden, no later than `LOADING_LIMIT` after a load that is still running
/// began, and never within `AUDIO_GRACE` of its sound stopping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct HiddenTimer {
    visible: bool,
    /// WebView2 reports the document as playing audio, muted or not.
    playing: bool,
    /// When the page's sound last stopped.
    quiet_since: Option<Instant>,
    /// The confirmed navigation in flight, if any.
    loading: Option<Loading>,
    /// When Syrinx asked for a navigation that WebView2 has not yet reported starting.
    requested: Option<Instant>,
    /// A suspension was requested or succeeded; only a resume or refusal clears it.
    suspending: bool,
    /// Refusals in a row; each one doubles the wait before the next attempt.
    refusals: Option<Refusals>,
    due: Option<Instant>,
}

impl HiddenTimer {
    /// A new page starts hidden and idle.
    pub(super) fn new(now: Instant) -> Self {
        Self {
            visible: false,
            playing: false,
            quiet_since: None,
            loading: None,
            requested: None,
            suspending: false,
            refusals: None,
            due: Some(now + GRACE),
        }
    }

    pub(super) fn visible(&self) -> bool {
        self.visible
    }

    /// WebView2 reported that the document started or stopped playing audio. A suspended page
    /// cannot start sound, so no resume on playback is ever needed.
    pub(super) fn playback_changed(&mut self, playing: bool, now: Instant) {
        if playing == self.playing {
            return;
        }
        self.playing = playing;
        // Only a page that was playing earns the audio grace; a seeded "not playing" does not.
        self.quiet_since = (!playing).then_some(now);
        self.rearm(now);
    }

    pub(super) fn due(&self) -> Option<Instant> {
        self.due
    }

    pub(super) fn suspending(&self) -> bool {
        self.suspending
    }

    /// The page was shown or hidden. Showing it forgets earlier refusals.
    pub(super) fn shown(&mut self, visible: bool, now: Instant) {
        self.visible = visible;
        if visible {
            self.refusals = None;
        }
        self.rearm(now);
    }

    /// Syrinx asked for a navigation; WebView2 reports its id when it starts.
    pub(super) fn navigation_requested(&mut self, now: Instant) {
        self.requested = Some(now);
        self.refusals = None;
        self.rearm(now);
    }

    /// Syrinx's navigation request failed before WebView2 accepted it; any earlier load stays.
    pub(super) fn navigation_refused(&mut self, now: Instant) {
        self.requested = None;
        self.rearm(now);
    }

    /// WebView2 started a main-frame navigation, including redirects and page-initiated ones.
    pub(super) fn navigation_started(&mut self, id: u64, now: Instant) {
        let requested = self.live_request(now);
        let started = match (requested, self.loading) {
            // Syrinx's own request superseded whatever was loading; the new load starts there.
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
        self.refusals = None;
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

    /// Resuming restarts the countdown and forgets refusals, never sooner than `GRACE`.
    pub(super) fn resumed(&mut self, now: Instant) {
        self.suspending = false;
        self.refusals = None;
        self.rearm(now);
        // A load past its limit would otherwise be asked again immediately.
        self.due = self.due.map(|due| due.max(now + GRACE));
    }

    /// WebView2 refused or failed a suspension. Each refusal in a row doubles the wait before
    /// the next attempt, up to `REFUSAL_CAP`, so a refusal Syrinx cannot explain (a capture,
    /// say) is not retried every five seconds.
    pub(super) fn suspension_refused(&mut self, now: Instant) {
        self.suspending = false;
        let count = self
            .refusals
            .map_or(1, |refusals| refusals.count.saturating_add(1));
        self.refusals = Some(Refusals { count, last: now });
        self.rearm(now);
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
        let due = match (self.visible || self.suspending || self.playing, in_flight) {
            (true, _) => None,
            (false, Some(started)) => Some(started + LOADING_LIMIT),
            (false, None) => Some(now + GRACE),
        };
        self.due = due.map(|due| self.not_before(due));
    }

    /// The earliest a hidden page may be due: never within `AUDIO_GRACE` of its sound stopping,
    /// and never within the backoff of its last refusal.
    fn not_before(&self, due: Instant) -> Instant {
        let floors = [
            self.quiet_since.map(|quiet| quiet + AUDIO_GRACE),
            self.refusals
                .map(|refusals| refusals.last + refusal_wait(refusals.count)),
        ];
        floors.into_iter().flatten().fold(due, Instant::max)
    }
}

/// The wait after `count` refusals in a row: `GRACE`, doubled each time, capped at `REFUSAL_CAP`.
fn refusal_wait(count: u32) -> Duration {
    2u32.checked_pow(count)
        .and_then(|factor| GRACE.checked_mul(factor))
        .map_or(REFUSAL_CAP, |wait| wait.min(REFUSAL_CAP))
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
        // A shown tab's load hangs past the limit, then Syrinx navigates it and hides it.
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
    fn a_pending_suspension_is_not_due_twice_and_a_resume_rearms() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.suspension_requested();
        assert!(!timer.suspension_due(start + LOADING_LIMIT));
        timer.resumed(start + Duration::from_secs(1));
        assert_eq!(timer.due(), Some(start + Duration::from_secs(1) + GRACE));
        // A refused suspension past the loading limit waits its backoff before retrying.
        let mut stuck = hidden_loading(start, 1);
        let late = start + LOADING_LIMIT + Duration::from_secs(1);
        stuck.suspension_requested();
        stuck.suspension_refused(late);
        assert_eq!(stuck.due(), Some(late + GRACE * 2));
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
        timer.resumed(resumed);
        assert_eq!(timer.due(), Some(resumed + GRACE));
    }

    #[test]
    fn an_unconfirmed_request_neither_dates_a_later_load_nor_outlives_the_limit() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.shown(true, start);
        // A same-document jump: Syrinx asked, but WebView2 never reported a start.
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

    fn hidden_playing(now: Instant) -> HiddenTimer {
        let mut timer = HiddenTimer::new(now);
        timer.playback_changed(true, now);
        timer
    }

    #[test]
    fn a_playing_hidden_page_is_never_due() {
        let start = Instant::now();
        let mut timer = hidden_loading(start, 1);
        timer.playback_changed(true, start);
        assert_eq!(timer.due(), None);
        // Not even a load that hangs past its limit puts a playing page to sleep.
        assert!(!timer.suspension_due(start + LOADING_LIMIT * 2));
        timer.navigation_completed(1, start + Duration::from_secs(1));
        assert_eq!(timer.due(), None);
    }

    #[test]
    fn sound_stopping_arms_the_audio_grace_from_that_moment() {
        let start = Instant::now();
        let mut timer = hidden_playing(start);
        let stopped = start + Duration::from_secs(10);
        timer.playback_changed(false, stopped);
        assert_eq!(timer.due(), Some(stopped + AUDIO_GRACE));
        assert!(!timer.suspension_due(stopped + AUDIO_GRACE - Duration::from_millis(1)));
        assert!(timer.suspension_due(stopped + AUDIO_GRACE));
    }

    #[test]
    fn sound_restarting_within_the_grace_cancels_the_due_time() {
        let start = Instant::now();
        let mut timer = hidden_playing(start);
        timer.playback_changed(false, start + Duration::from_secs(10));
        timer.playback_changed(true, start + Duration::from_secs(40));
        assert_eq!(timer.due(), None);
        let stopped_again = start + Duration::from_secs(50);
        timer.playback_changed(false, stopped_again);
        assert_eq!(timer.due(), Some(stopped_again + AUDIO_GRACE));
    }

    #[test]
    fn showing_and_hiding_a_playing_page_arms_nothing() {
        let start = Instant::now();
        let mut timer = hidden_playing(start);
        timer.shown(true, start + Duration::from_secs(1));
        timer.shown(false, start + Duration::from_secs(2));
        assert_eq!(timer.due(), None);
        assert!(!timer.suspension_due(start + LOADING_LIMIT));
    }

    #[test]
    fn sound_during_a_pending_suspension_waits_for_the_completion() {
        let start = Instant::now();
        let mut timer = hidden_loading(start, 1);
        let limit = start + LOADING_LIMIT;
        assert!(timer.suspension_due(limit));
        timer.suspension_requested();
        // Audio starts before TrySuspend reports back: nothing changes until it does.
        timer.playback_changed(true, limit);
        assert_eq!(timer.due(), None);
        assert!(timer.suspending());
        // The refusal arrives; the page is playing, so still nothing is due.
        timer.suspension_refused(limit + Duration::from_secs(1));
        assert!(!timer.suspending());
        assert_eq!(timer.due(), None);
        let stopped = limit + Duration::from_secs(5);
        timer.playback_changed(false, stopped);
        assert_eq!(timer.due(), Some(stopped + AUDIO_GRACE));
    }

    #[test]
    fn a_muted_page_that_plays_counts_as_playing() {
        // WebView2 reports IsDocumentPlayingAudio as true even while IsMuted is true, and the
        // timer has no mute input: the one playback flag decides, so a muted tab keeps its place.
        let start = Instant::now();
        let timer = hidden_playing(start);
        assert_eq!(timer.due(), None);
        assert!(!timer.suspension_due(start + LOADING_LIMIT * 2));
    }

    #[test]
    fn a_pause_while_shown_still_delays_sleep_after_hiding() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.shown(true, start);
        timer.playback_changed(true, start);
        // The owner pauses the video, then switches tabs two seconds later.
        let paused = start + Duration::from_secs(30);
        timer.playback_changed(false, paused);
        timer.shown(false, paused + Duration::from_secs(2));
        assert_eq!(timer.due(), Some(paused + AUDIO_GRACE));
    }

    #[test]
    fn a_page_that_never_played_keeps_the_ordinary_grace() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        // Creation seeds the runtime's state; a page that never played has no audio grace.
        timer.playback_changed(false, start);
        assert_eq!(timer.due(), Some(start + GRACE));
    }

    #[test]
    fn a_quiet_page_hidden_after_the_audio_grace_uses_the_ordinary_grace() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        timer.shown(true, start);
        timer.playback_changed(true, start);
        let stopped = start + Duration::from_secs(10);
        timer.playback_changed(false, stopped);
        let hidden = stopped + AUDIO_GRACE + Duration::from_secs(5);
        timer.shown(false, hidden);
        assert_eq!(timer.due(), Some(hidden + GRACE));
    }

    #[test]
    fn repeated_refusals_double_the_wait_up_to_the_cap() {
        let start = Instant::now();
        let mut timer = HiddenTimer::new(start);
        let mut refused = start + GRACE;
        // WebView2 keeps refusing (a capture, say): 10, 20, 40, 80, 160 s, then the cap holds.
        for expected in [10, 20, 40, 80, 160, 240, 240] {
            timer.suspension_requested();
            timer.suspension_refused(refused);
            assert_eq!(timer.due(), Some(refused + Duration::from_secs(expected)));
            refused += Duration::from_secs(expected);
        }
    }

    #[test]
    fn refusal_waits_are_capped_even_after_many_refusals() {
        assert_eq!(refusal_wait(0), GRACE);
        assert_eq!(refusal_wait(1), GRACE * 2);
        assert_eq!(refusal_wait(6), REFUSAL_CAP);
        assert_eq!(refusal_wait(40), REFUSAL_CAP);
        assert_eq!(refusal_wait(u32::MAX), REFUSAL_CAP);
    }

    #[test]
    fn a_refusal_backoff_resets_on_show_or_navigation() {
        let start = Instant::now();
        let second = start + Duration::from_secs(10);
        let mut shown = HiddenTimer::new(start);
        shown.suspension_requested();
        shown.suspension_refused(start);
        shown.suspension_requested();
        shown.suspension_refused(second);
        assert_eq!(shown.due(), Some(second + Duration::from_secs(20)));
        shown.shown(true, second + Duration::from_secs(2));
        let hidden = second + Duration::from_secs(3);
        shown.shown(false, hidden);
        assert_eq!(shown.due(), Some(hidden + GRACE));

        let mut navigated = HiddenTimer::new(start);
        navigated.suspension_requested();
        navigated.suspension_refused(start);
        navigated.suspension_requested();
        navigated.suspension_refused(second);
        let started = second + Duration::from_secs(2);
        navigated.navigation_started(9, started);
        assert_eq!(navigated.due(), Some(started + LOADING_LIMIT));
        let finished = started + Duration::from_secs(1);
        navigated.navigation_completed(9, finished);
        assert_eq!(navigated.due(), Some(finished + GRACE));
    }
}

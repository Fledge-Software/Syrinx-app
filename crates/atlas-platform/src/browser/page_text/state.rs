//! What one tab has observed about its current document, kept apart from WebView2 so the
//! scheduling rules can be tested without a window.

use std::time::Instant;

use atlas_core::{
    DocumentFacts, EligibleDocument, Ineligible, ObservedResponse, PageTextPolicy, judge,
    same_document_due,
};

/// The most top-level responses remembered for one navigation.
const RESPONSE_RECORDS: usize = 32;

#[derive(Default)]
pub(super) struct TextState {
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
    /// The last document that committed, kept across navigation starts so that a navigation
    /// the owner did not start knows which document it left.
    committed: Option<String>,
    /// The committed document a navigation left when the owner did not start it.
    unprompted_from: Option<String>,
    loaded: bool,
    /// When a capture falls due, and the generation it was scheduled for.
    due: Option<(Instant, u64)>,
    /// A capture that fell due while the page was suspending waits for it to be shown.
    deferred: bool,
    last_capture: Option<Instant>,
    in_flight: bool,
}

impl TextState {
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    /// The next wake-up; none while a call is in flight, so the worker does not spin.
    pub(super) fn due(&self) -> Option<Instant> {
        self.due
            .filter(|_| !self.in_flight)
            .map(|(instant, _)| instant)
    }

    /// Invalidates anything judged so far; reports whether a scheduled capture was cancelled.
    pub(super) fn advance(&mut self) -> bool {
        self.generation += 1;
        self.deferred = false;
        self.due.take().is_some()
    }

    pub(super) fn navigation_started(&mut self, start: Start) -> bool {
        let cancelled = self.advance();
        if start.redirected && self.navigation == Some(start.navigation) {
            self.redirects.extend(self.requested.take());
        } else {
            self.navigation = Some(start.navigation);
            self.redirects.clear();
            self.responses.clear();
            self.unprompted_from = self.committed.clone().filter(|_| !start.user_initiated);
        }
        let address = start.address;
        self.requested = Some(address);
        self.document = None;
        self.address = None;
        self.loaded = false;
        cancelled
    }

    /// Forgets the navigation when its facts could not be read, so nothing is judged readable.
    pub(super) fn forget_navigation(&mut self) -> bool {
        let cancelled = self.navigation_started(Start {
            address: String::new(),
            redirected: false,
            user_initiated: true,
            navigation: u64::MAX,
        });
        self.requested = None;
        cancelled
    }

    /// Whether a response is the one the current navigation is waiting for.
    pub(super) fn wants_response(&self, address: &str) -> bool {
        self.responses.len() < RESPONSE_RECORDS
            && self
                .requested
                .as_deref()
                .is_some_and(|requested| without_fragment(requested) == without_fragment(address))
    }

    pub(super) fn record_response(&mut self, response: ObservedResponse) {
        if self.wants_response(&response.address) {
            self.responses.push(response);
        }
    }

    pub(super) fn source_changed(
        &mut self,
        source: String,
        new_document: bool,
        policy: &PageTextPolicy,
        now: Instant,
    ) -> bool {
        let cancelled = self.advance();
        if new_document {
            self.committed = Some(source.clone());
            self.document = Some(source.clone());
            self.address = Some(source);
            self.loaded = false;
            return cancelled;
        }
        self.address = Some(source);
        if self.loaded && self.document.is_some() {
            let due = same_document_due(self.last_capture, now, policy.settle_delay());
            self.due = Some((due, self.generation));
        }
        cancelled
    }

    pub(super) fn navigation_completed(
        &mut self,
        navigation: u64,
        success: bool,
        policy: &PageTextPolicy,
        now: Instant,
    ) {
        if self.navigation != Some(navigation) {
            return;
        }
        // A reload or a visit to the same address raises no SourceChanged (F14 found this).
        // The guess is confirmed against the web view's Source before any call, and never
        // replaces `committed`, which only a real commit sets.
        if success && self.document.is_none() {
            self.document = self.requested.clone();
            self.address = self.requested.clone();
        }
        self.loaded = success && self.document.is_some();
        if self.loaded {
            self.due = Some((now + policy.settle_delay(), self.generation));
        }
    }

    pub(super) fn process_failed(&mut self) {
        self.advance();
        self.loaded = false;
    }

    /// Takes a capture that has fallen due, returning the generation it was scheduled for.
    pub(super) fn take_due(&mut self, now: Instant) -> Option<u64> {
        if self.in_flight {
            return None;
        }
        let (instant, generation) = self.due?;
        if instant > now {
            return None;
        }
        self.due = None;
        Some(generation)
    }

    /// The page is suspending or suspended; capture waits until it is shown.
    pub(super) fn defer(&mut self) {
        self.deferred = true;
    }

    pub(super) fn resumed(&mut self, now: Instant) {
        if std::mem::take(&mut self.deferred) {
            self.due = Some((now, self.generation));
        }
    }

    pub(super) fn call_started(&mut self) {
        self.in_flight = true;
    }

    /// Ends the call and returns the generation now, for step 8's check on arrival.
    pub(super) fn call_finished(&mut self, now: Instant) -> u64 {
        self.in_flight = false;
        self.last_capture = Some(now);
        self.generation
    }

    /// Steps 1 to 8: the facts as scheduled, judged against the generation now.
    pub(super) fn judge(
        &self,
        policy: Option<&PageTextPolicy>,
        scheduled: u64,
    ) -> Result<EligibleDocument, Ineligible> {
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
            unprompted_from: self.unprompted_from.as_deref(),
            generation: scheduled,
        };
        judge(policy, facts, self.generation)
    }
}

/// What `NavigationStarting` reported for one hop.
pub(super) struct Start {
    pub(super) address: String,
    pub(super) redirected: bool,
    /// False when a page script or form, not the owner or Atlas, started the navigation.
    pub(super) user_initiated: bool,
    pub(super) navigation: u64,
}

pub(super) fn without_fragment(address: &str) -> &str {
    address.split('#').next().unwrap_or(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const START: &str = "https://plain.example.test/start";
    const PAGE: &str = "https://plain.example.test/page";

    fn policy() -> PageTextPolicy {
        PageTextPolicy::new(&["owner.test"])
            .and_then(|policy| policy.with_banking(&["bank.test"]))
            .expect("synthetic policy")
    }

    fn html(address: &str) -> ObservedResponse {
        ObservedResponse {
            address: address.to_owned(),
            status: 200,
            content_type: Some("text/html".to_owned()),
            cache_control: Vec::new(),
            destination: Some("document".to_owned()),
            method: "GET".to_owned(),
        }
    }

    fn start(address: &str, redirected: bool, navigation: u64) -> Start {
        Start {
            address: address.to_owned(),
            redirected,
            user_initiated: true,
            navigation,
        }
    }

    /// A page that loaded and committed without redirects, under `navigation`.
    fn visit(state: &mut TextState, page: Start, now: Instant) {
        let (address, navigation) = (page.address.clone(), page.navigation);
        state.navigation_started(page);
        state.record_response(html(&address));
        state.source_changed(address, true, &policy(), now);
        state.navigation_completed(navigation, true, &policy(), now);
    }

    fn verdict(state: &mut TextState, now: Instant) -> Result<(), Ineligible> {
        let scheduled = state.take_due(now + Duration::from_secs(3)).expect("due");
        state.judge(Some(&policy()), scheduled).map(|_| ())
    }

    /// A page loaded through one redirect, with its capture scheduled.
    fn loaded(now: Instant) -> TextState {
        let mut state = TextState::default();
        state.navigation_started(start(START, false, 7));
        state.navigation_started(start(PAGE, true, 7));
        state.record_response(html(PAGE));
        state.source_changed(PAGE.into(), true, &policy(), now);
        state.navigation_completed(7, true, &policy(), now);
        state
    }

    #[test]
    fn redirects_accumulate_and_an_excluded_hop_blocks_the_page() {
        let now = Instant::now();
        let mut state = TextState::default();
        state.navigation_started(start("https://bank.test/login", false, 3));
        state.navigation_started(start(PAGE, true, 3));
        state.record_response(html(PAGE));
        state.source_changed(PAGE.into(), true, &policy(), now);
        state.navigation_completed(3, true, &policy(), now);
        let scheduled = state.take_due(now + Duration::from_secs(3)).expect("due");
        assert_eq!(
            state.judge(Some(&policy()), scheduled).map(|_| ()),
            Err(Ineligible::RedirectThroughExcluded)
        );
        // A new navigation starts a fresh chain.
        state.navigation_started(start(PAGE, false, 4));
        assert!(state.redirects.is_empty());
    }

    #[test]
    fn a_completion_without_source_change_uses_the_requested_address() {
        let now = Instant::now();
        let mut state = loaded(now);
        // A reload: start and completion only.
        state.navigation_started(start(PAGE, false, 8));
        state.record_response(html(PAGE));
        state.navigation_completed(8, true, &policy(), now);
        assert_eq!(state.document.as_deref(), Some(PAGE));
        let scheduled = state.take_due(now + Duration::from_secs(3)).expect("due");
        let document = state.judge(Some(&policy()), scheduled).expect("eligible");
        assert_eq!(document.address(), PAGE);
        // A failed completion schedules nothing.
        state.navigation_started(start(PAGE, false, 9));
        state.navigation_completed(9, false, &policy(), now);
        assert_eq!(state.due(), None);
    }

    #[test]
    fn a_stale_completion_is_ignored() {
        let now = Instant::now();
        let mut state = TextState::default();
        state.navigation_started(start(PAGE, false, 1));
        state.navigation_started(start(START, false, 2));
        state.navigation_completed(1, true, &policy(), now);
        assert_eq!(state.due(), None);
        assert_eq!(state.document, None);
    }

    #[test]
    fn a_history_change_schedules_after_the_same_document_interval() {
        let now = Instant::now();
        let mut state = loaded(now);
        let scheduled = state.take_due(now + Duration::from_secs(3)).expect("due");
        assert!(state.judge(Some(&policy()), scheduled).is_ok());
        state.call_started();
        let captured_at = now + Duration::from_secs(3);
        state.call_finished(captured_at);
        state.source_changed(format!("{PAGE}/2"), false, &policy(), captured_at);
        assert_eq!(
            state.due(),
            Some(captured_at + atlas_core::SAME_DOCUMENT_INTERVAL)
        );
    }

    #[test]
    fn no_wake_up_is_reported_while_a_call_is_in_flight() {
        let now = Instant::now();
        let mut state = loaded(now);
        state.take_due(now + Duration::from_secs(3)).expect("due");
        state.call_started();
        // A History API change while in flight schedules a capture it must not spin on.
        state.source_changed(format!("{PAGE}/2"), false, &policy(), now);
        assert_eq!(state.due(), None);
        assert_eq!(state.take_due(now + Duration::from_secs(60)), None);
        state.call_finished(now);
        assert!(state.due().is_some(), "the pending capture is re-armed");
    }

    #[test]
    fn step_eight_compares_the_scheduled_generation_with_now() {
        let now = Instant::now();
        let mut state = loaded(now);
        let scheduled = state.take_due(now + Duration::from_secs(3)).expect("due");
        state.generation += 1;
        assert_eq!(
            state.judge(Some(&policy()), scheduled).map(|_| ()),
            Err(Ineligible::Stale)
        );
    }

    #[test]
    fn a_deferred_capture_runs_when_shown_and_navigation_drops_it() {
        let now = Instant::now();
        let mut state = loaded(now);
        state.take_due(now + Duration::from_secs(3)).expect("due");
        state.defer();
        state.resumed(now);
        assert_eq!(state.due(), Some(now));
        state.take_due(now).expect("due again");
        state.defer();
        state.navigation_started(start(START, false, 11));
        state.resumed(now);
        assert_eq!(state.due(), None);
    }

    #[test]
    fn a_script_navigation_out_of_an_excluded_page_is_a_sign_in_hop() {
        let now = Instant::now();
        let mut state = TextState::default();
        visit(&mut state, start("https://bank.test/login", false, 1), now);
        let scripted = Start {
            user_initiated: false,
            ..start(PAGE, false, 2)
        };
        visit(&mut state, scripted, now);
        assert_eq!(
            verdict(&mut state, now),
            Err(Ineligible::LeftExcludedUnprompted)
        );
        // The same page's own script moving on again leaves a plain page, which is allowed.
        let onward = Start {
            user_initiated: false,
            ..start(START, false, 3)
        };
        visit(&mut state, onward, now);
        assert_eq!(verdict(&mut state, now), Ok(()));
    }

    #[test]
    fn a_user_link_out_of_an_excluded_page_stays_eligible() {
        let now = Instant::now();
        let mut state = TextState::default();
        visit(&mut state, start("https://bank.test/", false, 1), now);
        visit(&mut state, start(PAGE, false, 2), now);
        assert_eq!(verdict(&mut state, now), Ok(()));
    }

    #[test]
    fn a_cancelled_navigation_does_not_move_the_committed_document() {
        let now = Instant::now();
        let mut state = TextState::default();
        visit(&mut state, start("https://bank.test/", false, 1), now);
        // A 204 or a download from the bank: start and completion, no new document.
        state.navigation_started(start(PAGE, false, 2));
        state.navigation_completed(2, true, &policy(), now);
        let scripted = Start {
            user_initiated: false,
            ..start(START, false, 3)
        };
        visit(&mut state, scripted, now);
        assert_eq!(
            verdict(&mut state, now),
            Err(Ineligible::LeftExcludedUnprompted)
        );
    }

    #[test]
    fn a_posted_document_is_a_form_result() {
        let now = Instant::now();
        let mut state = TextState::default();
        state.navigation_started(start(PAGE, false, 1));
        state.record_response(ObservedResponse {
            method: "POST".to_owned(),
            ..html(PAGE)
        });
        state.source_changed(PAGE.into(), true, &policy(), now);
        state.navigation_completed(1, true, &policy(), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::FormPost));
    }

    #[test]
    fn unreadable_navigation_facts_leave_nothing_readable() {
        let now = Instant::now();
        let mut state = loaded(now);
        state.forget_navigation();
        state.navigation_completed(u64::MAX, true, &policy(), now);
        assert_eq!(state.due(), None);
        assert!(!state.wants_response(PAGE));
    }
}

//! What one tab has observed about its current document, kept apart from WebView2 so the
//! scheduling and exit rules can be tested without a window.

use std::time::Instant;

use atlas_core::{
    DocumentFacts, EligibleDocument, Ineligible, LeftDocument, ObservedResponse, PageTextPolicy,
    judge, same_document_address, same_document_due,
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
    /// An address Atlas itself asked this tab to load, until its navigation starts.
    atlas_requested: Option<String>,
    /// The document this navigation left, when Atlas did not start it.
    left: Option<Left>,
    /// The committed document was reached by leaving an excluded or tainted document; kept
    /// until Atlas itself navigates the tab, even while capture is off.
    tainted: bool,
    loaded: bool,
    /// When a capture falls due, and the generation it was scheduled for.
    due: Option<(Instant, u64)>,
    /// A capture that fell due while the page was suspending waits for it to be shown.
    deferred: bool,
    last_capture: Option<Instant>,
    in_flight: bool,
}

/// The exclusion lists to apply, and whether capture is on now. While capture is off the
/// last lists still decide taint, but nothing is scheduled.
#[derive(Clone, Copy)]
pub(super) struct Rules<'a> {
    pub(super) policy: &'a PageTextPolicy,
    pub(super) capture: bool,
}

/// What `NavigationStarting` reported for one hop.
pub(super) struct Start {
    pub(super) address: String,
    pub(super) redirected: bool,
    pub(super) navigation: u64,
    /// The web view's `Source()` as the navigation began: the document being left.
    pub(super) source: String,
}

struct Left {
    address: String,
    tainted: bool,
}

impl TextState {
    /// A popup's state: it starts tainted when its opener's document was excluded or tainted.
    pub(super) fn seeded(tainted: bool) -> Self {
        Self {
            tainted,
            ..Self::default()
        }
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn tainted(&self) -> bool {
        self.tainted
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

    /// Atlas asked the tab to load `address` (address bar, palette, bookmark, tile, Home).
    pub(super) fn atlas_navigation(&mut self, address: &str) {
        self.atlas_requested = Some(address.to_owned());
    }

    /// WebView2 refused Atlas's request before starting it.
    pub(super) fn atlas_navigation_refused(&mut self) {
        self.atlas_requested = None;
    }

    pub(super) fn navigation_started(&mut self, start: Start) -> bool {
        let cancelled = self.advance();
        if start.redirected && self.navigation == Some(start.navigation) {
            self.redirects.extend(self.requested.take());
        } else {
            self.begin_navigation(&start);
        }
        self.requested = Some(start.address);
        self.document = None;
        self.address = None;
        self.loaded = false;
        cancelled
    }

    /// A new navigation: a fresh chain, and the document left unless Atlas started it.
    fn begin_navigation(&mut self, start: &Start) {
        self.navigation = Some(start.navigation);
        self.redirects.clear();
        self.responses.clear();
        let started_by_atlas = self
            .atlas_requested
            .as_deref()
            .is_some_and(|requested| same_document_address(requested, &start.address));
        if started_by_atlas {
            self.atlas_requested = None;
            self.left = None;
            return;
        }
        self.left = Some(Left {
            address: start.source.clone(),
            tainted: self.tainted,
        });
    }

    /// Forgets the navigation when its facts could not be read, so nothing is judged readable.
    /// The taint is kept: an unreadable start never clears it.
    pub(super) fn forget_navigation(&mut self) -> bool {
        let cancelled = self.navigation_started(Start {
            address: String::new(),
            redirected: false,
            navigation: u64::MAX,
            source: String::new(),
        });
        self.requested = None;
        cancelled
    }

    /// Whether a response belongs to the current navigation: its final address or any
    /// earlier hop, since a redirect's response can arrive after the next hop starts.
    pub(super) fn wants_response(&self, address: &str) -> bool {
        let address = without_fragment(address);
        self.responses.len() < RESPONSE_RECORDS
            && self
                .redirects
                .iter()
                .chain(self.requested.as_ref())
                .any(|hop| without_fragment(hop) == address)
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
        rules: Rules<'_>,
        now: Instant,
    ) -> bool {
        let cancelled = self.advance();
        if new_document {
            self.commit(rules.policy);
            self.document = Some(source.clone());
            self.address = Some(source);
            self.loaded = false;
            return cancelled;
        }
        self.address = Some(source);
        if rules.capture && self.loaded && self.document.is_some() {
            let due = same_document_due(self.last_capture, now, rules.policy.settle_delay());
            self.due = Some((due, self.generation));
        }
        cancelled
    }

    /// A new document committed: it inherits the taint of what its navigation left.
    fn commit(&mut self, policy: &PageTextPolicy) {
        let left = self.left.as_ref().map(|left| LeftDocument {
            address: &left.address,
            tainted: left.tainted,
        });
        self.tainted = policy.taints(left, &self.redirects);
    }

    pub(super) fn navigation_completed(
        &mut self,
        navigation: u64,
        success: bool,
        rules: Rules<'_>,
        now: Instant,
    ) {
        if self.navigation != Some(navigation) {
            return;
        }
        // A reload or a visit to the same address raises no SourceChanged (F14 found this).
        // The guess is confirmed against the web view's Source before any call, and never
        // changes the taint, which only a real commit sets.
        if success && self.document.is_none() {
            self.document = self.requested.clone();
            self.address = self.requested.clone();
        }
        self.loaded = success && self.document.is_some();
        if rules.capture && self.loaded {
            self.due = Some((now + rules.policy.settle_delay(), self.generation));
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
            left: self.left.as_ref().map(|left| LeftDocument {
                address: &left.address,
                tainted: left.tainted,
            }),
            generation: scheduled,
        };
        judge(policy, facts, self.generation)
    }
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
    const BANK: &str = "https://bank.test/login";

    fn policy() -> PageTextPolicy {
        PageTextPolicy::new(&["owner.test"])
            .and_then(|policy| policy.with_banking(&["bank.test"]))
            .expect("synthetic policy")
    }

    fn on(policy: &PageTextPolicy) -> Rules<'_> {
        Rules {
            policy,
            capture: true,
        }
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

    /// A hop that leaves a blank tab.
    fn start(address: &str, redirected: bool, navigation: u64) -> Start {
        exit("about:blank", address, navigation, redirected)
    }

    /// A hop that leaves `from`, as the web view's Source reports it.
    fn exit(from: &str, address: &str, navigation: u64, redirected: bool) -> Start {
        Start {
            address: address.to_owned(),
            redirected,
            navigation,
            source: from.to_owned(),
        }
    }

    /// A page that loaded and committed without redirects, under `page.navigation`.
    fn visit(state: &mut TextState, page: Start, now: Instant) {
        let (address, navigation) = (page.address.clone(), page.navigation);
        state.navigation_started(page);
        state.record_response(html(&address));
        state.source_changed(address, true, on(&policy()), now);
        state.navigation_completed(navigation, true, on(&policy()), now);
    }

    /// The same, for a navigation Atlas started.
    fn atlas_visit(
        state: &mut TextState,
        from: &str,
        address: &str,
        navigation: u64,
        now: Instant,
    ) {
        state.atlas_navigation(address);
        visit(state, exit(from, address, navigation, false), now);
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
        state.source_changed(PAGE.into(), true, on(&policy()), now);
        state.navigation_completed(7, true, on(&policy()), now);
        state
    }

    #[test]
    fn redirects_accumulate_and_an_excluded_hop_blocks_the_page() {
        let now = Instant::now();
        let mut state = TextState::default();
        state.navigation_started(start("https://bank.test/login", false, 3));
        state.navigation_started(start(PAGE, true, 3));
        state.record_response(html(PAGE));
        state.source_changed(PAGE.into(), true, on(&policy()), now);
        state.navigation_completed(3, true, on(&policy()), now);
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
        state.navigation_completed(8, true, on(&policy()), now);
        assert_eq!(state.document.as_deref(), Some(PAGE));
        let scheduled = state.take_due(now + Duration::from_secs(3)).expect("due");
        let document = state.judge(Some(&policy()), scheduled).expect("eligible");
        assert_eq!(document.address(), PAGE);
        // A failed completion schedules nothing.
        state.navigation_started(start(PAGE, false, 9));
        state.navigation_completed(9, false, on(&policy()), now);
        assert_eq!(state.due(), None);
    }

    #[test]
    fn a_stale_completion_is_ignored() {
        let now = Instant::now();
        let mut state = TextState::default();
        state.navigation_started(start(PAGE, false, 1));
        state.navigation_started(start(START, false, 2));
        state.navigation_completed(1, true, on(&policy()), now);
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
        state.source_changed(format!("{PAGE}/2"), false, on(&policy()), captured_at);
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
        state.source_changed(format!("{PAGE}/2"), false, on(&policy()), now);
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
    fn a_script_navigation_out_of_an_excluded_page_is_skipped_and_the_taint_carries_on() {
        let now = Instant::now();
        let mut state = TextState::default();
        atlas_visit(&mut state, "about:blank", BANK, 1, now);
        visit(&mut state, exit(BANK, PAGE, 2, false), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::LeftExcluded));
        // The callback page's own script moves on: still tainted, still skipped.
        visit(&mut state, exit(PAGE, START, 3, false), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::LeftExcluded));
        // Only a navigation Atlas starts clears the taint.
        atlas_visit(&mut state, START, PAGE, 4, now);
        assert_eq!(verdict(&mut state, now), Ok(()));
        visit(&mut state, exit(PAGE, START, 5, false), now);
        assert_eq!(verdict(&mut state, now), Ok(()));
    }

    #[test]
    fn a_link_out_of_an_excluded_page_is_skipped() {
        let now = Instant::now();
        let mut state = TextState::default();
        atlas_visit(&mut state, "about:blank", BANK, 1, now);
        // A click is not an Atlas navigation, however recently the owner touched the page.
        visit(&mut state, exit(BANK, PAGE, 2, false), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::LeftExcluded));
    }

    #[test]
    fn atlas_navigation_is_recognised_only_for_the_address_it_asked_for() {
        let now = Instant::now();
        let mut state = TextState::default();
        atlas_visit(&mut state, "about:blank", BANK, 1, now);
        // A page script's navigation that starts before Atlas's own does not consume it.
        state.atlas_navigation(PAGE);
        visit(&mut state, exit(BANK, START, 2, false), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::LeftExcluded));
        visit(
            &mut state,
            exit(START, &format!("{PAGE}#top"), 3, false),
            now,
        );
        assert_eq!(verdict(&mut state, now), Ok(()));
    }

    #[test]
    fn a_redirect_through_an_excluded_site_taints_the_next_exit() {
        let now = Instant::now();
        let mut state = TextState::default();
        state.atlas_navigation(START);
        state.navigation_started(start(START, false, 1));
        state.navigation_started(start("https://accounts.google.com/o", true, 1));
        state.navigation_started(start(PAGE, true, 1));
        state.record_response(html(PAGE));
        state.source_changed(PAGE.into(), true, on(&policy()), now);
        assert!(state.tainted());
        visit(&mut state, exit(PAGE, START, 2, false), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::LeftExcluded));
    }

    #[test]
    fn a_cancelled_navigation_leaves_the_source_as_the_document_left() {
        let now = Instant::now();
        let mut state = TextState::default();
        atlas_visit(&mut state, "about:blank", BANK, 1, now);
        // A 204 or a download from the bank: start and completion, no new document.
        state.navigation_started(exit(BANK, PAGE, 2, false));
        state.navigation_completed(2, true, on(&policy()), now);
        visit(&mut state, exit(BANK, START, 3, false), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::LeftExcluded));
    }

    #[test]
    fn taint_is_tracked_while_capture_is_off_and_nothing_is_scheduled() {
        let now = Instant::now();
        let mut state = TextState::default();
        let policy = policy();
        let off = Rules {
            policy: &policy,
            capture: false,
        };
        state.navigation_started(exit(BANK, PAGE, 1, false));
        state.source_changed(PAGE.into(), true, off, now);
        state.navigation_completed(1, true, off, now);
        assert!(state.tainted());
        assert_eq!(state.due(), None);
    }

    #[test]
    fn a_popup_from_an_excluded_or_tainted_opener_starts_tainted() {
        let now = Instant::now();
        let mut state = TextState::seeded(true);
        visit(&mut state, start(PAGE, false, 1), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::LeftExcluded));
        let mut clean = TextState::seeded(false);
        visit(&mut clean, start(PAGE, false, 1), now);
        assert_eq!(verdict(&mut clean, now), Ok(()));
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
        state.source_changed(PAGE.into(), true, on(&policy()), now);
        state.navigation_completed(1, true, on(&policy()), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::FormPost));
    }

    #[test]
    fn a_post_redirect_response_arriving_after_the_next_hop_still_counts() {
        let now = Instant::now();
        let mut state = TextState::default();
        let submit = "https://plain.example.test/submit";
        state.navigation_started(start(submit, false, 1));
        state.navigation_started(start(PAGE, true, 1));
        // The 303's response event arrives after the redirect's NavigationStarting.
        state.record_response(ObservedResponse {
            method: "POST".to_owned(),
            status: 303,
            ..html(submit)
        });
        state.record_response(html(PAGE));
        state.source_changed(PAGE.into(), true, on(&policy()), now);
        state.navigation_completed(1, true, on(&policy()), now);
        assert_eq!(verdict(&mut state, now), Err(Ineligible::FormPost));
    }

    #[test]
    fn unreadable_navigation_facts_leave_nothing_readable() {
        let now = Instant::now();
        let mut state = loaded(now);
        state.forget_navigation();
        state.navigation_completed(u64::MAX, true, on(&policy()), now);
        assert_eq!(state.due(), None);
        assert!(!state.wants_response(PAGE));
    }
}

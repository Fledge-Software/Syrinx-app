//! Tracks the palette's rows, selection and request identity so stale replies never replace newer ones.

use syrinx_core::{AddressPrefix, address_prefix};
use syrinx_search::{SearchHit, SearchResults, Target};

use super::{
    PaletteEvent,
    commands::CommandMatches,
    query::{ChipCounts, CommandCounts},
};
use crate::{navigation, session::SessionError};

#[derive(Default)]
pub(super) struct PaletteChoices {
    pub(super) request_id: u64,
    pub(super) address: Option<String>,
    /// Plain text in the Everything view leads with a "Search Google" row (#127).
    pub(super) web_search: bool,
    /// The trimmed text of the current request, for the search row and exact matches.
    pub(super) entry: String,
    /// Strong command rows, the worker's rows, then other commands; only the current request
    /// changes them.
    pub(super) results: Vec<SearchHit>,
    pub(super) selected: Option<usize>,
    refresh_selection: Option<RefreshSelection>,
    pub(super) status: String,
    /// Locally matched commands for the current request; the first `staged_strong` lead pages.
    staged: Vec<SearchHit>,
    staged_strong: usize,
    staged_counts: CommandCounts,
    typed: bool,
    /// A `>`, `@` or `/` query never falls back to navigation or web search.
    prefixed: bool,
    /// The owner moved the selection, so a reply keeps it on the same destination.
    moved: bool,
    /// The current request has not been answered, so its rows are still unknown.
    awaiting_reply: bool,
    /// Enter pressed while waiting, with the text it submits once the reply arrives.
    deferred: Option<String>,
    pub(super) counts: ChipCounts,
}

#[derive(Clone, Copy)]
enum RefreshSelection {
    Default,
    /// The owner chose the leading search row.
    Lead,
    Target(Target),
    Unselected,
}

impl PaletteChoices {
    /// Starts a new palette entry: no rows, selection, status or waiting Enter. Request IDs
    /// keep counting, so a reply to the previous entry can never match.
    pub(super) fn reset(&mut self) {
        *self = Self {
            request_id: self.request_id,
            ..Self::default()
        };
    }

    fn begin_query(&mut self) -> u64 {
        self.request_id = self
            .request_id
            .checked_add(1)
            .expect("palette request IDs exhausted");
        self.status = "Searching locally…".to_owned();
        self.results.clear();
        self.awaiting_reply = true;
        self.request_id
    }

    #[cfg(test)]
    fn query(&mut self, text: &str) -> u64 {
        self.query_scoped(text, true)
    }

    /// Starts a request; only the Everything view can offer the Go to address row.
    pub(super) fn query_scoped(&mut self, text: &str, accepts_address: bool) -> u64 {
        self.begin_query();
        // Refused input has neither lead row; submission still uses the existing validator.
        let recognized = navigation::recognized_address(text);
        self.entry = text.trim().to_owned();
        self.web_search = accepts_address && matches!(recognized, Ok(None));
        self.address = match recognized {
            Ok(address) if accepts_address => address,
            Ok(_) => None,
            Err(error) => {
                if accepts_address {
                    self.status = error.to_owned();
                }
                None
            }
        };
        self.selected = Some(0);
        self.refresh_selection = None;
        self.moved = false;
        // Changed text replaces whatever an earlier Enter was waiting to submit.
        self.deferred = None;
        self.request_id
    }

    fn selection(&self) -> RefreshSelection {
        if let Some(hit) = self.selected_hit() {
            return RefreshSelection::Target(hit.target);
        }
        if self.moved && self.selected == Some(0) && self.web_search {
            return RefreshSelection::Lead;
        }
        if self.selected.is_some() {
            RefreshSelection::Default
        } else {
            RefreshSelection::Unselected
        }
    }

    /// Records this request's command rows. Commands are local, so rows that will lead the list
    /// show at once (all of them after a prefix); the rest wait for the worker's reply.
    pub(super) fn stage_commands(&mut self, matches: CommandMatches, typed: bool, prefixed: bool) {
        self.staged_strong = matches.strong.min(matches.hits.len());
        self.staged = matches.hits;
        self.staged_counts = matches.counts;
        self.typed = typed;
        self.prefixed = prefixed;
        let shown = if prefixed {
            self.staged.len()
        } else {
            self.staged_strong
        };
        self.results = self.staged[..shown].to_vec();
        // A strong command shown at once is Enter's choice before the reply arrives.
        if self.refresh_selection.is_none() && !self.moved {
            self.selected = Some(self.default_row());
        }
    }

    pub(super) fn refresh(&mut self) -> u64 {
        // Repeated model changes retain intent even after the visible rows were invalidated.
        let selection = self.refresh_selection.unwrap_or_else(|| self.selection());
        self.begin_query();
        self.selected = match selection {
            RefreshSelection::Lead => Some(0),
            RefreshSelection::Default if self.address.is_some() => Some(0),
            _ => None,
        };
        self.refresh_selection = Some(selection);
        self.request_id
    }

    pub(super) fn receive(
        &mut self,
        request_id: u64,
        result: Result<SearchResults, SessionError>,
    ) -> bool {
        if request_id != self.request_id {
            return false;
        }
        let selected_target = self.selected_hit().map(|hit| hit.target);
        let refresh_selection = self.refresh_selection.take();
        self.awaiting_reply = false;
        let succeeded = result.is_ok();
        match result {
            Ok(results) => {
                self.status = format!(
                    "{} tabs · {} saved pages",
                    results.counts.tabs, results.counts.saved
                );
                self.counts = ChipCounts::new(results.counts, self.staged_counts, self.typed);
                let (strong, weak) = self.staged.split_at(self.staged_strong);
                self.results = strong
                    .iter()
                    .cloned()
                    .chain(results.hits)
                    .chain(weak.iter().cloned())
                    .collect();
            }
            Err(error) => {
                // A worker failure leaves the locally matched commands usable.
                self.status = error.to_string();
                self.results = self.staged.clone();
            }
        }
        // A removed local selection cannot silently activate the row that inherited its index.
        self.selected = match refresh_selection {
            Some(RefreshSelection::Target(target)) => self.target_row(target),
            Some(RefreshSelection::Unselected) => None,
            Some(RefreshSelection::Lead) => Some(0),
            Some(RefreshSelection::Default) if !succeeded && self.lead_rows() == 0 => None,
            Some(RefreshSelection::Default) => Some(self.default_row()),
            None if self.moved => selected_target
                .and_then(|target| self.target_row(target))
                .or(Some(0)),
            None => Some(self.default_row()),
        };
        true
    }

    /// The row Enter takes while the owner has not moved the selection (#127): a confident
    /// local match if there is one, otherwise the leading Go to or search row, or the top row.
    fn default_row(&self) -> usize {
        self.results
            .iter()
            .enumerate()
            .position(|(index, hit)| self.is_confident(index, hit))
            .map_or(0, |index| index + self.lead_rows())
    }

    /// Whether a row matches the typed text letter for letter, so Enter may open it instead of
    /// searching: a strong command, the start of its address, or an open page's exact title.
    fn is_confident(&self, index: usize, hit: &SearchHit) -> bool {
        if self.web_search {
            return index < self.staged_strong
                || address_prefix(&self.entry, &hit.address) != AddressPrefix::No
                || has_exact_title(hit, &self.entry);
        }
        // Typed text that only starts a known host, such as `cloud.exam`, beats Go to.
        self.address.is_some()
            && address_prefix(&self.entry, &hit.address) == AddressPrefix::PartialHost
    }

    fn target_row(&self, target: Target) -> Option<usize> {
        self.results
            .iter()
            .position(|hit| hit.target == target)
            .map(|index| index + self.lead_rows())
            .or_else(|| self.address.as_ref().map(|_| 0))
    }

    /// The Go to or search row drawn above the results, if any.
    pub(super) fn lead_rows(&self) -> usize {
        usize::from(self.address.is_some() || self.web_search)
    }

    /// How many selectable rows are drawn: the lead row, if any, then the results.
    pub(super) fn row_count(&self) -> usize {
        self.results.len() + self.lead_rows()
    }

    /// The row a screen reader hears as focused (DW-60): the selection, but only once the
    /// owner has moved it, so the query field keeps the focus while the owner types.
    pub(super) fn announced_row(&self) -> Option<usize> {
        self.selected.filter(|_| self.moved)
    }

    pub(super) fn selected_hit(&self) -> Option<&SearchHit> {
        self.selected
            .and_then(|selected| selected.checked_sub(self.lead_rows()))
            .and_then(|index| self.results.get(index))
    }

    pub(super) fn select(&mut self, selected: usize) {
        self.selected = Some(selected);
        self.moved = true;
        // An explicit choice while waiting supersedes the retained pre-refresh selection.
        if self.refresh_selection.is_some() {
            self.refresh_selection = Some(self.selection());
        }
    }

    pub(super) fn next(&mut self) {
        let rows = self.row_count();
        if rows == 0 {
            return;
        }
        self.select(
            self.selected
                .map_or(0, |selected| selected.saturating_add(1).min(rows - 1)),
        );
    }

    pub(super) fn previous(&mut self) {
        if self.results.is_empty() && self.lead_rows() == 0 {
            return;
        }
        self.select(
            self.selected
                .map_or(0, |selected| selected.saturating_sub(1)),
        );
    }

    pub(super) fn activation(&self, request_id: u64, target: Target) -> Option<PaletteEvent> {
        if request_id != self.request_id || self.refresh_selection.is_some() {
            return None;
        }
        self.results
            .iter()
            .any(|hit| hit.target == target)
            .then_some(PaletteEvent::Activate(target))
    }

    /// What Enter does now. While the current request is unanswered and no visible row or
    /// Go to is selected, Enter waits for the reply instead of searching the web.
    pub(super) fn submission(&mut self, text: String, force_search: bool) -> Option<PaletteEvent> {
        if self.must_wait_for_rows(force_search) {
            self.status = "Waiting for local results…".to_owned();
            self.deferred = Some(text);
            return None;
        }
        self.decide(text, force_search)
    }

    /// Whether the current request's reply has arrived.
    pub(super) fn answered(&self) -> bool {
        !self.awaiting_reply
    }

    /// The submission an Enter pressed while waiting makes, once its reply has arrived.
    pub(super) fn take_deferred(&mut self) -> Option<PaletteEvent> {
        if self.awaiting_reply {
            return None;
        }
        let text = self.deferred.take()?;
        self.decide(text, false)
    }

    /// Enter waits when the rows it would choose from have not arrived and nothing is chosen.
    /// Go to is known at once; the search row counts only once the owner picked it, since a
    /// confident match may still arrive.
    fn must_wait_for_rows(&self, force_search: bool) -> bool {
        let lead_chosen =
            self.selected == Some(0) && (self.address.is_some() || (self.web_search && self.moved));
        !force_search && self.awaiting_reply && self.selected_hit().is_none() && !lead_chosen
    }

    fn decide(&self, text: String, force_search: bool) -> Option<PaletteEvent> {
        // Enter on a prefixed query only runs or opens a listed row; Alt Enter stays explicit.
        if self.prefixed && !force_search {
            return self
                .selected_hit()
                .map(|hit| PaletteEvent::Activate(hit.target));
        }
        if !force_search {
            self.selected?;
            if let Some(hit) = self.selected_hit() {
                return Some(PaletteEvent::Activate(hit.target));
            }
            if self.selected == Some(0)
                && let Some(address) = &self.address
            {
                return Some(PaletteEvent::Submit {
                    text: address.clone(),
                    force_search: false,
                });
            }
        }
        Some(PaletteEvent::Submit { text, force_search })
    }
}

/// An open tab or saved page whose title is exactly the typed text, ignoring case.
fn has_exact_title(hit: &SearchHit, entry: &str) -> bool {
    matches!(hit.target, Target::Tab(_) | Target::Saved(_))
        && hit.title.trim().to_lowercase() == entry.to_lowercase()
}

#[cfg(test)]
mod tests {
    use syrinx_core::{SavedPageId, TabId};
    use syrinx_search::{ScopeCounts, SearchError};

    use super::*;

    const ADDRESS: &str = "https://palette.example.test/Case?Key=Value";

    fn targets() -> [Target; 2] {
        [
            Target::Tab(TabId::new(1).expect("synthetic tab identity")),
            Target::Saved(SavedPageId::new(1).expect("synthetic saved identity")),
        ]
    }

    fn results(targets: &[Target]) -> SearchResults {
        let tabs = targets
            .iter()
            .filter(|target| matches!(target, Target::Tab(_)))
            .count();
        let saved = targets
            .iter()
            .filter(|target| matches!(target, Target::Saved(_)))
            .count();
        SearchResults {
            generation: 1,
            counts: ScopeCounts {
                tabs,
                saved,
                ..ScopeCounts::default()
            },
            hits: targets
                .iter()
                .map(|target| SearchHit {
                    target: *target,
                    score: 1,
                    title: "Synthetic palette match".to_owned(),
                    address: ADDRESS.to_owned(),
                    title_matches: Vec::new(),
                    address_matches: Vec::new(),
                })
                .collect(),
        }
    }

    /// One synthetic hit per `(target, title, address)`, in ranking order.
    fn ranked(rows: &[(Target, &str, &str)]) -> SearchResults {
        let targets: Vec<_> = rows.iter().map(|(target, _, _)| *target).collect();
        let mut results = results(&targets);
        for (hit, (_, title, address)) in results.hits.iter_mut().zip(rows) {
            hit.title = (*title).to_owned();
            hit.address = (*address).to_owned();
        }
        results
    }

    fn history(key: u64) -> Target {
        Target::History(std::num::NonZeroU64::new(key).expect("synthetic history key"))
    }

    /// Enter submits the typed text, which `navigation` turns into a Google search.
    fn assert_search(choices: &mut PaletteChoices, input: &str) {
        assert_submit(choices, input, false, input);
    }

    fn assert_submit(
        choices: &mut PaletteChoices,
        input: &str,
        force_search: bool,
        expected: &str,
    ) {
        match choices.submission(input.to_owned(), force_search) {
            Some(PaletteEvent::Submit {
                text,
                force_search: actual_force,
            }) => {
                assert_eq!(text, expected);
                assert_eq!(actual_force, force_search);
            }
            _ => panic!("expected explicit address or search submission"),
        }
    }

    fn assert_target(choices: &mut PaletteChoices, expected: Target) {
        assert!(matches!(
            choices.submission(ADDRESS.to_owned(), false),
            Some(PaletteEvent::Activate(target)) if target == expected
        ));
    }

    #[test]
    fn literal_enter_stays_on_go_to_across_local_results_empty_and_failure() {
        let mut choices = PaletteChoices::default();
        let request = choices.query(ADDRESS);
        assert_eq!(choices.address.as_deref(), Some(ADDRESS));
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
        choices.next();
        assert_eq!(
            choices.selected,
            Some(0),
            "Down cannot select an unavailable local row"
        );
        assert!(choices.receive(request, Ok(results(&targets()))));
        assert_eq!(choices.selected, Some(0));
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
        assert!(choices.receive(request, Ok(results(&[]))));
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
        assert!(choices.receive(
            request,
            Err(SessionError::Search(SearchError::QueryTooLong))
        ));
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn deliberate_local_selection_tracks_target_and_arrows_return_to_go_to() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let request = choices.query(ADDRESS);
        assert!(choices.receive(request, Ok(results(&[tab, saved]))));
        choices.next();
        assert_eq!(choices.selected, Some(1));
        assert_target(&mut choices, tab);
        choices.previous();
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
        choices.previous();
        assert_eq!(choices.selected, Some(0));
        choices.next();
        choices.next();
        choices.next();
        assert_eq!(choices.selected, Some(2));
        assert_target(&mut choices, saved);

        // An updated ranking must not quietly change the chosen destination.
        assert!(choices.receive(request, Ok(results(&[saved, tab]))));
        assert_eq!(choices.selected, Some(1));
        assert_target(&mut choices, saved);
        assert!(choices.receive(request, Ok(results(&[tab]))));
        assert_eq!(choices.selected, Some(0));
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn input_changes_reject_stale_replies_and_reopening_resets_selection() {
        let mut choices = PaletteChoices::default();
        let old = choices.query(ADDRESS);
        assert!(choices.receive(old, Ok(results(&targets()))));
        choices.next();
        assert_target(&mut choices, targets()[0]);
        let next_address = "https://next-palette.example.test/";
        let next = choices.query(next_address);
        assert_ne!(next, old);
        assert_eq!(choices.selected, Some(0));
        let status = choices.status.clone();
        assert!(!choices.receive(old, Ok(results(&targets()))));
        assert!(!choices.receive(old, Err(SessionError::Search(SearchError::QueryTooLong))));
        assert_eq!(choices.status, status);
        assert!(choices.results.is_empty());
        assert_submit(&mut choices, next_address, false, next_address);
        assert!(choices.receive(next, Ok(results(&targets()))));
        assert_submit(&mut choices, next_address, false, next_address);

        // The owning UI correlates popup identities; a new popup starts fresh local state.
        let mut reopened = PaletteChoices::default();
        assert_eq!(reopened.query(ADDRESS), old);
        assert!(reopened.results.is_empty());
        assert_eq!(reopened.selected, Some(0));
        assert_submit(&mut reopened, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn refused_and_empty_input_keep_existing_submission_behavior() {
        for input in [
            "",
            "JAVASCRIPT:1",
            "https://synthetic@example.test/",
            "https://[invalid]",
        ] {
            let mut choices = PaletteChoices::default();
            let request = choices.query(input);
            assert_eq!(choices.address, None);
            if let Err(error) = navigation::recognized_address(input) {
                assert_eq!(choices.status, error);
            }
            assert!(choices.submission(input.to_owned(), false).is_none());
            assert!(choices.receive(request, Ok(results(&targets()))));
            assert!(matches!(
                choices.take_deferred(),
                Some(PaletteEvent::Activate(target)) if target == targets()[0]
            ));
            assert_target(&mut choices, targets()[0]);
            choices.next();
            assert_target(&mut choices, targets()[1]);
            choices.previous();
            assert_target(&mut choices, targets()[0]);
            assert!(choices.receive(
                request,
                Err(SessionError::Search(SearchError::QueryTooLong))
            ));
            assert_submit(&mut choices, input, false, input);
        }
    }

    #[test]
    fn plain_text_leads_with_a_web_search_and_keeps_pages_as_choices() {
        let [tab, saved] = targets();
        let input = "ordinary search terms";
        let mut choices = PaletteChoices::default();
        let request = choices.query(input);
        assert!(choices.web_search);
        assert_eq!(choices.entry, input);
        assert_eq!(choices.address, None);
        // Enter before the reply waits, since an exact match could still arrive.
        assert!(choices.submission(input.to_owned(), false).is_none());
        assert!(choices.receive(request, Ok(results(&[tab, saved]))));
        assert!(matches!(
            choices.take_deferred(),
            Some(PaletteEvent::Submit { text, force_search: false }) if text == input
        ));
        assert_eq!(choices.selected, Some(0));
        assert_eq!(choices.row_count(), 3);
        assert_search(&mut choices, input);
        choices.next();
        assert_target(&mut choices, tab);
        choices.next();
        assert_target(&mut choices, saved);
        choices.previous();
        choices.previous();
        assert_search(&mut choices, input);
    }

    #[test]
    fn a_frequently_visited_fuzzy_match_never_becomes_enters_target() {
        let wrong = history(7);
        let input = "cloudflare";
        let mut choices = PaletteChoices::default();
        let request = choices.query(input);
        let mut rows = ranked(&[(
            wrong,
            "Cloud database login",
            "https://www.database.example.test/",
        )]);
        rows.hits[0].score = u32::MAX;
        assert!(choices.receive(request, Ok(rows)));
        assert_eq!(
            choices.selected,
            Some(0),
            "the search row stays Enter's target"
        );
        assert_search(&mut choices, input);
        choices.next();
        assert_target(&mut choices, wrong);
    }

    #[test]
    fn the_start_of_a_known_address_is_preselected_below_the_lead_row() {
        let [tab, saved] = targets();
        // `www.cloudf` is an address, so it leads with Go to; its partial host still wins.
        for input in ["cloudf", "CLOUDF", "www.cloudf"] {
            let mut choices = PaletteChoices::default();
            let request = choices.query(input);
            assert_eq!(choices.lead_rows(), 1);
            assert!(choices.receive(
                request,
                Ok(ranked(&[
                    (
                        tab,
                        "Synthetic fuzzy match",
                        "https://mycloudflare.example/"
                    ),
                    (
                        saved,
                        "Synthetic docs",
                        "https://www.cloudflare.example/docs"
                    ),
                ]))
            ));
            assert_eq!(choices.selected, Some(2), "{input}");
            assert_target(&mut choices, saved);
        }
    }

    #[test]
    fn a_partial_host_beats_go_to_but_a_complete_host_does_not() {
        let [tab, _] = targets();
        let known = "https://cloud.example.test/projects/flare-console";
        let mut choices = PaletteChoices::default();
        let request = choices.query("cloud.example");
        assert_eq!(choices.address.as_deref(), Some("https://cloud.example/"));
        assert!(choices.receive(request, Ok(ranked(&[(tab, "Synthetic console", known)]))));
        assert_eq!(choices.selected, Some(1));
        assert_target(&mut choices, tab);

        let request = choices.query("cloud.example.test");
        assert!(choices.receive(request, Ok(ranked(&[(tab, "Synthetic console", known)]))));
        assert_eq!(
            choices.selected,
            Some(0),
            "Go to the typed host stays first"
        );
        assert_submit(
            &mut choices,
            "cloud.example.test",
            false,
            "https://cloud.example.test/",
        );
    }

    #[test]
    fn an_open_pages_exact_title_switches_to_it_but_history_does_not() {
        let [tab, saved] = targets();
        let title = "Synthetic Palette Match";
        for (target, opens) in [(tab, true), (saved, true), (history(3), false)] {
            let input = "synthetic palette match";
            let mut choices = PaletteChoices::default();
            let request = choices.query(input);
            assert!(choices.receive(
                request,
                Ok(ranked(&[(target, title, "https://titles.example.test/")]))
            ));
            if opens {
                assert_target(&mut choices, target);
            } else {
                assert_search(&mut choices, input);
            }
        }
    }

    #[test]
    fn a_chosen_search_row_is_used_at_once_and_survives_a_refresh() {
        let [tab, saved] = targets();
        let input = "cloudf";
        let mut choices = PaletteChoices::default();
        choices.query(input);
        // Clicking the search row needs no local rows, so Enter does not wait.
        choices.select(0);
        assert_search(&mut choices, input);

        let request = choices.query(input);
        let rows = || {
            ranked(&[
                (tab, "Synthetic fuzzy match", "https://fuzzy.example.test/"),
                (saved, "Synthetic docs", "https://cloudflare.example/"),
            ])
        };
        assert!(choices.receive(request, Ok(rows())));
        assert_eq!(choices.selected, Some(2));
        choices.select(0);
        let refreshed = choices.refresh();
        assert_eq!(choices.selected, Some(0));
        assert!(choices.receive(refreshed, Ok(rows())));
        assert_eq!(
            choices.selected,
            Some(0),
            "the owner's choice outlives the refresh"
        );
        assert_search(&mut choices, input);
    }

    #[test]
    fn go_to_uses_canonical_address_but_forced_search_preserves_input() {
        let input = "  HTTPS://palette.example.test/Case?Key=Value  ";
        let mut choices = PaletteChoices::default();
        let request = choices.query(input);
        assert_eq!(choices.address.as_deref(), Some(ADDRESS));
        assert_submit(&mut choices, input, false, ADDRESS);
        assert_submit(&mut choices, input, true, input);
        assert!(choices.receive(request, Ok(results(&targets()))));
        choices.next();
        assert_target(&mut choices, targets()[0]);
        assert_submit(&mut choices, input, true, input);

        // Clicking Go to explicitly selects its row, regardless of the previous local choice.
        choices.select(0);
        assert_submit(&mut choices, input, false, ADDRESS);
    }

    #[test]
    fn model_refresh_replaces_counts_and_rows_without_an_input_change() {
        let [first, _] = targets();
        let second = Target::Tab(TabId::new(2).expect("second synthetic Home"));
        let mut choices = PaletteChoices::default();
        let old = choices.query("");
        assert!(choices.receive(old, Ok(results(&[first, second]))));
        assert_eq!(choices.status, "2 tabs · 0 saved pages");

        let refreshed = choices.refresh();
        assert!(choices.results.is_empty());
        assert_eq!(choices.status, "Searching locally…");
        assert!(choices.submission(String::new(), false).is_none());
        assert!(choices.activation(old, second).is_none());
        assert!(!choices.receive(old, Ok(results(&[first, second]))));
        assert!(choices.receive(refreshed, Ok(results(&[first]))));
        assert_eq!(choices.status, "1 tabs · 0 saved pages");
        assert_eq!(choices.results.len(), 1);
        assert_target(&mut choices, first);
        assert!(choices.activation(refreshed, second).is_none());
    }

    #[test]
    fn repeated_refreshes_preserve_target_identity_and_reject_earlier_replies() {
        let [tab, saved] = targets();
        for input in [ADDRESS, "synthetic"] {
            let mut choices = PaletteChoices::default();
            let initial = choices.query(input);
            assert!(choices.receive(initial, Ok(results(&[tab, saved]))));
            choices.select(choices.lead_rows() + 1);
            assert_target(&mut choices, saved);

            let first_refresh = choices.refresh();
            let latest_refresh = choices.refresh();
            assert!(choices.results.is_empty());
            assert!(choices.submission(input.to_owned(), false).is_none());
            assert!(choices.activation(initial, saved).is_none());
            assert!(!choices.receive(first_refresh, Ok(results(&[tab]))));
            assert!(!choices.receive(
                initial,
                Err(SessionError::Search(SearchError::QueryTooLong))
            ));
            assert!(choices.receive(latest_refresh, Ok(results(&[saved, tab]))));
            assert_target(&mut choices, saved);
            assert_eq!(choices.selected, Some(choices.lead_rows()));
            assert!(matches!(
                choices.activation(latest_refresh, saved),
                Some(PaletteEvent::Activate(target)) if target == saved
            ));
        }
    }

    #[test]
    fn removed_local_selection_never_activates_its_replacement_row() {
        let [tab, saved] = targets();
        for input in [ADDRESS, "synthetic"] {
            let mut choices = PaletteChoices::default();
            let request = choices.query(input);
            assert!(choices.receive(request, Ok(results(&[tab, saved]))));
            choices.select(choices.lead_rows());
            let refresh = choices.refresh();
            assert!(choices.receive(refresh, Ok(results(&[saved]))));
            assert!(choices.activation(request, tab).is_none());
            assert!(choices.activation(refresh, tab).is_none());
            if input == ADDRESS {
                assert_submit(&mut choices, input, false, ADDRESS);
                choices.next();
            } else {
                assert_eq!(choices.selected, None);
                assert!(choices.submission(input.to_owned(), false).is_none());
                let again = choices.refresh();
                assert!(choices.receive(again, Ok(results(&[saved]))));
                assert!(choices.submission(input.to_owned(), false).is_none());
                choices.next();
                assert_search(&mut choices, input);
                choices.next();
            }
            assert_target(&mut choices, saved);
        }
    }

    #[test]
    fn refresh_failure_and_input_change_keep_submission_intent_distinct() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let initial = choices.query("synthetic");
        assert!(choices.receive(initial, Ok(results(&[tab, saved]))));
        let refresh = choices.refresh();
        assert!(choices.receive(
            refresh,
            Err(SessionError::Search(SearchError::QueryTooLong))
        ));
        assert!(choices.results.is_empty());
        assert_eq!(
            choices.selected,
            Some(0),
            "the search row needs no local rows"
        );
        assert_search(&mut choices, "synthetic");
        assert_submit(&mut choices, "synthetic", true, "synthetic");

        // A new input query waits for its own reply, never an earlier one.
        let current = choices.query("new synthetic query");
        assert!(
            choices
                .submission("new synthetic query".to_owned(), false)
                .is_none()
        );
        assert!(!choices.receive(refresh, Ok(results(&[saved]))));
        assert!(choices.take_deferred().is_none());
        assert!(choices.receive(current, Ok(results(&[saved]))));
        assert!(matches!(
            choices.take_deferred(),
            Some(PaletteEvent::Submit { text, force_search: false }) if text == "new synthetic query"
        ));
    }

    #[test]
    fn go_to_selection_survives_refresh_and_supersedes_pending_local_intent() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let initial = choices.query(ADDRESS);
        assert!(choices.receive(initial, Ok(results(&[tab, saved]))));
        let refresh = choices.refresh();
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
        assert!(choices.receive(refresh, Ok(results(&[saved, tab]))));
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);

        choices.next();
        assert_target(&mut choices, saved);
        let pending = choices.refresh();
        assert!(choices.submission(ADDRESS.to_owned(), false).is_none());
        // Only Go to is available while local rows are being refreshed.
        choices.previous();
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
        let latest = choices.refresh();
        assert!(!choices.receive(pending, Ok(results(&[saved, tab]))));
        assert!(choices.receive(latest, Ok(results(&[saved, tab]))));
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn refresh_updates_saved_counts_and_metadata_from_current_results() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let initial = choices.query("synthetic");
        assert!(choices.receive(initial, Ok(results(&[tab]))));
        let refresh = choices.refresh();
        let mut current = results(&[tab, saved]);
        current.hits[0].title = "Changed synthetic title".to_owned();
        current.hits[0].address = "https://changed.example.test/".to_owned();
        assert!(choices.receive(refresh, Ok(current)));
        assert_eq!(choices.status, "1 tabs · 1 saved pages");
        assert_eq!(choices.results[0].title, "Changed synthetic title");
        assert_eq!(choices.results[0].address, "https://changed.example.test/");
        assert_search(&mut choices, "synthetic");
        choices.next();
        assert_target(&mut choices, tab);

        let removed = choices.refresh();
        assert!(choices.receive(removed, Ok(results(&[tab]))));
        assert_eq!(choices.status, "1 tabs · 0 saved pages");
        assert!(choices.activation(removed, saved).is_none());
    }

    fn command(key: u64) -> Target {
        Target::Command(std::num::NonZeroU64::new(key).expect("synthetic key"))
    }

    fn staged(keys: &[u64], strong: usize) -> CommandMatches {
        let targets: Vec<_> = keys.iter().map(|key| command(*key)).collect();
        let hits = results(&targets).hits;
        CommandMatches {
            counts: CommandCounts {
                browser: hits.len(),
                plugin: 0,
            },
            strong,
            hits,
        }
    }

    #[test]
    fn weak_commands_wait_for_their_own_reply_and_follow_pages() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let old = choices.query_scoped("synthetic", true);
        choices.stage_commands(staged(&[1], 0), true, false);
        assert!(
            choices.results.is_empty(),
            "weak rows wait for the worker reply"
        );
        // Enter before the reply waits for it; newer text then cancels that Enter.
        assert!(choices.submission("synthetic".to_owned(), false).is_none());
        let current = choices.query_scoped("synthetic newer", true);
        choices.stage_commands(staged(&[], 0), true, false);
        assert!(choices.deferred.is_none());

        // The older reply neither shows its commands nor replaces the chip counts.
        assert!(!choices.receive(old, Ok(results(&[tab]))));
        assert_eq!(choices.counts, ChipCounts::default());
        assert!(choices.receive(current, Ok(results(&[saved]))));
        assert_eq!(choices.results.len(), 1);
        assert_eq!(choices.counts.bookmarks, 1);

        // "close" must never close the tab: Enter searches, and pages lead weak commands.
        let next = choices.query_scoped("synthetic", true);
        choices.stage_commands(staged(&[1], 0), true, false);
        assert!(choices.receive(next, Ok(results(&[tab, saved]))));
        let order: Vec<_> = choices.results.iter().map(|hit| hit.target).collect();
        assert_eq!(order, [tab, saved, command(1)]);
        assert_eq!(choices.counts.everything, 3);
        assert_search(&mut choices, "synthetic");
        choices.next();
        assert_target(&mut choices, tab);

        // A worker failure keeps the local command rows.
        let failed = choices.query_scoped("synthetic", true);
        choices.stage_commands(staged(&[1], 0), true, false);
        assert!(choices.receive(failed, Err(SessionError::Search(SearchError::QueryTooLong))));
        assert_eq!(choices.results.len(), 1);
    }

    #[test]
    fn exact_title_commands_show_at_once_and_lead_pages() {
        let [tab, _] = targets();
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped("close tab", true);
        choices.stage_commands(staged(&[2, 3], 1), true, false);
        let shown: Vec<_> = choices.results.iter().map(|hit| hit.target).collect();
        assert_eq!(shown, [command(2)]);
        assert!(choices.receive(request, Ok(results(&[tab]))));
        let order: Vec<_> = choices.results.iter().map(|hit| hit.target).collect();
        assert_eq!(order, [command(2), tab, command(3)]);
        assert_target(&mut choices, command(2));
    }

    #[test]
    fn prefixed_queries_never_fall_back_to_navigation_or_search() {
        let mut choices = PaletteChoices::default();
        // Commands are local: `>rel` can run before the worker answers.
        let request = choices.query_scoped(">rel", false);
        choices.stage_commands(staged(&[4], 1), true, true);
        assert_target(&mut choices, command(4));
        assert!(choices.receive(request, Ok(results(&[]))));
        assert_target(&mut choices, command(4));

        // Nothing listed means Enter does nothing, never a web search for ">zzz".
        for input in [">zzz", "@", "/nothing"] {
            let request = choices.query_scoped(input, false);
            choices.stage_commands(staged(&[], 0), true, true);
            assert!(
                choices.submission(input.to_owned(), false).is_none(),
                "{input}"
            );
            assert!(choices.receive(request, Ok(results(&[]))));
            assert!(
                choices.submission(input.to_owned(), false).is_none(),
                "{input}"
            );
            // Alt Enter is an explicit request and still searches the web.
            assert_submit(&mut choices, input, true, input);
        }
    }

    #[test]
    fn only_a_moved_selection_is_announced_and_typing_withdraws_it() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped("synthetic target", true);
        choices.stage_commands(staged(&[], 0), true, false);
        assert!(choices.receive(request, Ok(results(&[tab, saved]))));
        assert_eq!(choices.row_count(), 3, "the search row, then two pages");
        // The search row is selected for Enter, but the query field keeps the focus.
        assert_eq!(choices.selected, Some(0));
        assert_eq!(choices.announced_row(), None);

        choices.next();
        assert_eq!(choices.announced_row(), Some(1));
        choices.previous();
        assert_eq!(choices.announced_row(), Some(0));

        // A model refresh keeps the moved row announced once its reply arrives.
        choices.next();
        let refreshed = choices.refresh();
        assert_eq!(choices.announced_row(), None, "no rows while waiting");
        assert!(choices.receive(refreshed, Ok(results(&[tab, saved]))));
        assert_eq!(choices.announced_row(), Some(1));

        // Typing hands the focus back to the query field.
        let typed = choices.query_scoped("synthetic target two", true);
        assert_eq!(choices.announced_row(), None);
        assert!(choices.receive(typed, Ok(results(&[tab]))));
        assert_eq!(choices.announced_row(), None);
    }

    #[test]
    fn an_empty_list_announces_nothing_even_after_a_move() {
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped("synthetic nothing", false);
        choices.stage_commands(staged(&[], 0), true, false);
        assert!(choices.receive(request, Ok(results(&[]))));
        choices.next();
        choices.previous();
        assert_eq!(choices.row_count(), 0);
        assert_eq!(choices.announced_row(), None);
    }

    #[test]
    fn a_moved_selection_stays_on_its_row_when_the_reply_arrives() {
        let [tab, _] = targets();
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped(">", false);
        choices.stage_commands(staged(&[1, 2], 2), false, true);
        choices.next();
        assert_target(&mut choices, command(2));
        assert!(choices.receive(request, Ok(results(&[tab]))));
        assert_target(&mut choices, command(2));
    }

    #[test]
    fn enter_before_the_reply_waits_then_searches_unless_a_row_matches_exactly() {
        let [tab, saved] = targets();
        for (address, expected) in [
            ("https://fuzzy.example.test/", None),
            ("https://shelf.example.test/", Some(saved)),
        ] {
            let mut choices = PaletteChoices::default();
            let request = choices.query_scoped("shelf", true);
            choices.stage_commands(staged(&[], 0), true, false);
            assert!(choices.submission("shelf".to_owned(), false).is_none());
            assert_eq!(choices.status, "Waiting for local results…");
            assert!(choices.take_deferred().is_none(), "no reply yet");
            let rows = ranked(&[
                (tab, "Synthetic shelves", "https://other.example.test/"),
                (saved, "Synthetic", address),
            ]);
            assert!(choices.receive(request, Ok(rows)));
            match (choices.take_deferred(), expected) {
                (Some(PaletteEvent::Activate(target)), Some(expected)) => {
                    assert_eq!(target, expected);
                }
                (
                    Some(PaletteEvent::Submit {
                        text,
                        force_search: false,
                    }),
                    None,
                ) => {
                    assert_eq!(text, "shelf");
                }
                _ => panic!("Enter chose the wrong row for {address}"),
            }
            assert!(choices.take_deferred().is_none(), "Enter is applied once");
        }
    }

    #[test]
    fn a_new_entry_forgets_the_last_one_but_keeps_counting_requests() {
        let [tab, _] = targets();
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped("synthetic target", true);
        choices.stage_commands(staged(&[], 0), true, false);
        assert!(choices.receive(request, Ok(results(&[tab]))));
        choices.next();
        let waiting = choices.query_scoped("synthetic target two", true);
        assert!(
            choices
                .submission("synthetic target two".to_owned(), false)
                .is_none()
        );

        choices.reset();
        assert!(choices.results.is_empty());
        assert_eq!(choices.selected, None);
        assert_eq!(choices.status, "");
        let next = choices.query_scoped("", true);
        assert!(next > waiting, "request IDs keep increasing across entries");
        assert!(
            !choices.receive(waiting, Ok(results(&[tab]))),
            "a reply to the old entry is ignored"
        );
        assert!(choices.receive(next, Ok(results(&[tab]))));
        assert!(choices.take_deferred().is_none(), "the old Enter is gone");
    }

    #[test]
    fn enter_waits_through_a_model_refresh_and_decides_on_the_refreshed_rows() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let first = choices.query_scoped("shelf", true);
        choices.stage_commands(staged(&[], 0), true, false);
        assert!(choices.submission("shelf".to_owned(), false).is_none());

        // A committed model change re-runs the same text before the first reply arrives.
        let refreshed = choices.refresh();
        assert_ne!(refreshed, first);
        choices.stage_commands(staged(&[], 0), true, false);
        assert!(
            !choices.receive(first, Ok(results(&[saved]))),
            "the superseded reply is ignored"
        );
        assert!(choices.take_deferred().is_none(), "Enter still waits");
        let rows = ranked(&[
            (tab, "Synthetic shelves", "https://other.example.test/"),
            (saved, "Synthetic", "https://shelf.example.test/"),
        ]);
        assert!(choices.receive(refreshed, Ok(rows)));
        assert!(matches!(
            choices.take_deferred(),
            Some(PaletteEvent::Activate(target)) if target == saved
        ));
        assert!(choices.take_deferred().is_none(), "Enter is applied once");
    }

    #[test]
    fn enter_before_an_empty_reply_searches_only_once_nothing_matched() {
        let input = "synthetic nothing";
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped(input, true);
        choices.stage_commands(staged(&[], 0), true, false);
        assert!(choices.submission(input.to_owned(), false).is_none());
        assert!(choices.receive(request, Ok(results(&[]))));
        assert!(matches!(
            choices.take_deferred(),
            Some(PaletteEvent::Submit { text, force_search: false }) if text == input
        ));

        // Alt Enter is explicit and searches at once; Go to is known before the reply.
        choices.query_scoped(input, true);
        assert_submit(&mut choices, input, true, input);
        choices.query_scoped(ADDRESS, true);
        assert_submit(&mut choices, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn scoped_views_never_offer_the_go_to_row() {
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped(ADDRESS, false);
        assert_eq!(choices.address, None);
        assert_eq!(choices.status, "Searching locally…");
        assert!(choices.receive(request, Ok(results(&targets()))));
        assert_target(&mut choices, targets()[0]);

        // Refused addresses only explain themselves where an address could be entered.
        choices.query_scoped("JAVASCRIPT:1", false);
        assert_eq!(choices.status, "Searching locally…");
    }
}

//! Tracks the palette's rows, selection and request identity so stale replies never replace newer ones.

use atlas_search::{SearchHit, SearchResults, Target};

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
    Target(Target),
    Unselected,
}

impl PaletteChoices {
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
        // Refused input has no address row; submission still uses the existing validator.
        self.address = match navigation::recognized_address(text) {
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
    }

    pub(super) fn refresh(&mut self) -> u64 {
        // Repeated model changes retain intent even after the visible rows were invalidated.
        let selection = self.refresh_selection.unwrap_or_else(|| self.selection());
        self.begin_query();
        self.selected = if matches!(selection, RefreshSelection::Default) && self.address.is_some()
        {
            Some(0)
        } else {
            None
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
            Some(RefreshSelection::Default) if !succeeded && self.address.is_none() => None,
            Some(RefreshSelection::Default) => Some(0),
            None if self.address.is_some() || self.moved => selected_target
                .and_then(|target| self.target_row(target))
                .or(Some(0)),
            None => Some(0),
        };
        true
    }

    fn target_row(&self, target: Target) -> Option<usize> {
        self.results
            .iter()
            .position(|hit| hit.target == target)
            .map(|index| index + self.address_rows())
            .or_else(|| self.address.as_ref().map(|_| 0))
    }

    pub(super) fn address_rows(&self) -> usize {
        usize::from(self.address.is_some())
    }

    pub(super) fn selected_hit(&self) -> Option<&SearchHit> {
        self.selected
            .and_then(|selected| selected.checked_sub(self.address_rows()))
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
        let rows = self.results.len() + self.address_rows();
        if rows == 0 {
            return;
        }
        self.select(
            self.selected
                .map_or(0, |selected| selected.saturating_add(1).min(rows - 1)),
        );
    }

    pub(super) fn previous(&mut self) {
        if self.results.is_empty() && self.address.is_none() {
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
    fn must_wait_for_rows(&self, force_search: bool) -> bool {
        let go_to_selected = self.selected == Some(0) && self.address.is_some();
        !force_search && self.awaiting_reply && self.selected_hit().is_none() && !go_to_selected
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

#[cfg(test)]
mod tests {
    use atlas_core::{SavedPageId, TabId};
    use atlas_search::{ScopeCounts, SearchError};

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
    fn ordinary_and_refused_input_keep_existing_submission_behavior() {
        for input in [
            "ordinary search terms",
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
            choices.select(choices.address_rows() + 1);
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
            assert_eq!(choices.selected, Some(choices.address_rows()));
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
            choices.select(choices.address_rows());
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
        assert!(choices.submission("synthetic".to_owned(), false).is_none());
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
            Some(PaletteEvent::Activate(target)) if target == saved
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

        // "close" must open a page rather than close the tab: pages lead weak commands.
        let next = choices.query_scoped("synthetic", true);
        choices.stage_commands(staged(&[1], 0), true, false);
        assert!(choices.receive(next, Ok(results(&[tab, saved]))));
        let order: Vec<_> = choices.results.iter().map(|hit| hit.target).collect();
        assert_eq!(order, [tab, saved, command(1)]);
        assert_eq!(choices.counts.everything, 3);
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
    fn enter_before_the_reply_opens_its_top_row_instead_of_searching() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped("synthetic target", true);
        choices.stage_commands(staged(&[], 0), true, false);
        assert!(
            choices
                .submission("synthetic target".to_owned(), false)
                .is_none()
        );
        assert_eq!(choices.status, "Waiting for local results…");
        assert!(choices.take_deferred().is_none(), "no reply yet");
        assert!(choices.receive(request, Ok(results(&[tab, saved]))));
        assert!(matches!(
            choices.take_deferred(),
            Some(PaletteEvent::Activate(target)) if target == tab
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

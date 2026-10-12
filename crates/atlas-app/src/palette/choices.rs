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
    /// Command rows first, then the worker's rows; replaced only by the current request's reply.
    pub(super) results: Vec<SearchHit>,
    pub(super) selected: Option<usize>,
    refresh_selection: Option<RefreshSelection>,
    pub(super) status: String,
    /// Locally matched commands waiting to join the current request's reply.
    staged: Vec<SearchHit>,
    staged_counts: CommandCounts,
    typed: bool,
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

    /// Holds this request's command rows until its worker reply arrives, so rows change together.
    pub(super) fn stage_commands(&mut self, matches: CommandMatches, typed: bool) {
        self.staged = matches.hits;
        self.staged_counts = matches.counts;
        self.typed = typed;
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
        let succeeded = result.is_ok();
        match result {
            Ok(results) => {
                self.status = format!(
                    "{} tabs · {} saved pages",
                    results.counts.tabs, results.counts.saved
                );
                self.counts = ChipCounts::new(results.counts, self.staged_counts, self.typed);
                self.results = self.staged.iter().cloned().chain(results.hits).collect();
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
            None if self.address.is_some() => selected_target
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

    pub(super) fn submission(&self, text: String, force_search: bool) -> Option<PaletteEvent> {
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

    fn assert_submit(choices: &PaletteChoices, input: &str, force_search: bool, expected: &str) {
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

    fn assert_target(choices: &PaletteChoices, expected: Target) {
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
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        choices.next();
        assert_eq!(
            choices.selected,
            Some(0),
            "Down cannot select an unavailable local row"
        );
        assert!(choices.receive(request, Ok(results(&targets()))));
        assert_eq!(choices.selected, Some(0));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        assert!(choices.receive(request, Ok(results(&[]))));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        assert!(choices.receive(
            request,
            Err(SessionError::Search(SearchError::QueryTooLong))
        ));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn deliberate_local_selection_tracks_target_and_arrows_return_to_go_to() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let request = choices.query(ADDRESS);
        assert!(choices.receive(request, Ok(results(&[tab, saved]))));
        choices.next();
        assert_eq!(choices.selected, Some(1));
        assert_target(&choices, tab);
        choices.previous();
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        choices.previous();
        assert_eq!(choices.selected, Some(0));
        choices.next();
        choices.next();
        choices.next();
        assert_eq!(choices.selected, Some(2));
        assert_target(&choices, saved);

        // An updated ranking must not quietly change the chosen destination.
        assert!(choices.receive(request, Ok(results(&[saved, tab]))));
        assert_eq!(choices.selected, Some(1));
        assert_target(&choices, saved);
        assert!(choices.receive(request, Ok(results(&[tab]))));
        assert_eq!(choices.selected, Some(0));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn input_changes_reject_stale_replies_and_reopening_resets_selection() {
        let mut choices = PaletteChoices::default();
        let old = choices.query(ADDRESS);
        assert!(choices.receive(old, Ok(results(&targets()))));
        choices.next();
        assert_target(&choices, targets()[0]);
        let next_address = "https://next-palette.example.test/";
        let next = choices.query(next_address);
        assert_ne!(next, old);
        assert_eq!(choices.selected, Some(0));
        let status = choices.status.clone();
        assert!(!choices.receive(old, Ok(results(&targets()))));
        assert!(!choices.receive(old, Err(SessionError::Search(SearchError::QueryTooLong))));
        assert_eq!(choices.status, status);
        assert!(choices.results.is_empty());
        assert_submit(&choices, next_address, false, next_address);
        assert!(choices.receive(next, Ok(results(&targets()))));
        assert_submit(&choices, next_address, false, next_address);

        // The owning UI correlates popup identities; a new popup starts fresh local state.
        let mut reopened = PaletteChoices::default();
        assert_eq!(reopened.query(ADDRESS), old);
        assert!(reopened.results.is_empty());
        assert_eq!(reopened.selected, Some(0));
        assert_submit(&reopened, ADDRESS, false, ADDRESS);
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
            assert_submit(&choices, input, false, input);
            if let Err(error) = navigation::recognized_address(input) {
                assert_eq!(choices.status, error);
            }
            assert!(choices.receive(request, Ok(results(&targets()))));
            assert_target(&choices, targets()[0]);
            choices.next();
            assert_target(&choices, targets()[1]);
            choices.previous();
            assert_target(&choices, targets()[0]);
            assert!(choices.receive(
                request,
                Err(SessionError::Search(SearchError::QueryTooLong))
            ));
            assert_submit(&choices, input, false, input);
        }
    }

    #[test]
    fn go_to_uses_canonical_address_but_forced_search_preserves_input() {
        let input = "  HTTPS://palette.example.test/Case?Key=Value  ";
        let mut choices = PaletteChoices::default();
        let request = choices.query(input);
        assert_eq!(choices.address.as_deref(), Some(ADDRESS));
        assert_submit(&choices, input, false, ADDRESS);
        assert_submit(&choices, input, true, input);
        assert!(choices.receive(request, Ok(results(&targets()))));
        choices.next();
        assert_target(&choices, targets()[0]);
        assert_submit(&choices, input, true, input);

        // Clicking Go to explicitly selects its row, regardless of the previous local choice.
        choices.select(0);
        assert_submit(&choices, input, false, ADDRESS);
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
        assert_target(&choices, first);
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
            assert_target(&choices, saved);

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
            assert_target(&choices, saved);
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
                assert_submit(&choices, input, false, ADDRESS);
                choices.next();
            } else {
                assert_eq!(choices.selected, None);
                assert!(choices.submission(input.to_owned(), false).is_none());
                let again = choices.refresh();
                assert!(choices.receive(again, Ok(results(&[saved]))));
                assert!(choices.submission(input.to_owned(), false).is_none());
                choices.next();
            }
            assert_target(&choices, saved);
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
        assert_submit(&choices, "synthetic", true, "synthetic");

        // A new input query keeps the established no-result submission behavior.
        let current = choices.query("new synthetic query");
        assert_submit(
            &choices,
            "new synthetic query",
            false,
            "new synthetic query",
        );
        assert!(!choices.receive(refresh, Ok(results(&[saved]))));
        assert!(choices.receive(current, Ok(results(&[saved]))));
        assert_target(&choices, saved);
    }

    #[test]
    fn go_to_selection_survives_refresh_and_supersedes_pending_local_intent() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let initial = choices.query(ADDRESS);
        assert!(choices.receive(initial, Ok(results(&[tab, saved]))));
        let refresh = choices.refresh();
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        assert!(choices.receive(refresh, Ok(results(&[saved, tab]))));
        assert_submit(&choices, ADDRESS, false, ADDRESS);

        choices.next();
        assert_target(&choices, saved);
        let pending = choices.refresh();
        assert!(choices.submission(ADDRESS.to_owned(), false).is_none());
        // Only Go to is available while local rows are being refreshed.
        choices.previous();
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        let latest = choices.refresh();
        assert!(!choices.receive(pending, Ok(results(&[saved, tab]))));
        assert!(choices.receive(latest, Ok(results(&[saved, tab]))));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
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
        assert_target(&choices, tab);

        let removed = choices.refresh();
        assert!(choices.receive(removed, Ok(results(&[tab]))));
        assert_eq!(choices.status, "1 tabs · 0 saved pages");
        assert!(choices.activation(removed, saved).is_none());
    }

    fn staged(keys: &[u64]) -> CommandMatches {
        let hits = results(
            &keys
                .iter()
                .map(|key| Target::Command(std::num::NonZeroU64::new(*key).expect("synthetic key")))
                .collect::<Vec<_>>(),
        )
        .hits;
        CommandMatches {
            counts: CommandCounts {
                browser: hits.len(),
                plugin: 0,
            },
            hits,
        }
    }

    #[test]
    fn staged_commands_appear_only_with_their_own_reply() {
        let [tab, saved] = targets();
        let command = Target::Command(std::num::NonZeroU64::MIN);
        let mut choices = PaletteChoices::default();
        let old = choices.query_scoped("synthetic", true);
        choices.stage_commands(staged(&[1]), true);
        assert!(choices.results.is_empty(), "rows wait for the worker reply");
        let current = choices.query_scoped("synthetic newer", true);
        choices.stage_commands(staged(&[]), true);

        // The older reply neither shows its commands nor replaces the chip counts.
        assert!(!choices.receive(old, Ok(results(&[tab]))));
        assert_eq!(choices.counts, ChipCounts::default());
        assert!(choices.receive(current, Ok(results(&[saved]))));
        assert_eq!(choices.results.len(), 1);
        assert_eq!(choices.counts.bookmarks, 1);

        let next = choices.query_scoped("synthetic", true);
        choices.stage_commands(staged(&[1]), true);
        assert!(choices.receive(next, Ok(results(&[tab, saved]))));
        let order: Vec<_> = choices.results.iter().map(|hit| hit.target).collect();
        assert_eq!(order, [command, tab, saved]);
        assert_eq!(choices.counts.everything, 3);
        assert!(matches!(
            choices.submission("synthetic".to_owned(), false),
            Some(PaletteEvent::Activate(target)) if target == command
        ));

        // A worker failure keeps the local command rows.
        let failed = choices.query_scoped("synthetic", true);
        choices.stage_commands(staged(&[1]), true);
        assert!(choices.receive(failed, Err(SessionError::Search(SearchError::QueryTooLong))));
        assert_eq!(choices.results.len(), 1);
    }

    #[test]
    fn scoped_views_never_offer_the_go_to_row() {
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped(ADDRESS, false);
        assert_eq!(choices.address, None);
        assert_eq!(choices.status, "Searching locally…");
        assert!(choices.receive(request, Ok(results(&targets()))));
        assert_target(&choices, targets()[0]);

        // Refused addresses only explain themselves where an address could be entered.
        choices.query_scoped("JAVASCRIPT:1", false);
        assert_eq!(choices.status, "Searching locally…");
    }
}

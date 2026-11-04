//! Decides what each engine report did to a tab's back and forward list (#126), on every platform.

use syrinx_core::HistoryChange;

/// What the latest navigation said about itself when it started.
#[derive(Clone, Copy)]
pub(super) enum Started {
    NewDocument { user_initiated: bool },
    BackOrForward,
    Reload,
}

/// Facts gathered between two `HistoryChanged` events.
#[derive(Clone, Copy, Default)]
pub(super) struct Pending {
    /// The latest navigation's id and kind, until it commits or fails.
    pub(super) started: Option<(u64, Started)>,
    /// Set by `SourceChanged`: whether the new address came with a new document.
    pub(super) new_document: Option<bool>,
    /// The latest step back or forward failed and no navigation has started since, so the
    /// jump it was for is abandoned. Reported with the next history change: each middle step
    /// of a jump fails just before the next one starts (spike, cacheable pages).
    pub(super) abandoned: bool,
}

impl Pending {
    /// The change the spike's event sequences describe. A reload, and any visit to the
    /// address already shown, raises no `SourceChanged` (trapstoknow, F14).
    pub(super) fn change(self) -> HistoryChange {
        let started = self.started.map(|(_, started)| started);
        match (self.new_document, started) {
            (Some(false), started) => HistoryChange::SamePage {
                navigating: started.is_some(),
            },
            (Some(true), Some(Started::BackOrForward)) => HistoryChange::BackOrForward,
            (Some(true), Some(Started::NewDocument { user_initiated })) => {
                HistoryChange::NewDocument { user_initiated }
            }
            // A reload that redirects replaces its entry, so it adds no row.
            (Some(true), Some(Started::Reload)) => HistoryChange::Unchanged,
            // A new document with no start seen is Syrinx's own or the engine's: user-made.
            (Some(true), None) => HistoryChange::NewDocument {
                user_initiated: true,
            },
            // A step back or forward to an entry with the shown address still needs its
            // landing checked against the requested jump.
            (None, Some(Started::BackOrForward)) => HistoryChange::BackOrForward,
            // Chromium turns a new visit to the shown address into a replacement, so it adds
            // no row; a form posted back to the same address is the known exception.
            (None, Some(Started::NewDocument { .. } | Started::Reload) | None) => {
                HistoryChange::Unchanged
            }
        }
    }

    /// What is left once a `HistoryChanged` has reported `change`. A same-page change keeps
    /// a navigation still in flight, such as a Back during a slow load.
    pub(super) fn after(self, change: HistoryChange) -> Self {
        match change {
            HistoryChange::SamePage { .. } => Self {
                new_document: None,
                ..self
            },
            HistoryChange::NewDocument { .. }
            | HistoryChange::BackOrForward
            | HistoryChange::Unchanged
            | HistoryChange::JumpAbandoned => Self::default(),
        }
    }

    /// The changes one `HistoryChanged` reports, in order, and the facts left afterwards. An
    /// abandoned jump is reported first as `JumpAbandoned`, which only drops the jump, so the
    /// change after it is not checked against it and no row is renamed (review of #212).
    pub(super) fn reports(self) -> (Vec<HistoryChange>, Self) {
        let change = self.change();
        let mut reports = Vec::with_capacity(2);
        if self.abandoned {
            reports.push(HistoryChange::JumpAbandoned);
        }
        reports.push(change);
        let next = Self {
            abandoned: false,
            ..self
        }
        .after(change);
        (reports, next)
    }

    /// Navigation `id` has started; any abandoned step before it was a middle step.
    pub(super) fn start(self, id: u64, started: Started) -> Self {
        Self {
            started: Some((id, started)),
            abandoned: false,
            ..self
        }
    }

    /// Navigation `id` finished. It is forgotten either way, so a later `HistoryChanged` is
    /// never read as its own; a failed latest step back or forward abandons its jump. A commit's
    /// `HistoryChanged` always came before its `NavigationCompleted` in the spike.
    pub(super) fn complete(self, id: u64, success: bool) -> Self {
        let latest_step =
            matches!(self.started, Some((started, Started::BackOrForward)) if started == id);
        Self {
            started: self.started.filter(|(started, _)| *started != id),
            abandoned: self.abandoned || (latest_step && !success),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(started: Option<Started>, new_document: Option<bool>) -> Pending {
        Pending {
            started: started.map(|started| (7, started)),
            new_document,
            abandoned: false,
        }
    }

    fn change(started: Option<Started>, new_document: Option<bool>) -> HistoryChange {
        pending(started, new_document).change()
    }

    const TYPED: Option<Started> = Some(Started::NewDocument {
        user_initiated: true,
    });

    #[test]
    fn an_abandoned_jump_is_reported_as_dropped_before_the_change_that_revealed_it() {
        // Review of #213: it was reported as `Unchanged`, which renamed the current row.
        let typed_after_a_failed_back = Pending {
            abandoned: true,
            ..pending(TYPED, Some(true))
        };
        let (reports, next) = typed_after_a_failed_back.reports();
        assert_eq!(
            reports,
            [
                HistoryChange::JumpAbandoned,
                HistoryChange::NewDocument {
                    user_initiated: true
                }
            ]
        );
        assert!(!next.abandoned);
        let (quiet, _) = pending(TYPED, Some(true)).reports();
        assert_eq!(quiet.len(), 1, "no abandonment, no extra report");
    }

    #[test]
    fn a_same_document_address_change_is_a_same_page_change() {
        assert_eq!(
            change(None, Some(false)),
            HistoryChange::SamePage { navigating: false }
        );
    }

    #[test]
    fn a_same_page_change_during_another_navigation_is_the_page_s_own() {
        // A replaceState while a Back loads must not be read as the Back's landing.
        assert_eq!(
            change(Some(Started::BackOrForward), Some(false)),
            HistoryChange::SamePage { navigating: true }
        );
        assert_eq!(
            change(TYPED, Some(false)),
            HistoryChange::SamePage { navigating: true }
        );
    }

    #[test]
    fn the_middle_steps_of_a_jump_do_not_abandon_it() {
        // The spike: each cancelled middle step completes just before the next one starts.
        let jump = Pending::default()
            .start(33, Started::BackOrForward)
            .complete(33, false)
            .start(34, Started::BackOrForward)
            .complete(34, false)
            .start(35, Started::BackOrForward);
        assert!(!jump.abandoned);
    }

    #[test]
    fn a_middle_step_that_completes_after_the_next_start_does_not_abandon_it() {
        let jump = Pending::default()
            .start(33, Started::BackOrForward)
            .start(34, Started::BackOrForward)
            .complete(33, false);
        assert!(!jump.abandoned);
    }

    #[test]
    fn a_failed_last_step_abandons_its_jump() {
        let stayed = Pending::default()
            .start(40, Started::BackOrForward)
            .complete(40, false);
        assert!(stayed.abandoned);
        let typed = Pending::default()
            .start(
                41,
                Started::NewDocument {
                    user_initiated: true,
                },
            )
            .complete(41, false);
        assert!(
            !typed.abandoned,
            "only a step back or forward abandons a jump"
        );
    }

    #[test]
    fn a_finished_navigation_is_forgotten_either_way() {
        let done = Pending::default()
            .start(50, Started::BackOrForward)
            .complete(50, true);
        assert_eq!(done.change(), HistoryChange::Unchanged);
        assert!(!done.abandoned);
    }

    #[test]
    fn a_new_document_carries_its_navigation_kind() {
        let script = Some(Started::NewDocument {
            user_initiated: false,
        });
        assert_eq!(
            change(script, Some(true)),
            HistoryChange::NewDocument {
                user_initiated: false
            }
        );
        assert_eq!(
            change(Some(Started::BackOrForward), Some(true)),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_reload_or_a_change_without_a_new_address_is_unchanged() {
        assert_eq!(
            change(Some(Started::Reload), None),
            HistoryChange::Unchanged
        );
        assert_eq!(change(None, None), HistoryChange::Unchanged);
    }

    #[test]
    fn a_reload_that_redirects_replaces_rather_than_adds() {
        assert_eq!(
            change(Some(Started::Reload), Some(true)),
            HistoryChange::Unchanged
        );
    }

    #[test]
    fn a_step_to_an_entry_at_the_shown_address_is_still_a_step() {
        // No SourceChanged arrives, but the jump's landing must still be checked.
        assert_eq!(
            change(Some(Started::BackOrForward), None),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_new_visit_to_the_shown_address_replaces_rather_than_adds() {
        assert_eq!(change(TYPED, None), HistoryChange::Unchanged);
    }

    #[test]
    fn a_navigation_that_never_committed_is_forgotten() {
        let started = pending(Some(Started::BackOrForward), None);
        assert_eq!(
            started.complete(7, false).change(),
            HistoryChange::Unchanged
        );
        // A late failure of an older navigation leaves the newer one in place.
        assert_eq!(
            started.complete(6, false).change(),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_same_page_change_keeps_a_navigation_still_in_flight() {
        // A replaceState during a slow Back must not turn the Back's commit into a push.
        let during = pending(Some(Started::BackOrForward), Some(false));
        let left = during.after(during.change());
        assert_eq!(
            Pending {
                new_document: Some(true),
                ..left
            }
            .change(),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_committed_change_clears_everything() {
        let committed = pending(TYPED, Some(true));
        let left = committed.after(committed.change());
        assert_eq!(left.change(), HistoryChange::Unchanged);
    }
}

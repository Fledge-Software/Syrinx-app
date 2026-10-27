//! Keeps each tab's mute for this run only, reapplies it when its page is rebuilt, and shows
//! what each page reports (#95).

use super::*;

/// What the status line says when the WebView2 runtime refuses to mute a page.
const CANNOT_MUTE: &str = "This WebView2 runtime cannot mute pages.";

impl BrowserView {
    /// A new or restored page starts audible, so a muted tab is muted again.
    pub(super) fn reapply_mute(&mut self, id: u64) {
        let Some(command) = reapplied_mute(&self.muted, id) else {
            return;
        };
        self.page_command(command);
    }

    /// Records the owner's mute choice and asks the tab's live page to follow it. The badge
    /// waits for the page's own report (`audio_reported`); a tab with no page shows none.
    pub(super) fn set_tab_muted(&mut self, tab: TabId, mute: bool) {
        let reported = self.shown_muted.contains(&tab);
        let live = self.loaded.contains(&tab);
        if !live {
            self.shown_muted.remove(&tab);
        }
        if let Some(command) = change_mute(&mut self.muted, live, reported, tab, mute) {
            self.page_command(command);
        }
    }

    /// The tab's page is gone or new, so nothing reports it muted any more.
    pub(super) fn page_gone(&mut self, tab: TabId) {
        self.shown_muted.remove(&tab);
    }

    /// The page reported its mute state. It is stored as reported, never flipped, so a
    /// repeated event changes nothing; the sidebar's badge follows only this.
    pub(super) fn audio_reported(&mut self, id: u64, muted: bool, cx: &mut Context<Self>) {
        let Ok(tab) = TabId::new(id) else {
            return;
        };
        if record_reported_mute(&mut self.shown_muted, tab, muted) {
            cx.notify();
        }
    }

    /// The runtime refused a mute (`BrowserError::Mute`, which no other control reports): the
    /// choice is dropped, so no badge or Unmute item claims otherwise, and the status line
    /// says why.
    pub(super) fn mute_refused(&mut self, id: u64) {
        if let Ok(tab) = TabId::new(id) {
            self.muted.remove(&tab);
        }
        self.message = CANNOT_MUTE.to_owned();
    }

    /// Forgets mute state for tabs that have closed.
    pub(super) fn retain_mute(&mut self) {
        let open: HashSet<TabId> = self.model.tabs.iter().map(|tab| tab.id).collect();
        self.muted.retain(|id| open.contains(id));
        self.shown_muted.retain(|id| open.contains(id));
    }
}

/// Records the owner's mute choice; returns the command for the tab's live page, sent only
/// when the page `reported` the other state (WebView2 raises no event for an unchanged one).
fn change_mute(
    muted: &mut HashSet<TabId>,
    live: bool,
    reported: bool,
    id: TabId,
    mute: bool,
) -> Option<BrowserCommand> {
    if mute {
        muted.insert(id);
    } else {
        muted.remove(&id);
    }
    (live && reported != mute).then_some(BrowserCommand::SetMuted {
        id: id.get(),
        muted: mute,
    })
}

/// The tab whose mute the runtime refused, if `error` is that refusal. Only `Mute` counts: a
/// failed zoom or find on the same tab is `PageControl` and never drops a mute choice.
pub(super) fn refused_mute(
    tab: Option<u64>,
    error: &syrinx_platform::browser::BrowserError,
) -> Option<u64> {
    match (tab, error) {
        (Some(id), syrinx_platform::browser::BrowserError::Mute) => Some(id),
        _ => None,
    }
}

/// Sets the tab's reported state; returns whether it changed.
fn record_reported_mute(shown: &mut HashSet<TabId>, tab: TabId, muted: bool) -> bool {
    if muted {
        shown.insert(tab)
    } else {
        shown.remove(&tab)
    }
}

/// The command that mutes a rebuilt page again, or `None` for a tab left audible.
fn reapplied_mute(muted: &HashSet<TabId>, id: u64) -> Option<BrowserCommand> {
    let tab = TabId::new(id).ok()?;
    muted
        .contains(&tab)
        .then_some(BrowserCommand::SetMuted { id, muted: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_muted_tab_is_muted_again_when_its_page_is_rebuilt() {
        let muted = HashSet::from([TabId::new(7).unwrap()]);
        assert!(matches!(
            reapplied_mute(&muted, 7),
            Some(BrowserCommand::SetMuted { id: 7, muted: true })
        ));
        assert!(reapplied_mute(&muted, 8).is_none());
        assert!(reapplied_mute(&muted, 0).is_none(), "no tab has ID zero");
    }

    #[test]
    fn a_mute_choice_is_kept_and_reaches_only_a_live_page() {
        let live = TabId::new(7).unwrap();
        let unloaded = TabId::new(8).unwrap();
        let mut muted = HashSet::new();
        assert!(matches!(
            change_mute(&mut muted, true, false, live, true),
            Some(BrowserCommand::SetMuted { id: 7, muted: true })
        ));
        assert!(change_mute(&mut muted, false, false, unloaded, true).is_none());
        assert_eq!(muted, HashSet::from([live, unloaded]));
        assert!(matches!(
            change_mute(&mut muted, true, true, live, false),
            Some(BrowserCommand::SetMuted {
                id: 7,
                muted: false
            })
        ));
        assert_eq!(muted, HashSet::from([unloaded]));
    }

    #[test]
    fn only_a_mute_failure_is_read_as_a_refused_mute() {
        use syrinx_platform::browser::BrowserError;
        assert_eq!(refused_mute(Some(7), &BrowserError::Mute), Some(7));
        // A zoom or find failure on a tab with a mute in flight is not a mute refusal.
        assert_eq!(refused_mute(Some(7), &BrowserError::PageControl), None);
        assert_eq!(refused_mute(None, &BrowserError::Mute), None);
    }

    #[test]
    fn a_page_already_in_the_chosen_state_is_sent_nothing() {
        let tab = TabId::new(7).unwrap();
        let mut muted = HashSet::new();
        // WebView2 raises no event for an unchanged state, so nothing would answer.
        assert!(change_mute(&mut muted, true, true, tab, true).is_none());
        assert!(muted.contains(&tab), "the choice is still kept");
        assert!(change_mute(&mut muted, true, false, tab, false).is_none());
        assert!(muted.is_empty());
    }

    #[test]
    fn a_reported_state_is_set_never_toggled() {
        let tab = TabId::new(7).unwrap();
        let mut shown = HashSet::new();
        assert!(record_reported_mute(&mut shown, tab, true));
        // WebView2 can report one change twice; the second changes nothing, and a muted page
        // that starts playing reports muted again, so the badge stays.
        assert!(!record_reported_mute(&mut shown, tab, true));
        assert_eq!(shown, HashSet::from([tab]));
        assert!(record_reported_mute(&mut shown, tab, false));
        assert!(!record_reported_mute(&mut shown, tab, false));
        assert!(shown.is_empty());
    }
}

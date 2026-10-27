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
        if self.page_command(command)
            && let Ok(tab) = TabId::new(id)
        {
            self.mute_requests.insert(tab);
        }
    }

    /// The page reported its mute state. It is stored as reported, never flipped, so a
    /// repeated event changes nothing; the sidebar's badge follows only this.
    pub(super) fn audio_reported(&mut self, id: u64, muted: bool, cx: &mut Context<Self>) {
        let Ok(tab) = TabId::new(id) else {
            return;
        };
        self.mute_requests.remove(&tab);
        if record_reported_mute(&mut self.shown_muted, tab, muted) {
            cx.notify();
        }
    }

    /// A page control failed. When it was a mute this run asked for, the runtime cannot mute:
    /// the choice is dropped, so no badge or Unmute item claims otherwise, and the status line
    /// says why. Returns whether the failure was a mute.
    pub(super) fn mute_refused(&mut self, id: u64) -> bool {
        let Ok(tab) = TabId::new(id) else {
            return false;
        };
        if !self.mute_requests.remove(&tab) {
            return false;
        }
        self.muted.remove(&tab);
        self.message = CANNOT_MUTE.to_owned();
        true
    }

    /// Forgets mute state for tabs that have closed.
    pub(super) fn retain_mute(&mut self) {
        let open: HashSet<TabId> = self.model.tabs.iter().map(|tab| tab.id).collect();
        self.muted.retain(|id| open.contains(id));
        self.shown_muted.retain(|id| open.contains(id));
        self.mute_requests.retain(|id| open.contains(id));
    }
}

/// Records the owner's mute choice; returns the command for the tab's live page, if any.
pub(super) fn change_mute(
    muted: &mut HashSet<TabId>,
    loaded: &HashSet<TabId>,
    id: TabId,
    mute: bool,
) -> Option<BrowserCommand> {
    if mute {
        muted.insert(id);
    } else {
        muted.remove(&id);
    }
    loaded.contains(&id).then_some(BrowserCommand::SetMuted {
        id: id.get(),
        muted: mute,
    })
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
        let loaded = HashSet::from([live]);
        let mut muted = HashSet::new();
        assert!(matches!(
            change_mute(&mut muted, &loaded, live, true),
            Some(BrowserCommand::SetMuted { id: 7, muted: true })
        ));
        assert!(change_mute(&mut muted, &loaded, unloaded, true).is_none());
        assert_eq!(muted, HashSet::from([live, unloaded]));
        assert!(matches!(
            change_mute(&mut muted, &loaded, live, false),
            Some(BrowserCommand::SetMuted {
                id: 7,
                muted: false
            })
        ));
        assert_eq!(muted, HashSet::from([unloaded]));
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

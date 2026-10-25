//! Keeps each tab's mute for this run only and reapplies it when its page is rebuilt (#95).

use super::*;

impl BrowserView {
    /// A new or restored page starts audible, so a muted tab is muted again.
    pub(super) fn reapply_mute(&mut self, id: u64) {
        if let Some(command) = reapplied_mute(&self.muted, id) {
            self.page_command(command);
        }
    }
}

// TODO(#95): call from the row menu's Mute action, then drop cfg(test).
/// Records the owner's mute choice; returns the command for the tab's live page, if any.
#[cfg(test)]
fn change_mute(
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
}

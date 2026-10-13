//! Keeps a fresh synthetic session hidden for the separate eight-tab idle measurement.

use super::*;

pub(super) struct SuspendedFixture {
    count: usize,
    suspended: HashSet<u64>,
}
impl SuspendedFixture {
    pub(super) fn new(count: usize) -> Self {
        Self {
            count,
            suspended: HashSet::new(),
        }
    }
    pub(super) fn enabled(&self) -> bool {
        self.count > 0
    }
    pub(super) fn observe(&mut self, id: u64, suspended: bool) {
        if self.count == 0 {
            return;
        }
        if suspended {
            self.suspended.insert(id);
        } else {
            self.suspended.remove(&id);
        }
        println!(
            "Synthetic suspended tabs: {}/{} (native readback).",
            self.suspended.len(),
            self.count
        );
    }
}

impl BrowserView {
    pub(super) fn seed_suspended_fixture(&mut self) {
        for _ in 0..self.suspended_fixture.count {
            self.command(SessionCommand::NewTab {
                address: "about:blank".into(),
            });
        }
    }
    pub(super) fn sync_suspended_fixture(&mut self) -> bool {
        if self.suspended_fixture.count == 0 {
            return false;
        }
        if !self.browser_ready {
            return true;
        }
        let missing: Vec<_> = self
            .model
            .tabs
            .iter()
            .filter(|tab| !self.loaded.contains(&tab.id) && !self.pending_pages.contains(&tab.id))
            .map(|tab| tab.id)
            .collect();
        for id in missing {
            if self.page_command(BrowserCommand::Create {
                id: id.get(),
                url: "about:blank".into(),
            }) {
                self.pending_pages.insert(id);
            }
        }
        true
    }
}

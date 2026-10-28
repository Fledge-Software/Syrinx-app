//! Keeps each tab's back and forward list in memory, rebuilt from engine reports (#126).

use std::collections::HashMap;
use std::fmt;

use super::{TabError, TabId, TabModel};

/// The most rows a back or forward list shows, as Chrome does.
pub const HISTORY_ROW_LIMIT: usize = 15;
/// The most entries kept per tab: Chromium's own limit for a tab's history.
pub const HISTORY_ENTRY_LIMIT: usize = 50;

/// What the engine says a committed change did to the tab's history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryChange {
    /// A new document; `user_initiated` false means the page moved on by itself.
    NewDocument { user_initiated: bool },
    /// The engine stepped back or forward, at Syrinx's request or the page's own.
    BackOrForward,
    /// The address changed inside the same document (a fragment, pushState or replaceState).
    SamePage,
    /// The history changed without a new address, such as a reload.
    Unchanged,
}

/// One engine report: the change, the address now shown, and where the engine can go.
#[derive(Clone, Eq, PartialEq)]
pub struct HistoryReport {
    pub change: HistoryChange,
    pub address: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

impl fmt::Debug for HistoryReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The address is the owner's browsing; only the structural facts are shown.
        formatter
            .debug_struct("HistoryReport")
            .field("change", &self.change)
            .field("can_go_back", &self.can_go_back)
            .field("can_go_forward", &self.can_go_forward)
            .finish_non_exhaustive()
    }
}

/// Which side of the current page a list or a jump is on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryDirection {
    Back,
    Forward,
}

/// Whether a report extended the list or replaced it with the current page alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryObserved {
    Followed,
    Reset,
}

/// One row of a back or forward list; `steps` is how many Back or Forward presses it is.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct HistoryRow<'a> {
    pub steps: usize,
    pub address: &'a str,
    pub title: &'a str,
}

impl fmt::Debug for HistoryRow<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryRow")
            .field("steps", &self.steps)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Default)]
struct Entry {
    address: String,
    title: String,
    /// The page moved on without a user gesture, so the engine's Back passes over it.
    hidden: bool,
}

impl Entry {
    fn at(address: &str) -> Self {
        Self {
            address: address.to_owned(),
            ..Self::default()
        }
    }
}

/// A best-effort copy of one tab's back and forward list (owner, 2026-10-28). The engine
/// cannot tell pushState from replaceState, so a same-page change replaces the current row.
/// Known limits: a `location.replace` after a user gesture adds a row the engine did not,
/// and a page hidden for moving on without a fresh gesture may still be one Back lands on;
/// a form posted back to the shown address adds an engine entry but no row; a jump across
/// any of these lands elsewhere and resets the list.
#[derive(Clone, Default)]
pub struct NavigationList {
    entries: Vec<Entry>,
    current: usize,
    /// The entry a requested jump should land on.
    jump: Option<usize>,
}

impl fmt::Debug for NavigationList {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Addresses and titles are the owner's browsing; never print them.
        formatter
            .debug_struct("NavigationList")
            .field("entries", &self.entries.len())
            .field("current", &self.current)
            .field("jump", &self.jump)
            .finish()
    }
}

impl NavigationList {
    /// Applies one engine report, then resets to the current page if the list and the
    /// engine disagree about whether anything lies behind or ahead.
    pub fn observe(&mut self, report: &HistoryReport) -> HistoryObserved {
        if self.entries.is_empty() {
            self.reset(&report.address);
        } else if !self.apply(report) {
            self.reset(&report.address);
            return HistoryObserved::Reset;
        }
        if self.agrees_with(report) {
            return HistoryObserved::Followed;
        }
        self.reset(&report.address);
        HistoryObserved::Reset
    }

    /// Names the current page.
    pub fn retitle(&mut self, title: &str) {
        if let Some(entry) = self.entries.get_mut(self.current) {
            title.clone_into(&mut entry.title);
        }
    }

    pub fn current_address(&self) -> Option<&str> {
        self.entries
            .get(self.current)
            .map(|entry| entry.address.as_str())
    }

    /// The pages in `direction`, nearest first, at most `limit`, without hidden pages.
    pub fn rows(&self, direction: HistoryDirection, limit: usize) -> Vec<HistoryRow<'_>> {
        self.visible(direction)
            .take(limit)
            .enumerate()
            .map(|(index, entry)| HistoryRow {
                steps: index + 1,
                address: &self.entries[entry].address,
                title: &self.entries[entry].title,
            })
            .collect()
    }

    /// Expects the engine to land on the row `steps` away; false when there is no such row.
    pub fn start_jump(&mut self, direction: HistoryDirection, steps: usize) -> bool {
        // A refused jump leaves no stale target for a later, unrelated step to fail against.
        self.jump = steps
            .checked_sub(1)
            .and_then(|skip| self.visible(direction).nth(skip));
        self.jump.is_some()
    }

    /// Whether any row lies behind and ahead.
    pub fn reach(&self) -> HistoryReach {
        HistoryReach {
            back: self.visible(HistoryDirection::Back).next().is_some(),
            forward: self.visible(HistoryDirection::Forward).next().is_some(),
        }
    }

    /// Indices of the visible entries in `direction`, nearest first.
    fn visible(&self, direction: HistoryDirection) -> impl Iterator<Item = usize> + '_ {
        let indices: Box<dyn Iterator<Item = usize>> = match direction {
            HistoryDirection::Back => Box::new((0..self.current).rev()),
            HistoryDirection::Forward => Box::new(self.current + 1..self.entries.len()),
        };
        indices.filter(|&index| !self.entries[index].hidden)
    }

    /// Applies a report to a non-empty list; false means the list lost track.
    fn apply(&mut self, report: &HistoryReport) -> bool {
        let address = report.address.as_str();
        match report.change {
            HistoryChange::NewDocument { user_initiated } => {
                self.push(address, user_initiated);
                true
            }
            HistoryChange::BackOrForward => self.land(address),
            // A jump can land on an entry the same document added, with no new document.
            HistoryChange::SamePage if self.jump.is_some() => self.land(address),
            HistoryChange::SamePage => {
                self.readdress(address);
                true
            }
            HistoryChange::Unchanged => {
                // A reload ends any jump it interrupted.
                self.jump = None;
                self.readdress(address);
                true
            }
        }
    }

    /// Gives the current row the address the engine shows now.
    fn readdress(&mut self, address: &str) {
        address.clone_into(&mut self.entries[self.current].address);
    }

    /// Adds a new page after the current one, dropping the pages ahead.
    fn push(&mut self, address: &str, user_initiated: bool) {
        self.jump = None;
        self.entries[self.current].hidden |= !user_initiated;
        self.entries.truncate(self.current + 1);
        self.entries.push(Entry::at(address));
        if self.entries.len() > HISTORY_ENTRY_LIMIT {
            self.entries.remove(0);
        }
        self.current = self.entries.len() - 1;
    }

    /// Moves to the page the engine stepped to: the requested jump's target, or else the
    /// nearest page with that address. False when neither matches, so the list resets.
    fn land(&mut self, address: &str) -> bool {
        let landed = match self.jump.take() {
            Some(target) => (self.entries[target].address == address).then_some(target),
            None => self.nearest(address),
        };
        let Some(landed) = landed else {
            return false;
        };
        self.current = landed;
        self.entries[landed].hidden = false;
        true
    }

    /// The entry with `address` closest to the current one, preferring the one behind.
    fn nearest(&self, address: &str) -> Option<usize> {
        let behind = (0..self.current).rev();
        let ahead = self.current + 1..self.entries.len();
        behind
            .chain(ahead)
            .filter(|&index| self.entries[index].address == address)
            .min_by_key(|&index| index.abs_diff(self.current))
    }

    /// Whether the list has entries behind and ahead exactly where the engine does.
    fn agrees_with(&self, report: &HistoryReport) -> bool {
        let has_back = self.current > 0;
        let has_forward = self.current + 1 < self.entries.len();
        has_back == report.can_go_back && has_forward == report.can_go_forward
    }

    /// Forgets everything but the page the engine shows now.
    fn reset(&mut self, address: &str) {
        self.entries = vec![Entry::at(address)];
        self.current = 0;
        self.jump = None;
    }
}

/// Whether a tab's list has a row behind it and a row ahead of it. It carries no address or
/// title, so it may travel to the interface freely.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HistoryReach {
    pub back: bool,
    pub forward: bool,
}

/// Every open tab's list, held in memory only: never saved, logged or restored.
#[derive(Debug, Default)]
pub struct NavigationLists(HashMap<TabId, NavigationList>);

impl NavigationLists {
    /// Applies a report to `tab`'s list, starting one if the tab has none yet.
    pub fn observe(&mut self, tab: TabId, report: &HistoryReport) -> HistoryObserved {
        self.0.entry(tab).or_default().observe(report)
    }

    /// Names the current row of `tab`'s list.
    pub fn retitle(&mut self, tab: TabId, title: &str) {
        if let Some(list) = self.0.get_mut(&tab) {
            list.retitle(title);
        }
    }

    pub fn get(&self, tab: TabId) -> Option<&NavigationList> {
        self.0.get(&tab)
    }

    pub fn get_mut(&mut self, tab: TabId) -> Option<&mut NavigationList> {
        self.0.get_mut(&tab)
    }

    /// Forgets a closed tab's list.
    pub fn remove(&mut self, tab: TabId) {
        self.0.remove(&tab);
    }
}

/// The tab model owns each open tab's list (owner, 2026-10-28); the interface only draws it.
impl TabModel {
    /// Follows an engine report for an open tab.
    pub fn observe_history(
        &mut self,
        id: TabId,
        report: &HistoryReport,
    ) -> Result<HistoryObserved, TabError> {
        self.index_of(id)?;
        Ok(self.navigation.observe(id, report))
    }

    /// Expects tab `id` to land on the row `steps` away; false when there is no such row.
    pub fn start_history_jump(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
        steps: usize,
    ) -> bool {
        self.navigation
            .get_mut(id)
            .is_some_and(|list| list.start_jump(direction, steps))
    }

    /// Tab `id`'s rows in `direction`, nearest first.
    pub fn history_rows(
        &self,
        id: TabId,
        direction: HistoryDirection,
        limit: usize,
    ) -> Vec<HistoryRow<'_>> {
        self.navigation
            .get(id)
            .map(|list| list.rows(direction, limit))
            .unwrap_or_default()
    }

    /// Whether tab `id` has rows behind and ahead; a tab with no list has neither.
    pub fn history_reach(&self, id: TabId) -> HistoryReach {
        self.navigation
            .get(id)
            .map(NavigationList::reach)
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "navigation_list_tests.rs"]
mod tests;

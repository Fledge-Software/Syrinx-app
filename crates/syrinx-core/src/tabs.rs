//! Keeps open tabs and persistent saved destinations in one sidebar and palette model.

mod folders;
mod import;
mod snapshot;

pub use folders::{
    FolderContents, FolderRemoval, RemovedSavedEntries, SavedEntryId, SavedFolder, SavedFolderId,
    SavedPlacement, saved_folder_contents,
};
pub(crate) use folders::{UNTITLED_FOLDER, is_blank_folder_title};
pub use import::{AppendedSavedTree, NewSavedItem};
pub use snapshot::{
    SavedFolderSnapshot, SavedPageSnapshot, SnapshotError, TabModelSnapshot, TabSnapshot,
};

use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroU64;

const CLOSED_TAB_LIMIT: usize = 20;

/// An immutable identity for one open tab, independent of its displayed position.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TabId(NonZeroU64);

impl TabId {
    /// Validates a numeric identity without creating or selecting a tab.
    pub fn new(value: u64) -> Result<Self, TabError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(TabError::InvalidTabId)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// A saved destination keeps its identity when its live tab is closed or replaced.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SavedPageId(NonZeroU64);

impl SavedPageId {
    /// Validates a numeric identity without creating a saved destination.
    pub fn new(value: u64) -> Result<Self, TabError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(TabError::InvalidSavedPageId)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// The complete tab vocabulary; plugin ownership and lifecycle arrive in plan 2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabKind {
    Web,
    PluginView,
    Note,
    File,
    Diff,
    Terminal,
}

/// Owned presentation values; interpreting an address belongs to its tab's host.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct TabMetadata {
    pub title: String,
    pub address: String,
    pub favicon: Option<String>,
    pub badge: Option<String>,
}

/// The address a new, blank tab holds until the owner navigates somewhere.
pub const BLANK_ADDRESS: &str = "about:blank";

/// How a web tab came to exist; an adopted popup is not shown as Home while it is blank.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageOrigin {
    Ordinary,
    AdoptedPopup,
}

impl PageOrigin {
    /// The origin for a tab that is, or is not, an adopted popup.
    pub fn from_adopted_popup(adopted: bool) -> Self {
        if adopted {
            Self::AdoptedPopup
        } else {
            Self::Ordinary
        }
    }
}

/// What every view calls a blank tab; spelled here and nowhere else.
const HOME_TITLE: &str = "Home";

/// The address of Syrinx's own Settings page (#91). No web view ever loads it, and the
/// address bar refuses the scheme, so only Syrinx itself opens a tab here.
pub const SETTINGS_ADDRESS: &str = "syrinx://settings";
const SETTINGS_TITLE: &str = "Settings";

impl TabMetadata {
    /// Whether this is a blank tab shown as Home; an adopted popup still awaiting its first
    /// address is not, so its row keeps the address.
    pub fn is_home(&self, origin: PageOrigin) -> bool {
        self.address == BLANK_ADDRESS && origin == PageOrigin::Ordinary
    }

    /// The name Home takes the place of the title with, if this tab is a blank one. Views
    /// that fall back to the address themselves (the palette) use this directly.
    pub fn home_title(&self, origin: PageOrigin) -> Option<&'static str> {
        self.is_home(origin).then_some(HOME_TITLE)
    }

    /// Whether this tab shows Syrinx's Settings page rather than a web page.
    pub fn is_settings(&self) -> bool {
        self.address == SETTINGS_ADDRESS
    }

    /// The name Syrinx gives its own pages, Home and Settings, in place of a stored title.
    pub fn syrinx_title(&self, origin: PageOrigin) -> Option<&'static str> {
        self.home_title(origin)
            .or_else(|| self.is_settings().then_some(SETTINGS_TITLE))
    }

    /// Whether this tab shows one of Syrinx's own pages: exactly those `syrinx_title` names, so
    /// a page added there is covered here too.
    pub fn is_syrinx_page(&self, origin: PageOrigin) -> bool {
        self.syrinx_title(origin).is_some()
    }

    /// The one name the sidebar shows for an open tab: Syrinx's name for its own pages,
    /// otherwise the title, or the address when the page has no title.
    pub fn display_title(&self, origin: PageOrigin) -> String {
        if let Some(name) = self.syrinx_title(origin) {
            return name.to_owned();
        }
        if self.title.is_empty() {
            return self.address.clone();
        }
        self.title.clone()
    }
}

impl fmt::Debug for TabMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never include browsing or document content in diagnostic formatting.
        formatter
            .debug_struct("TabMetadata")
            .finish_non_exhaustive()
    }
}

/// A tab's order is its position in the model, not a second mutable rank field.
#[derive(Clone, Eq, PartialEq)]
pub struct Tab {
    id: TabId,
    kind: TabKind,
    metadata: TabMetadata,
    saved_page_id: Option<SavedPageId>,
}

impl Tab {
    pub fn id(&self) -> TabId {
        self.id
    }

    pub fn kind(&self) -> TabKind {
        self.kind
    }

    pub fn metadata(&self) -> &TabMetadata {
        &self.metadata
    }

    pub fn saved_page_id(&self) -> Option<SavedPageId> {
        self.saved_page_id
    }
}

impl fmt::Debug for Tab {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // IDs and structural flags are safe; metadata is deliberately omitted.
        formatter
            .debug_struct("Tab")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("saved_page_id", &self.saved_page_id)
            .finish_non_exhaustive()
    }
}

/// The saved snapshot is independent of navigation in an associated live tab.
#[derive(Clone, Eq, PartialEq)]
pub struct SavedPage {
    id: SavedPageId,
    metadata: TabMetadata,
}

impl SavedPage {
    pub fn id(&self) -> SavedPageId {
        self.id
    }

    pub fn metadata(&self) -> &TabMetadata {
        &self.metadata
    }
}

impl fmt::Debug for SavedPage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Saved destinations receive the same diagnostic redaction as live tabs.
        formatter
            .debug_struct("SavedPage")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Each folder and saved page has one row, whether its associated live tab exists or not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidebarRow<'a> {
    Folder {
        folder: &'a SavedFolder,
        parent: Option<SavedFolderId>,
    },
    Saved {
        page: &'a SavedPage,
        parent: Option<SavedFolderId>,
        open_tab: Option<&'a Tab>,
    },
    Ordinary(&'a Tab),
}

/// The caller explicitly chooses focus when opening a tab.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Activation {
    Foreground,
    Background,
}

/// Failures contain identities and positions, never tab metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabError {
    InvalidTabId,
    InvalidSavedPageId,
    InvalidSavedFolderId,
    UnknownTab(TabId),
    UnknownSavedPage(SavedPageId),
    UnknownSavedFolder(SavedFolderId),
    FolderCycle {
        folder: SavedFolderId,
        parent: SavedFolderId,
    },
    NotWebTab(TabId),
    NotOrdinaryTab(TabId),
    InvalidPosition {
        index: usize,
        group_len: usize,
    },
    IdExhausted,
    SavedPageIdExhausted,
    SavedFolderIdExhausted,
    /// Folder names may not be empty or only spaces (owner decision, 2026-10-12).
    EmptyFolderTitle,
    /// A confirmed delete no longer matches what the owner was shown, so nothing is deleted.
    FolderContentsChanged(SavedFolderId),
}

impl fmt::Display for TabError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Describe rejected operations without echoing user-controlled strings.
        match self {
            Self::InvalidTabId => formatter.write_str("tab IDs must be nonzero"),
            Self::InvalidSavedPageId => formatter.write_str("saved page IDs must be nonzero"),
            Self::InvalidSavedFolderId => formatter.write_str("saved folder IDs must be nonzero"),
            Self::UnknownTab(id) => write!(formatter, "tab {} is not open", id.get()),
            Self::UnknownSavedPage(id) => {
                write!(formatter, "saved page {} does not exist", id.get())
            }
            Self::UnknownSavedFolder(id) => {
                write!(formatter, "saved folder {} does not exist", id.get())
            }
            Self::FolderCycle { folder, parent } => write!(
                formatter,
                "folder {} cannot move inside its own descendant {}",
                folder.get(),
                parent.get()
            ),
            Self::NotWebTab(id) => write!(formatter, "tab {} is not a web tab", id.get()),
            Self::NotOrdinaryTab(id) => {
                write!(formatter, "tab {} belongs to a saved page", id.get())
            }
            Self::InvalidPosition { index, group_len } => write!(
                formatter,
                "position {index} is outside a group of {group_len} rows"
            ),
            Self::IdExhausted => formatter.write_str("no unused tab IDs remain"),
            Self::SavedPageIdExhausted => formatter.write_str("no unused saved page IDs remain"),
            Self::SavedFolderIdExhausted => {
                formatter.write_str("no unused saved folder IDs remain")
            }
            Self::EmptyFolderTitle => formatter.write_str("folder names cannot be empty"),
            Self::FolderContentsChanged(id) => write!(
                formatter,
                "folder {} changed after its deletion was confirmed",
                id.get()
            ),
        }
    }
}

impl std::error::Error for TabError {}

struct ClosedTab {
    metadata: TabMetadata,
    saved_page_id: Option<SavedPageId>,
    ordinary_position: usize,
}

/// Ordered saved pages, open tabs and a bounded in-session history of closed web tabs.
/// `layout` is the saved tree; `saved` and `folders` hold its records in the same order.
#[derive(Default)]
pub struct TabModel {
    tabs: Vec<Tab>,
    saved: Vec<SavedPage>,
    folders: Vec<SavedFolder>,
    layout: Vec<SavedPlacement>,
    active: Option<TabId>,
    last_id: u64,
    last_saved_id: u64,
    last_folder_id: u64,
    closed: VecDeque<ClosedTab>,
}

impl TabModel {
    /// An empty model has no active tab; the shell decides how to render Home.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open saved tabs follow saved-row order, followed by the ordinary tab group.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn saved_pages(&self) -> &[SavedPage] {
        &self.saved
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn get_saved(&self, id: SavedPageId) -> Option<&SavedPage> {
        self.saved.iter().find(|page| page.id == id)
    }

    /// Merges the saved tree and the ordinary group without searching for each saved row.
    pub fn sidebar_rows(&self) -> impl Iterator<Item = SidebarRow<'_>> {
        // Pages, folders and the open saved prefix all follow layout order: a linear merge.
        let boundary = self.saved_tab_count();
        let mut open_saved = self.tabs[..boundary].iter().peekable();
        let mut pages = self.saved.iter();
        let mut folders = self.folders.iter();
        let saved_rows = self.layout.iter().filter_map(move |placement| {
            let parent = placement.parent;
            match placement.entry {
                SavedEntryId::Folder(_) => {
                    let folder = folders.next()?;
                    Some(SidebarRow::Folder { folder, parent })
                }
                SavedEntryId::Page(_) => {
                    let page = pages.next()?;
                    let open_tab = open_saved.next_if(|tab| tab.saved_page_id == Some(page.id));
                    Some(SidebarRow::Saved {
                        page,
                        parent,
                        open_tab,
                    })
                }
            }
        });
        saved_rows.chain(self.tabs[boundary..].iter().map(SidebarRow::Ordinary))
    }

    pub fn active_id(&self) -> Option<TabId> {
        self.active
    }

    pub fn closed_count(&self) -> usize {
        self.closed.len()
    }

    /// Appends an ordinary tab. The first open tab is always active.
    pub fn open(
        &mut self,
        kind: TabKind,
        metadata: TabMetadata,
        activation: Activation,
    ) -> Result<TabId, TabError> {
        // Allocate before changing any tab state so exhaustion is an atomic failure.
        let id = self.allocate_id()?;
        self.tabs.push(Tab {
            id,
            kind,
            metadata,
            saved_page_id: None,
        });
        self.activate_if_requested(id, activation);
        Ok(id)
    }

    /// Saves a web tab's current destination once and moves it to the saved group.
    pub fn save_page(&mut self, id: TabId) -> Result<SavedPageId, TabError> {
        // Validate before allocating or moving a row; saving twice is idempotent.
        let index = self.index_of(id)?;
        if self.tabs[index].kind != TabKind::Web {
            return Err(TabError::NotWebTab(id));
        }
        if let Some(saved_id) = self.tabs[index].saved_page_id {
            return Ok(saved_id);
        }
        let saved_id = self.allocate_saved_id()?;
        self.saved.push(SavedPage {
            id: saved_id,
            metadata: self.tabs[index].metadata.clone(),
        });
        self.layout.push(SavedPlacement {
            entry: SavedEntryId::Page(saved_id),
            parent: None,
        });

        // Only the tab owns the live association; the saved record holds no live ID.
        let mut tab = self.tabs.remove(index);
        tab.saved_page_id = Some(saved_id);
        self.tabs.insert(self.saved_tab_count(), tab);
        Ok(saved_id)
    }

    /// Reuses an open saved page, or opens a fresh tab from its saved destination.
    pub fn open_saved(
        &mut self,
        id: SavedPageId,
        activation: Activation,
    ) -> Result<TabId, TabError> {
        // An existing association must never produce a duplicate row or live instance.
        let saved_index = self.saved_index_of(id)?;
        if let Some(tab_index) = self.open_saved_index(id) {
            let tab_id = self.tabs[tab_index].id;
            self.activate_if_requested(tab_id, activation);
            return Ok(tab_id);
        }

        // Allocate before changing the model so a failed open preserves the saved row.
        let tab_id = self.allocate_id()?;
        let position = self.saved_tab_position(saved_index);
        self.tabs.insert(
            position,
            Tab {
                id: tab_id,
                kind: TabKind::Web,
                metadata: self.saved[saved_index].metadata.clone(),
                saved_page_id: Some(id),
            },
        );
        self.activate_if_requested(tab_id, activation);
        Ok(tab_id)
    }

    /// Removes the saved destination, leaving any open page at the ordinary start.
    pub fn remove_saved(&mut self, id: SavedPageId) -> Result<SavedPage, TabError> {
        // Pages have no children, so removing the placement removes only this row.
        let placement = self.placement_index(SavedEntryId::Page(id))?;
        self.layout.remove(placement);
        self.unlink_saved_page(id)
            .ok_or(TabError::UnknownSavedPage(id))
    }

    /// Deletes a saved record whose placement is already gone, keeping any live page open.
    fn unlink_saved_page(&mut self, id: SavedPageId) -> Option<SavedPage> {
        // Removing a saved row never closes its live tab or changes its identity.
        let saved_index = self.saved.iter().position(|page| page.id == id)?;
        let page = self.saved.remove(saved_index);
        if let Some(tab_index) = self.open_saved_index(id) {
            let mut tab = self.tabs.remove(tab_index);
            tab.saved_page_id = None;
            self.tabs.insert(self.saved_tab_count(), tab);
        }

        // Closed entries retain their live snapshot, but cannot resurrect the saved row.
        for closed in &mut self.closed {
            if closed.saved_page_id == Some(id) {
                closed.saved_page_id = None;
            }
        }
        Some(page)
    }

    pub fn select(&mut self, id: TabId) -> Result<(), TabError> {
        // Reject stale identities before changing the active selection.
        self.index_of(id)?;
        self.active = Some(id);
        Ok(())
    }

    pub fn update_metadata(&mut self, id: TabId, metadata: TabMetadata) -> Result<(), TabError> {
        // Live navigation cannot replace identity, saved destination or order.
        let index = self.index_of(id)?;
        self.tabs[index].metadata = metadata;
        Ok(())
    }

    /// Moves a saved row, including an unopened one, to a final position within its folder.
    pub fn move_saved(&mut self, id: SavedPageId, index: usize) -> Result<(), TabError> {
        let entry = SavedEntryId::Page(id);
        let parent = self.saved_parent(entry)?;
        self.move_saved_entry(entry, parent, index)
    }

    /// Moves an ordinary tab to a final zero-based position within its group.
    pub fn move_ordinary(&mut self, id: TabId, index: usize) -> Result<(), TabError> {
        // Check the complete destination before removing anything from the order.
        let current = self.index_of(id)?;
        if self.tabs[current].saved_page_id.is_some() {
            return Err(TabError::NotOrdinaryTab(id));
        }
        let start = self.saved_tab_count();
        Self::validate_position(index, self.tabs.len() - start)?;
        let destination = start + index;
        if current == destination {
            return Ok(());
        }
        let tab = self.tabs.remove(current);
        self.tabs.insert(destination, tab);
        Ok(())
    }

    /// Closes a live tab without removing a saved row. Only web tabs enter history.
    pub fn close(&mut self, id: TabId) -> Result<Tab, TabError> {
        // A removed saved association reopens at the ordinary start, like unpinning.
        let index = self.index_of(id)?;
        let ordinary_position = index.saturating_sub(self.saved_tab_count());
        let tab = self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.last())
                .map(Tab::id);
        }
        if tab.kind == TabKind::Web {
            self.remember_closed(&tab, ordinary_position);
        }
        Ok(tab)
    }

    /// Opens a selected copy of a web tab right after it; a saved page's copy is unsaved and
    /// starts the ordinary tabs.
    pub fn duplicate(&mut self, id: TabId) -> Result<TabId, TabError> {
        // Validate the source before opening anything, so a refusal changes nothing.
        let index = self.index_of(id)?;
        let source = &self.tabs[index];
        if source.kind != TabKind::Web {
            return Err(TabError::NotWebTab(id));
        }
        let position = match source.saved_page_id {
            Some(_) => 0,
            None => index - self.saved_tab_count() + 1,
        };
        let metadata = source.metadata.clone();

        // The copy is appended, so its final position always lies inside the ordinary group.
        let copy = self.open(TabKind::Web, metadata, Activation::Foreground)?;
        self.move_ordinary(copy, position)?;
        Ok(copy)
    }

    /// Closes every ordinary tab except `keep`, sparing open saved pages. `keep` is selected
    /// only when something closed; a no-op leaves the selection alone.
    pub fn close_others(&mut self, keep: TabId) -> Result<Vec<Tab>, TabError> {
        self.index_of(keep)?;
        let targets = self.ordinary_ids_from(0, keep);
        self.close_many(keep, &targets)
    }

    /// Closes the ordinary tabs after `id`, sparing open saved pages. `id` is selected only
    /// when something closed; a no-op leaves the selection alone.
    pub fn close_below(&mut self, id: TabId) -> Result<Vec<Tab>, TabError> {
        let index = self.index_of(id)?;
        let targets = self.ordinary_ids_from(index + 1, id);
        self.close_many(id, &targets)
    }

    /// The ordinary tabs from position `start` on, other than `keep`, in display order.
    fn ordinary_ids_from(&self, start: usize, keep: TabId) -> Vec<TabId> {
        let start = start.max(self.saved_tab_count());
        self.tabs[start..]
            .iter()
            .map(Tab::id)
            .filter(|id| *id != keep)
            .collect()
    }

    /// Closes `targets` bottom-up, so reopening restores them top-down in their old positions.
    /// Returns the closed tabs in display order and selects `keep` if any closed. The callers
    /// have validated `keep` and read `targets` from the model, so every close succeeds.
    fn close_many(&mut self, keep: TabId, targets: &[TabId]) -> Result<Vec<Tab>, TabError> {
        let mut closed = Vec::with_capacity(targets.len());
        for id in targets.iter().rev() {
            closed.push(self.close(*id)?);
        }
        closed.reverse();
        if !closed.is_empty() {
            self.active = Some(keep);
        }
        Ok(closed)
    }

    /// Selects a saved instance or restores the newest closed ordinary web snapshot.
    pub fn reopen_closed(&mut self) -> Result<Option<TabId>, TabError> {
        // A saved row still owns its destination, even after its previous tab navigated.
        let Some(closed) = self.closed.back() else {
            return Ok(None);
        };
        if let Some(saved_id) = closed.saved_page_id {
            let id = self.open_saved(saved_id, Activation::Foreground)?;
            drop(self.closed.pop_back());
            return Ok(Some(id));
        }

        // Keep history unchanged on allocation failure, then restore a fresh live identity.
        let metadata = closed.metadata.clone();
        let start = self.saved_tab_count();
        let index = start + closed.ordinary_position.min(self.tabs.len() - start);
        let id = self.allocate_id()?;
        self.tabs.insert(
            index,
            Tab {
                id,
                kind: TabKind::Web,
                metadata,
                saved_page_id: None,
            },
        );
        self.active = Some(id);
        drop(self.closed.pop_back());
        Ok(Some(id))
    }

    fn index_of(&self, id: TabId) -> Result<usize, TabError> {
        self.tabs
            .iter()
            .position(|tab| tab.id == id)
            .ok_or(TabError::UnknownTab(id))
    }

    fn saved_index_of(&self, id: SavedPageId) -> Result<usize, TabError> {
        self.saved
            .iter()
            .position(|page| page.id == id)
            .ok_or(TabError::UnknownSavedPage(id))
    }

    fn open_saved_index(&self, id: SavedPageId) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.saved_page_id == Some(id))
    }

    fn saved_tab_count(&self) -> usize {
        self.tabs.partition_point(|tab| tab.saved_page_id.is_some())
    }

    fn saved_tab_position(&self, saved_index: usize) -> usize {
        // Merge the saved prefix once; unopened rows contribute no live position.
        let mut position = 0;
        for page in &self.saved[..saved_index] {
            if self
                .tabs
                .get(position)
                .is_some_and(|tab| tab.saved_page_id == Some(page.id))
            {
                position += 1;
            }
        }
        position
    }

    fn validate_position(index: usize, group_len: usize) -> Result<(), TabError> {
        if index >= group_len {
            return Err(TabError::InvalidPosition { index, group_len });
        }
        Ok(())
    }

    fn activate_if_requested(&mut self, id: TabId, activation: Activation) {
        if self.active.is_none() || activation == Activation::Foreground {
            self.active = Some(id);
        }
    }

    fn allocate_id(&mut self) -> Result<TabId, TabError> {
        // Never recycle IDs: callbacks for closed views must remain stale.
        let next = self.last_id.checked_add(1).ok_or(TabError::IdExhausted)?;
        let id = TabId::new(next)?;
        self.last_id = next;
        Ok(id)
    }

    fn allocate_saved_id(&mut self) -> Result<SavedPageId, TabError> {
        // A removed saved identity stays retired, just like a closed live identity.
        let next = self
            .last_saved_id
            .checked_add(1)
            .ok_or(TabError::SavedPageIdExhausted)?;
        let id = SavedPageId::new(next)?;
        self.last_saved_id = next;
        Ok(id)
    }

    fn remember_closed(&mut self, tab: &Tab, ordinary_position: usize) {
        // Retain only the twenty most recently closed web tabs until this model is dropped.
        self.closed.push_back(ClosedTab {
            metadata: tab.metadata.clone(),
            saved_page_id: tab.saved_page_id,
            ordinary_position,
        });
        if self.closed.len() > CLOSED_TAB_LIMIT {
            drop(self.closed.pop_front());
        }
    }
}

impl fmt::Debug for TabModel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Aggregate structure is enough for diagnostics; open and closed content stays private.
        formatter
            .debug_struct("TabModel")
            .field("open_count", &self.tabs.len())
            .field("saved_count", &self.saved.len())
            .field("folder_count", &self.folders.len())
            .field("active", &self.active)
            .field("closed_count", &self.closed.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Activation, BLANK_ADDRESS, PageOrigin, SETTINGS_ADDRESS, TabError, TabKind, TabMetadata,
        TabModel,
    };

    #[test]
    fn a_blank_tab_is_called_home_and_other_tabs_use_title_then_address() {
        let metadata = |title: &str, address: &str| TabMetadata {
            title: title.to_owned(),
            address: address.to_owned(),
            ..TabMetadata::default()
        };
        assert_eq!(
            metadata("", BLANK_ADDRESS).display_title(PageOrigin::Ordinary),
            "Home"
        );
        assert_eq!(
            metadata("", BLANK_ADDRESS).display_title(PageOrigin::AdoptedPopup),
            BLANK_ADDRESS
        );
        assert_eq!(
            metadata("Example", "https://example.test/").display_title(PageOrigin::Ordinary),
            "Example"
        );
        assert_eq!(
            metadata("", "https://example.test/").display_title(PageOrigin::Ordinary),
            "https://example.test/"
        );
    }

    #[test]
    fn the_settings_address_is_called_settings_whatever_its_stored_title() {
        let settings = TabMetadata {
            title: SETTINGS_ADDRESS.to_owned(),
            address: SETTINGS_ADDRESS.to_owned(),
            ..TabMetadata::default()
        };
        assert!(settings.is_settings());
        assert!(!settings.is_home(PageOrigin::Ordinary));
        assert_eq!(settings.display_title(PageOrigin::Ordinary), "Settings");
        assert_eq!(
            settings.syrinx_title(PageOrigin::Ordinary),
            Some("Settings")
        );
        let home = TabMetadata {
            address: BLANK_ADDRESS.to_owned(),
            ..TabMetadata::default()
        };
        assert!(!home.is_settings());
        assert_eq!(home.syrinx_title(PageOrigin::Ordinary), Some("Home"));
        // A web page that merely mentions the address is still a web page.
        let page = TabMetadata {
            title: "Settings".to_owned(),
            address: "https://example.test/syrinx://settings".to_owned(),
            ..TabMetadata::default()
        };
        assert!(!page.is_settings());
        assert_eq!(page.syrinx_title(PageOrigin::Ordinary), None);
        // Syrinx's own pages are exactly the ones it names.
        assert!(settings.is_syrinx_page(PageOrigin::Ordinary));
        assert!(home.is_syrinx_page(PageOrigin::Ordinary));
        assert!(!home.is_syrinx_page(PageOrigin::AdoptedPopup));
        assert!(!page.is_syrinx_page(PageOrigin::Ordinary));
    }

    #[test]
    fn live_id_exhaustion_preserves_open_state() -> Result<(), TabError> {
        // Exercise the last valid identity without a test-only public constructor.
        let mut model = TabModel {
            last_id: u64::MAX - 1,
            ..TabModel::new()
        };
        let id = model.open(TabKind::Web, TabMetadata::default(), Activation::Foreground)?;
        assert_eq!(id.get(), u64::MAX);
        let before_tabs = model.tabs.clone();

        assert_eq!(
            model.open(TabKind::Web, TabMetadata::default(), Activation::Foreground),
            Err(TabError::IdExhausted)
        );
        assert_eq!(model.tabs, before_tabs);
        assert_eq!(model.active, Some(id));
        assert_eq!(model.last_id, u64::MAX);
        assert!(model.saved.is_empty());
        assert!(model.closed.is_empty());
        Ok(())
    }

    #[test]
    fn saved_id_exhaustion_preserves_order_and_associations() -> Result<(), TabError> {
        // Exhausting durable identities must leave the unsaved live tab untouched.
        let mut model = TabModel {
            last_saved_id: u64::MAX - 1,
            ..TabModel::new()
        };
        let first = model.open(TabKind::Web, TabMetadata::default(), Activation::Foreground)?;
        let second = model.open(TabKind::Web, TabMetadata::default(), Activation::Foreground)?;
        let saved_id = model.save_page(first)?;
        assert_eq!(saved_id.get(), u64::MAX);
        let before_tabs = model.tabs.clone();
        let before_saved = model.saved.clone();

        assert_eq!(model.save_page(second), Err(TabError::SavedPageIdExhausted));
        assert_eq!(model.save_page(first), Ok(saved_id));
        assert_eq!(model.tabs, before_tabs);
        assert_eq!(model.saved, before_saved);
        assert_eq!(model.active, Some(second));
        assert_eq!(model.last_saved_id, u64::MAX);
        assert!(model.closed.is_empty());
        Ok(())
    }

    #[test]
    fn failed_ordinary_reopen_preserves_history_and_selection() -> Result<(), TabError> {
        // Keep a different open tab active while the closed snapshot cannot be restored.
        let mut model = TabModel::new();
        let closed = model.open(TabKind::Web, TabMetadata::default(), Activation::Foreground)?;
        let remaining = model.open(
            TabKind::Note,
            TabMetadata::default(),
            Activation::Background,
        )?;
        drop(model.close(closed)?);
        model.last_id = u64::MAX;
        let before_tabs = model.tabs.clone();

        assert_eq!(model.reopen_closed(), Err(TabError::IdExhausted));
        assert_eq!(model.tabs, before_tabs);
        assert_eq!(model.active, Some(remaining));
        assert_eq!(model.closed.len(), 1);
        assert_eq!(model.closed[0].metadata, TabMetadata::default());
        assert_eq!(model.closed[0].saved_page_id, None);
        assert_eq!(model.closed[0].ordinary_position, 0);
        assert_eq!(model.last_id, u64::MAX);

        // The retained entry remains usable when a live identity is available.
        model.last_id = u64::MAX - 1;
        let reopened = model.reopen_closed()?;
        assert_eq!(reopened.map(|id| id.get()), Some(u64::MAX));
        assert!(model.closed.is_empty());
        Ok(())
    }

    #[test]
    fn failed_saved_reopen_preserves_history_and_can_reuse_a_live_instance() -> Result<(), TabError>
    {
        // A failed saved open must preserve both the destination and the history entry.
        let mut model = TabModel::new();
        let tab = model.open(TabKind::Web, TabMetadata::default(), Activation::Foreground)?;
        let saved_id = model.save_page(tab)?;
        drop(model.close(tab)?);
        model.last_id = u64::MAX;
        let before_saved = model.saved.clone();

        assert_eq!(model.reopen_closed(), Err(TabError::IdExhausted));
        assert!(model.tabs.is_empty());
        assert_eq!(model.saved, before_saved);
        assert_eq!(model.active, None);
        assert_eq!(model.closed.len(), 1);
        assert_eq!(model.closed[0].saved_page_id, Some(saved_id));
        assert_eq!(model.last_id, u64::MAX);

        // Reopening through the saved row first lets history reuse the last live identity.
        model.last_id = u64::MAX - 1;
        let live = model.open_saved(saved_id, Activation::Foreground)?;
        assert_eq!(live.get(), u64::MAX);
        assert_eq!(model.reopen_closed(), Ok(Some(live)));
        assert_eq!(model.tabs.len(), 1);
        assert_eq!(model.saved, before_saved);
        assert!(model.closed.is_empty());
        Ok(())
    }
}

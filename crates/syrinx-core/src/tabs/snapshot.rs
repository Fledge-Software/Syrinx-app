//! Validates owned persistence records before restoring a new tab model.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use super::{
    PageOrigin, SavedEntryId, SavedFolder, SavedFolderId, SavedPage, SavedPageId, SavedPlacement,
    Tab, TabId, TabKind, TabMetadata, TabModel,
};

/// One live session tab; metadata is independent of its optional saved destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabSnapshot {
    pub id: TabId,
    pub kind: TabKind,
    pub metadata: TabMetadata,
    pub saved_page_id: Option<SavedPageId>,
}

/// One saved destination, ordered by its position in the snapshot's saved vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedPageSnapshot {
    pub id: SavedPageId,
    pub metadata: TabMetadata,
}

/// One inline folder, ordered by its position in the snapshot's folder vector.
#[derive(Clone, Eq, PartialEq)]
pub struct SavedFolderSnapshot {
    pub id: SavedFolderId,
    pub title: String,
}

impl fmt::Debug for SavedFolderSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Folder names are owner content; only the identity is diagnostic.
        formatter
            .debug_struct("SavedFolderSnapshot")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Durable model state; closed-tab history remains in memory for this run only.
/// `saved_layout` lists every page and folder depth-first; `saved_pages` and
/// `saved_folders` hold their records in that same order.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct TabModelSnapshot {
    pub tabs: Vec<TabSnapshot>,
    pub saved_pages: Vec<SavedPageSnapshot>,
    pub saved_folders: Vec<SavedFolderSnapshot>,
    pub saved_layout: Vec<SavedPlacement>,
    pub active_id: Option<TabId>,
    pub last_tab_id: u64,
    pub last_saved_page_id: u64,
    pub last_saved_folder_id: u64,
}

impl fmt::Debug for TabModelSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Snapshot diagnostics expose structure, never saved or live metadata.
        formatter
            .debug_struct("TabModelSnapshot")
            .field("tab_count", &self.tabs.len())
            .field("saved_page_count", &self.saved_pages.len())
            .field("saved_folder_count", &self.saved_folders.len())
            .field("active_id", &self.active_id)
            .field("last_tab_id", &self.last_tab_id)
            .field("last_saved_page_id", &self.last_saved_page_id)
            .field("last_saved_folder_id", &self.last_saved_folder_id)
            .finish_non_exhaustive()
    }
}

/// Invalid snapshots are rejected without repairing, dropping or logging their contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    DuplicateTabId(TabId),
    DuplicateSavedPageId(SavedPageId),
    DuplicateSavedFolderId(SavedFolderId),
    TabIdAboveWatermark(TabId),
    SavedPageIdAboveWatermark(SavedPageId),
    SavedFolderIdAboveWatermark(SavedFolderId),
    SavedLayoutMismatch(SavedEntryId),
    SavedLayoutIncomplete,
    InvalidSavedParent(SavedEntryId),
    MissingActiveTab,
    UnknownActiveTab(TabId),
    UnknownSavedPage(SavedPageId),
    NonWebSavedAssociation(TabId),
    DuplicateSavedAssociation(SavedPageId),
    InvalidSavedPrefixOrder(TabId),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Describe structural faults without including title, address, icon or badge values.
        match self {
            Self::DuplicateTabId(id) => write!(formatter, "duplicate tab ID {}", id.get()),
            Self::DuplicateSavedPageId(id) => {
                write!(formatter, "duplicate saved page ID {}", id.get())
            }
            Self::DuplicateSavedFolderId(id) => {
                write!(formatter, "duplicate saved folder ID {}", id.get())
            }
            Self::TabIdAboveWatermark(id) => {
                write!(formatter, "tab {} exceeds the tab ID watermark", id.get())
            }
            Self::SavedPageIdAboveWatermark(id) => {
                write!(
                    formatter,
                    "saved page {} exceeds its ID watermark",
                    id.get()
                )
            }
            Self::SavedFolderIdAboveWatermark(id) => {
                write!(
                    formatter,
                    "saved folder {} exceeds its ID watermark",
                    id.get()
                )
            }
            Self::SavedLayoutMismatch(entry) => {
                write!(formatter, "saved layout entry {entry:?} is out of order")
            }
            Self::SavedLayoutIncomplete => {
                formatter.write_str("a saved page or folder has no layout entry")
            }
            Self::InvalidSavedParent(entry) => write!(
                formatter,
                "saved layout entry {entry:?} is not inside its parent folder"
            ),
            Self::MissingActiveTab => formatter.write_str("nonempty session has no active tab"),
            Self::UnknownActiveTab(id) => {
                write!(
                    formatter,
                    "active tab {} is absent from the session",
                    id.get()
                )
            }
            Self::UnknownSavedPage(id) => {
                write!(formatter, "saved association {} has no saved row", id.get())
            }
            Self::NonWebSavedAssociation(id) => {
                write!(
                    formatter,
                    "non-web tab {} has a saved association",
                    id.get()
                )
            }
            Self::DuplicateSavedAssociation(id) => {
                write!(formatter, "saved page {} has multiple live tabs", id.get())
            }
            Self::InvalidSavedPrefixOrder(id) => {
                write!(formatter, "tab {} is outside saved-row order", id.get())
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

impl TabModelSnapshot {
    /// Whether the active tab shows Syrinx's Settings page (#91).
    pub fn active_is_settings(&self) -> bool {
        self.active_metadata()
            .is_some_and(|metadata| metadata.is_settings())
    }

    /// Whether the active tab shows one of Syrinx's own pages, Home or Settings, which are not
    /// destinations and so are never saved (#91). A blank adopted popup counts too: it has
    /// nothing to save either.
    pub fn active_is_syrinx_page(&self) -> bool {
        self.active_metadata()
            .is_some_and(|metadata| metadata.is_syrinx_page(PageOrigin::Ordinary))
    }

    /// The active tab's metadata, if a tab is active.
    fn active_metadata(&self) -> Option<&TabMetadata> {
        let active_id = self.active_id?;
        self.tabs
            .iter()
            .find(|tab| tab.id == active_id)
            .map(|tab| &tab.metadata)
    }

    /// Checks the complete model contract before a store writes or restores these records.
    /// Watermarks cover this snapshot; comparing older writes is the writer's responsibility.
    pub fn validate(&self) -> Result<(), SnapshotError> {
        // Resolve saved identities once, then validate the tree, live rows and selection.
        let saved_positions = self.saved_positions()?;
        self.validate_folder_ids()?;
        self.validate_layout()?;
        let tab_ids = self.validate_tabs(&saved_positions)?;
        match self.active_id {
            Some(id) if !tab_ids.contains(&id) => Err(SnapshotError::UnknownActiveTab(id)),
            None if !tab_ids.is_empty() => Err(SnapshotError::MissingActiveTab),
            _ => Ok(()),
        }
    }

    fn saved_positions(&self) -> Result<BTreeMap<SavedPageId, usize>, SnapshotError> {
        // Preserve durable identities, including when their live pages are unloaded.
        let mut positions = BTreeMap::new();
        for (position, page) in self.saved_pages.iter().enumerate() {
            if page.id.get() > self.last_saved_page_id {
                return Err(SnapshotError::SavedPageIdAboveWatermark(page.id));
            }
            if positions.insert(page.id, position).is_some() {
                return Err(SnapshotError::DuplicateSavedPageId(page.id));
            }
        }
        Ok(positions)
    }

    fn validate_folder_ids(&self) -> Result<(), SnapshotError> {
        let mut ids = BTreeSet::new();
        for folder in &self.saved_folders {
            if folder.id.get() > self.last_saved_folder_id {
                return Err(SnapshotError::SavedFolderIdAboveWatermark(folder.id));
            }
            if !ids.insert(folder.id) {
                return Err(SnapshotError::DuplicateSavedFolderId(folder.id));
            }
        }
        Ok(())
    }

    fn validate_layout(&self) -> Result<(), SnapshotError> {
        // Records must appear in layout order, so each identity is placed exactly once.
        let mut pages = self.saved_pages.iter();
        let mut folders = self.saved_folders.iter();
        let mut open_folders: Vec<SavedFolderId> = Vec::new();
        for placement in &self.saved_layout {
            let matches_record = match placement.entry {
                SavedEntryId::Page(id) => pages.next().is_some_and(|page| page.id == id),
                SavedEntryId::Folder(id) => folders.next().is_some_and(|folder| folder.id == id),
            };
            if !matches_record {
                return Err(SnapshotError::SavedLayoutMismatch(placement.entry));
            }

            // Depth-first order: a parent is an enclosing folder that appeared earlier.
            Self::enter_parent(&mut open_folders, placement)?;
            if let SavedEntryId::Folder(id) = placement.entry {
                open_folders.push(id);
            }
        }
        if pages.next().is_some() || folders.next().is_some() {
            return Err(SnapshotError::SavedLayoutIncomplete);
        }
        Ok(())
    }

    fn enter_parent(
        open_folders: &mut Vec<SavedFolderId>,
        placement: &SavedPlacement,
    ) -> Result<(), SnapshotError> {
        // Close folders until the parent is innermost; an unknown parent closes them all.
        let Some(parent) = placement.parent else {
            open_folders.clear();
            return Ok(());
        };
        while let Some(&innermost) = open_folders.last() {
            if innermost == parent {
                return Ok(());
            }
            open_folders.pop();
        }
        Err(SnapshotError::InvalidSavedParent(placement.entry))
    }

    fn validate_tabs(
        &self,
        saved_positions: &BTreeMap<SavedPageId, usize>,
    ) -> Result<BTreeSet<TabId>, SnapshotError> {
        // An open saved prefix must follow saved-row order, skipping unloaded rows.
        let mut ids = BTreeSet::new();
        let mut associations = BTreeSet::new();
        let mut previous_saved_position = None;
        let mut ordinary_started = false;
        for tab in &self.tabs {
            if tab.id.get() > self.last_tab_id {
                return Err(SnapshotError::TabIdAboveWatermark(tab.id));
            }
            if !ids.insert(tab.id) {
                return Err(SnapshotError::DuplicateTabId(tab.id));
            }
            let Some(saved_id) = tab.saved_page_id else {
                ordinary_started = true;
                continue;
            };
            if tab.kind != TabKind::Web {
                return Err(SnapshotError::NonWebSavedAssociation(tab.id));
            }
            let position = *saved_positions
                .get(&saved_id)
                .ok_or(SnapshotError::UnknownSavedPage(saved_id))?;
            if !associations.insert(saved_id) {
                return Err(SnapshotError::DuplicateSavedAssociation(saved_id));
            }
            if ordinary_started
                || previous_saved_position.is_some_and(|previous| position <= previous)
            {
                return Err(SnapshotError::InvalidSavedPrefixOrder(tab.id));
            }
            previous_saved_position = Some(position);
        }
        Ok(ids)
    }
}

impl TabModel {
    /// Captures owned persistence records without serializing in-session closed history.
    pub fn snapshot(&self) -> TabModelSnapshot {
        // Keep live and saved metadata separate, including their independent ordering.
        TabModelSnapshot {
            tabs: self
                .tabs
                .iter()
                .map(|tab| TabSnapshot {
                    id: tab.id,
                    kind: tab.kind,
                    metadata: tab.metadata.clone(),
                    saved_page_id: tab.saved_page_id,
                })
                .collect(),
            saved_pages: self
                .saved
                .iter()
                .map(|page| SavedPageSnapshot {
                    id: page.id,
                    metadata: page.metadata.clone(),
                })
                .collect(),
            saved_folders: self
                .folders
                .iter()
                .map(|folder| SavedFolderSnapshot {
                    id: folder.id,
                    title: folder.title.clone(),
                })
                .collect(),
            saved_layout: self.layout.clone(),
            active_id: self.active,
            last_tab_id: self.last_id,
            last_saved_page_id: self.last_saved_id,
            last_saved_folder_id: self.last_folder_id,
        }
    }

    /// Restores a new model with the persisted logical IDs and allocation watermarks.
    /// Use at startup, not to replace a running model with outstanding page callbacks.
    pub fn from_snapshot(snapshot: TabModelSnapshot) -> Result<Self, SnapshotError> {
        // Validate everything before constructing a model; never silently repair records.
        snapshot.validate()?;
        Ok(Self {
            tabs: snapshot
                .tabs
                .into_iter()
                .map(|tab| Tab {
                    id: tab.id,
                    kind: tab.kind,
                    metadata: tab.metadata,
                    saved_page_id: tab.saved_page_id,
                })
                .collect(),
            saved: snapshot
                .saved_pages
                .into_iter()
                .map(|page| SavedPage {
                    id: page.id,
                    metadata: page.metadata,
                })
                .collect(),
            folders: snapshot
                .saved_folders
                .into_iter()
                .map(|folder| SavedFolder {
                    id: folder.id,
                    title: folder.title,
                })
                .collect(),
            layout: snapshot.saved_layout,
            active: snapshot.active_id,
            last_id: snapshot.last_tab_id,
            last_saved_id: snapshot.last_saved_page_id,
            last_folder_id: snapshot.last_saved_folder_id,
            closed: VecDeque::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{SavedPageSnapshot, SnapshotError, TabModelSnapshot, TabSnapshot};
    use crate::{
        Activation, BLANK_ADDRESS, SETTINGS_ADDRESS, SavedPageId, TabError, TabId, TabKind,
        TabMetadata, TabModel,
    };

    #[test]
    fn only_an_active_settings_tab_counts_as_settings_showing() {
        let open = |model: &mut TabModel, address: &str, activation| {
            model
                .open(
                    TabKind::Web,
                    TabMetadata {
                        address: address.to_owned(),
                        ..TabMetadata::default()
                    },
                    activation,
                )
                .expect("open synthetic tab")
        };
        let mut model = TabModel::new();
        assert!(!model.snapshot().active_is_settings());
        let settings = open(&mut model, SETTINGS_ADDRESS, Activation::Foreground);
        assert!(model.snapshot().active_is_settings());
        open(&mut model, "https://example.test/", Activation::Foreground);
        assert!(!model.snapshot().active_is_settings());
        model.select(settings).expect("select Settings");
        assert!(model.snapshot().active_is_settings());
    }

    #[test]
    fn only_an_active_home_or_settings_tab_counts_as_a_syrinx_page() {
        let open = |model: &mut TabModel, address: &str| {
            model
                .open(
                    TabKind::Web,
                    TabMetadata {
                        address: address.to_owned(),
                        ..TabMetadata::default()
                    },
                    Activation::Foreground,
                )
                .expect("open synthetic tab")
        };
        let mut model = TabModel::new();
        assert!(!model.snapshot().active_is_syrinx_page());
        let home = open(&mut model, BLANK_ADDRESS);
        assert!(model.snapshot().active_is_syrinx_page());
        let settings = open(&mut model, SETTINGS_ADDRESS);
        assert!(model.snapshot().active_is_syrinx_page());
        open(&mut model, "https://example.test/");
        assert!(!model.snapshot().active_is_syrinx_page());
        for own_page in [home, settings] {
            model.select(own_page).expect("select Syrinx's own page");
            assert!(model.snapshot().active_is_syrinx_page());
        }
    }

    fn metadata(number: u64) -> TabMetadata {
        TabMetadata {
            title: format!("Synthetic snapshot {number} — café"),
            address: format!("https://snapshot-{number}.test/"),
            favicon: Some(format!("https://snapshot-{number}.test/icon.svg")),
            badge: Some(format!("synthetic-badge-{number}")),
        }
    }

    fn open(model: &mut TabModel, number: u64) -> TabId {
        model
            .open(TabKind::Web, metadata(number), Activation::Foreground)
            .expect("synthetic tab allocation succeeds")
    }

    fn fixture() -> TabModelSnapshot {
        // Include unloaded saved rows, changed live metadata and ordinary tabs.
        let mut model = TabModel::new();
        let first = open(&mut model, 1);
        let unloaded = open(&mut model, 2);
        let third = open(&mut model, 3);
        open(&mut model, 4);
        for id in [first, unloaded, third] {
            model.save_page(id).expect("synthetic page saves");
        }
        model.close(unloaded).expect("synthetic tab closes");
        model
            .update_metadata(first, metadata(10))
            .expect("synthetic navigation succeeds");
        model.select(third).expect("synthetic tab exists");
        model.snapshot()
    }

    #[test]
    fn restores_all_records_selection_and_order_without_closed_history() {
        let snapshot = fixture();
        let restored = TabModel::from_snapshot(snapshot.clone()).expect("valid snapshot restores");
        assert_eq!(restored.snapshot(), snapshot);
        assert_eq!(restored.closed_count(), 0);
        assert_eq!(restored.saved_pages().len(), 3);
        assert_eq!(restored.tabs().len(), 3);
        assert_eq!(restored.tabs()[0].metadata(), &metadata(10));
        assert_eq!(restored.saved_pages()[0].metadata(), &metadata(1));
    }

    #[test]
    fn restored_watermarks_retire_closed_and_removed_identities() {
        let mut original = TabModel::new();
        let retired_tab = open(&mut original, 1);
        let retired_saved = original
            .save_page(retired_tab)
            .expect("synthetic page saves");
        original.close(retired_tab).expect("synthetic tab closes");
        original
            .remove_saved(retired_saved)
            .expect("synthetic row exists");
        assert_eq!(original.closed_count(), 1);

        let mut restored =
            TabModel::from_snapshot(original.snapshot()).expect("empty state restores");
        assert_eq!(restored.active_id(), None);
        assert_eq!(restored.reopen_closed(), Ok(None));
        let fresh = open(&mut restored, 2);
        let saved = restored.save_page(fresh).expect("fresh page saves");
        assert!(fresh.get() > retired_tab.get());
        assert!(saved.get() > retired_saved.get());
    }

    #[test]
    fn full_u64_watermarks_restore_without_wrapping_or_reusing_ids() {
        let saved_tab = TabId::new(u64::MAX - 1).expect("synthetic ID is nonzero");
        let ordinary_tab = TabId::new(u64::MAX).expect("synthetic ID is nonzero");
        let saved_page = SavedPageId::new(u64::MAX).expect("synthetic ID is nonzero");
        let snapshot = TabModelSnapshot {
            tabs: vec![
                TabSnapshot {
                    id: saved_tab,
                    kind: TabKind::Web,
                    metadata: metadata(1),
                    saved_page_id: Some(saved_page),
                },
                TabSnapshot {
                    id: ordinary_tab,
                    kind: TabKind::Web,
                    metadata: metadata(2),
                    saved_page_id: None,
                },
            ],
            saved_pages: vec![SavedPageSnapshot {
                id: saved_page,
                metadata: metadata(1),
            }],
            saved_folders: Vec::new(),
            saved_layout: vec![crate::SavedPlacement {
                entry: crate::SavedEntryId::Page(saved_page),
                parent: None,
            }],
            active_id: Some(ordinary_tab),
            last_tab_id: u64::MAX,
            last_saved_page_id: u64::MAX,
            last_saved_folder_id: u64::MAX,
        };
        let mut restored =
            TabModel::from_snapshot(snapshot.clone()).expect("exhausted state is valid");
        assert_eq!(
            restored.open(TabKind::Web, metadata(1), Activation::Foreground),
            Err(TabError::IdExhausted)
        );
        assert_eq!(
            restored.save_page(ordinary_tab),
            Err(TabError::SavedPageIdExhausted)
        );
        assert_eq!(restored.snapshot(), snapshot);
    }

    #[test]
    fn invalid_identities_and_watermarks_are_rejected_without_repair() {
        let original = fixture();
        let mut cases = Vec::new();
        let mut duplicate_tab = original.clone();
        duplicate_tab.tabs[1].id = duplicate_tab.tabs[0].id;
        cases.push((
            duplicate_tab,
            SnapshotError::DuplicateTabId(original.tabs[0].id),
        ));
        let mut duplicate_saved = original.clone();
        duplicate_saved.saved_pages[1].id = duplicate_saved.saved_pages[0].id;
        cases.push((
            duplicate_saved,
            SnapshotError::DuplicateSavedPageId(original.saved_pages[0].id),
        ));
        let mut regressed_tabs = original.clone();
        regressed_tabs.last_tab_id = 0;
        cases.push((
            regressed_tabs,
            SnapshotError::TabIdAboveWatermark(original.tabs[0].id),
        ));
        let mut regressed_saved = original.clone();
        regressed_saved.last_saved_page_id = 0;
        cases.push((
            regressed_saved,
            SnapshotError::SavedPageIdAboveWatermark(original.saved_pages[0].id),
        ));

        for (snapshot, error) in cases {
            assert_eq!(snapshot.validate(), Err(error));
            assert_eq!(TabModel::from_snapshot(snapshot).err(), Some(error));
        }
    }

    #[test]
    fn invalid_selection_associations_and_saved_order_are_rejected() {
        let original = fixture();
        let mut cases = Vec::new();
        let mut no_active = original.clone();
        no_active.active_id = None;
        cases.push((no_active, SnapshotError::MissingActiveTab));
        let mut missing_active = original.clone();
        let unknown_tab = TabId::new(99).expect("synthetic ID is nonzero");
        missing_active.active_id = Some(unknown_tab);
        cases.push((missing_active, SnapshotError::UnknownActiveTab(unknown_tab)));
        let mut dangling = original.clone();
        let unknown_saved = SavedPageId::new(99).expect("synthetic ID is nonzero");
        dangling.tabs[0].saved_page_id = Some(unknown_saved);
        cases.push((dangling, SnapshotError::UnknownSavedPage(unknown_saved)));
        let mut non_web = original.clone();
        non_web.tabs[0].kind = TabKind::Note;
        cases.push((
            non_web,
            SnapshotError::NonWebSavedAssociation(original.tabs[0].id),
        ));
        let mut duplicate = original.clone();
        duplicate.tabs[1].saved_page_id = duplicate.tabs[0].saved_page_id;
        cases.push((
            duplicate,
            SnapshotError::DuplicateSavedAssociation(original.saved_pages[0].id),
        ));
        let mut reversed = original.clone();
        reversed.tabs.swap(0, 1);
        cases.push((
            reversed,
            SnapshotError::InvalidSavedPrefixOrder(original.tabs[0].id),
        ));
        let mut late_saved = original.clone();
        late_saved.tabs.swap(1, 2);
        cases.push((
            late_saved,
            SnapshotError::InvalidSavedPrefixOrder(original.tabs[1].id),
        ));

        for (snapshot, error) in cases {
            assert_eq!(snapshot.validate(), Err(error));
            assert_eq!(TabModel::from_snapshot(snapshot).err(), Some(error));
        }
    }

    #[test]
    fn snapshot_and_record_diagnostics_do_not_include_metadata() {
        let snapshot = fixture();
        let diagnostic = format!(
            "{snapshot:?} {:?} {:?}",
            snapshot.tabs, snapshot.saved_pages
        );
        for values in snapshot
            .tabs
            .iter()
            .map(|tab| &tab.metadata)
            .chain(snapshot.saved_pages.iter().map(|page| &page.metadata))
        {
            for value in [&values.title, &values.address] {
                assert!(!diagnostic.contains(value));
            }
            assert!(!diagnostic.contains(values.favicon.as_deref().expect("synthetic icon")));
            assert!(!diagnostic.contains(values.badge.as_deref().expect("synthetic badge")));
        }
    }
}

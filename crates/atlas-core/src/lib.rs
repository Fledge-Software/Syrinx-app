//! Owns Atlas's application state independently of its interface and browser engine.

mod tabs;

pub use tabs::{
    Activation, FolderRemoval, RemovedSavedEntries, SavedEntryId, SavedFolder, SavedFolderId,
    SavedFolderSnapshot, SavedPage, SavedPageId, SavedPageSnapshot, SavedPlacement, SidebarRow,
    SnapshotError, Tab, TabError, TabId, TabKind, TabMetadata, TabModel, TabModelSnapshot,
    TabSnapshot,
};

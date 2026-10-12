//! Owns Atlas's application state independently of its interface and browser engine.

mod bookmark_import;
mod destinations;
mod tabs;

pub use bookmark_import::{
    BookmarkImport, FolderRole, ImportError, ImportLimits, ImportSummary, ImportedBookmark,
    ImportedFolder, ImportedItem, SkipReason, SkippedEntry, parse_bookmark_html,
    parse_bookmark_html_with_limits,
};

pub use destinations::{
    BuiltInPlugin, Destination, DestinationEdit, DestinationError, DestinationSurface,
    DestinationTarget, MAX_DESTINATIONS, QuickDestinations,
};

pub use tabs::{
    Activation, FolderRemoval, RemovedSavedEntries, SavedEntryId, SavedFolder, SavedFolderId,
    SavedFolderSnapshot, SavedPage, SavedPageId, SavedPageSnapshot, SavedPlacement, SidebarRow,
    SnapshotError, Tab, TabError, TabId, TabKind, TabMetadata, TabModel, TabModelSnapshot,
    TabSnapshot,
};

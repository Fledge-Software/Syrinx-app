//! Owns Atlas's application state independently of its interface and browser engine.

mod bookmark_import;
mod tabs;

pub use bookmark_import::{
    BookmarkImport, FolderRole, ImportError, ImportLimits, ImportSummary, ImportedBookmark,
    ImportedFolder, ImportedItem, SkipReason, SkippedEntry, parse_bookmark_html,
    parse_bookmark_html_with_limits,
};

pub use tabs::{
    Activation, SavedPage, SavedPageId, SavedPageSnapshot, SidebarRow, SnapshotError, Tab,
    TabError, TabId, TabKind, TabMetadata, TabModel, TabModelSnapshot, TabSnapshot,
};

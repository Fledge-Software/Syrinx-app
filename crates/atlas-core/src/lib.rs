//! Owns Atlas's application state independently of its interface and browser engine.

mod bookmark_import;
mod tabs;
mod web_address;

pub use web_address::{WebAddressError, web_address};

pub use bookmark_import::{
    BookmarkImport, DEFAULT_IMPORT_FOLDER, FolderRole, ImportCommit, ImportError, ImportLimits,
    ImportSummary, ImportedBookmark, ImportedFolder, ImportedItem, RejectedBookmark, SkipReason,
    SkippedEntry, commit_bookmark_import, import_folder_title, parse_bookmark_html,
    parse_bookmark_html_with_limits,
};

pub use tabs::{
    Activation, AppendedSavedTree, FolderRemoval, NewSavedItem, RemovedSavedEntries, SavedEntryId,
    SavedFolder, SavedFolderId, SavedFolderSnapshot, SavedPage, SavedPageId, SavedPageSnapshot,
    SavedPlacement, SidebarRow, SnapshotError, Tab, TabError, TabId, TabKind, TabMetadata,
    TabModel, TabModelSnapshot, TabSnapshot,
};

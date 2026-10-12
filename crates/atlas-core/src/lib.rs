//! Owns Atlas's application state independently of its interface and browser engine.

mod activation;
mod address_display;
mod address_entry;
mod bookmark_import;
mod commands;
mod destinations;
mod tabs;
mod web_address;

pub use address_display::readable_address;
pub use address_entry::{EntryError, entry_destination, google_search, recognized_address};
pub use web_address::{WebAddressError, web_address};

pub use bookmark_import::{
    BookmarkImport, DEFAULT_IMPORT_FOLDER, FolderRole, ImportCommit, ImportError, ImportLimits,
    ImportSummary, ImportedBookmark, ImportedFolder, ImportedItem, RejectedBookmark, SkipReason,
    SkippedEntry, commit_bookmark_import, import_folder_title, parse_bookmark_html,
    parse_bookmark_html_with_limits,
};

pub use activation::{ActivationTarget, Route, live_instance, route};
pub use commands::{
    BrowserAction, Command, CommandAction, CommandContext, CommandError, CommandId,
    CommandRegistry, CommandScope, Enablement, Shortcut,
};
pub use destinations::{
    BuiltInPlugin, Destination, DestinationEdit, DestinationError, DestinationSurface,
    DestinationTarget, MAX_DESTINATIONS, QuickDestinations,
};

pub use tabs::{
    Activation, AppendedSavedTree, FolderRemoval, NewSavedItem, RemovedSavedEntries, SavedEntryId,
    SavedFolder, SavedFolderId, SavedFolderSnapshot, SavedPage, SavedPageId, SavedPageSnapshot,
    SavedPlacement, SidebarRow, SnapshotError, Tab, TabError, TabId, TabKind, TabMetadata,
    TabModel, TabModelSnapshot, TabSnapshot,
};

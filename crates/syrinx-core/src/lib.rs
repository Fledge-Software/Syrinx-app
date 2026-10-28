//! Owns Syrinx's application state independently of its interface and browser engine.

mod activation;
mod address_display;
mod address_entry;
mod address_prefix;
mod bookmark_import;
mod commands;
mod destinations;
mod downloads;
mod page_text;
mod tabs;
pub mod usage;
mod web_address;

pub use address_display::readable_address;
pub use address_entry::{EntryError, entry_destination, google_search, recognized_address};
pub use address_prefix::{AddressPrefix, address_prefix, is_bare_host};
pub use web_address::{WebAddressError, web_address};

pub use page_text::{
    DocumentFacts, EligibleDocument, Ineligible, LeftDocument, ObservedResponse, PageTextPolicy,
    PolicyError, SAME_DOCUMENT_INTERVAL, SETTLE_DELAY, TEXT_LIMIT_BYTES, address_within_site,
    excluded_site, judge, same_document_address, same_document_due,
};

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
    DestinationTarget, MAX_DESTINATIONS, MAX_LABEL_CHARS, QuickDestinations,
};
pub use downloads::{
    BlockReason, Download, DownloadControl, DownloadInterruption, DownloadList, DownloadState,
    MAX_DOWNLOADS, format_size,
};

pub use tabs::{
    Activation, AppendedSavedTree, BLANK_ADDRESS, BulkScope, FolderContents, FolderRemoval,
    HISTORY_ENTRY_LIMIT, HISTORY_ROW_LIMIT, HistoryChange, HistoryDirection, HistoryObserved,
    HistoryReport, HistoryRow, NavigationList, NavigationLists, NewSavedItem, PageOrigin,
    RemovedSavedEntries, SETTINGS_ADDRESS, SavedEntryId, SavedFolder, SavedFolderId,
    SavedFolderSnapshot, SavedPage, SavedPageId, SavedPageSnapshot, SavedPlacement, SidebarRow,
    SnapshotError, Tab, TabError, TabId, TabKind, TabMetadata, TabModel, TabModelSnapshot,
    TabSnapshot, saved_folder_contents,
};

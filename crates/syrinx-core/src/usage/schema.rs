//! The fixed version 1 usage schema (AN-01): every field Syrinx may count, compiled in.

use std::time::Duration;

use crate::BrowserAction;

/// Bumped only when the owner approves a changed field list.
pub const SCHEMA_VERSION: u32 = 1;

/// One feature counter, named `area.event` in a report.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Feature {
    AppStarted,
    TabCreated,
    TabClosed,
    TabReopened,
    TabSwitchedSidebar,
    TabSwitchedKeyboard,
    SavedSaved,
    SavedRemoved,
    SavedOpenedUnloaded,
    FolderCreated,
    FolderRemoved,
    ImportCompleted,
    PaletteOpenedCtrlK,
    PaletteOpenedCtrlL,
    PaletteDismissed,
    PaletteChoseTab,
    PaletteChoseSaved,
    PaletteChoseHistory,
    PaletteChosePageText,
    PaletteChoseCommand,
    PaletteChoseAddress,
    PaletteChoseWebSearch,
    PaletteScopeChanged,
    NavBack,
    NavForward,
    NavReload,
    SidebarSetWide,
    SidebarSetCompact,
    SidebarSetHidden,
    SidebarPeekShown,
    HomeTileOpened,
    FavoriteOpened,
    FindOpened,
    PageTextCaptured,
    SettingsOpened,
}

impl Feature {
    /// Every feature counter, in report order; each one's position is its slot.
    pub const ALL: [Self; 35] = [
        Self::AppStarted,
        Self::TabCreated,
        Self::TabClosed,
        Self::TabReopened,
        Self::TabSwitchedSidebar,
        Self::TabSwitchedKeyboard,
        Self::SavedSaved,
        Self::SavedRemoved,
        Self::SavedOpenedUnloaded,
        Self::FolderCreated,
        Self::FolderRemoved,
        Self::ImportCompleted,
        Self::PaletteOpenedCtrlK,
        Self::PaletteOpenedCtrlL,
        Self::PaletteDismissed,
        Self::PaletteChoseTab,
        Self::PaletteChoseSaved,
        Self::PaletteChoseHistory,
        Self::PaletteChosePageText,
        Self::PaletteChoseCommand,
        Self::PaletteChoseAddress,
        Self::PaletteChoseWebSearch,
        Self::PaletteScopeChanged,
        Self::NavBack,
        Self::NavForward,
        Self::NavReload,
        Self::SidebarSetWide,
        Self::SidebarSetCompact,
        Self::SidebarSetHidden,
        Self::SidebarPeekShown,
        Self::HomeTileOpened,
        Self::FavoriteOpened,
        Self::FindOpened,
        Self::PageTextCaptured,
        Self::SettingsOpened,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::AppStarted => "app.started",
            Self::TabCreated => "tab.created",
            Self::TabClosed => "tab.closed",
            Self::TabReopened => "tab.reopened",
            Self::TabSwitchedSidebar => "tab.switched_sidebar",
            Self::TabSwitchedKeyboard => "tab.switched_keyboard",
            Self::SavedSaved => "saved.saved",
            Self::SavedRemoved => "saved.removed",
            Self::SavedOpenedUnloaded => "saved.opened_unloaded",
            Self::FolderCreated => "folder.created",
            Self::FolderRemoved => "folder.removed",
            Self::ImportCompleted => "import.completed",
            Self::PaletteOpenedCtrlK => "palette.opened_ctrl_k",
            Self::PaletteOpenedCtrlL => "palette.opened_ctrl_l",
            Self::PaletteDismissed => "palette.dismissed",
            Self::PaletteChoseTab => "palette.chose_tab",
            Self::PaletteChoseSaved => "palette.chose_saved",
            Self::PaletteChoseHistory => "palette.chose_history",
            Self::PaletteChosePageText => "palette.chose_page_text",
            Self::PaletteChoseCommand => "palette.chose_command",
            Self::PaletteChoseAddress => "palette.chose_address",
            Self::PaletteChoseWebSearch => "palette.chose_web_search",
            Self::PaletteScopeChanged => "palette.scope_changed",
            Self::NavBack => "nav.back",
            Self::NavForward => "nav.forward",
            Self::NavReload => "nav.reload",
            Self::SidebarSetWide => "sidebar.set_wide",
            Self::SidebarSetCompact => "sidebar.set_compact",
            Self::SidebarSetHidden => "sidebar.set_hidden",
            Self::SidebarPeekShown => "sidebar.peek_shown",
            Self::HomeTileOpened => "home.tile_opened",
            Self::FavoriteOpened => "favorite.opened",
            Self::FindOpened => "find.opened",
            Self::PageTextCaptured => "page_text.captured",
            Self::SettingsOpened => "settings.opened",
        }
    }

    pub(crate) const fn slot(self) -> usize {
        FEATURE_BASE + self as usize
    }
}

/// The built-in commands' counters, frozen with the schema: `command.` and the registry's id.
/// A new built-in command fails `command_index`'s match until the owner approves a new version.
pub const COMMAND_KEYS: [&str; COMMAND_COUNT] = [
    "command.browser.new-tab",
    "command.browser.close-tab",
    "command.browser.reopen-tab",
    "command.browser.reload",
    "command.browser.back",
    "command.browser.forward",
    "command.browser.edit-address",
    "command.browser.next-tab",
    "command.browser.previous-tab",
    "command.browser.save-page",
    "command.browser.show-history",
    "command.browser.find",
    "command.browser.downloads",
    "command.browser.print",
    "command.browser.zoom-in",
    "command.browser.zoom-out",
    "command.browser.zoom-reset",
    "command.browser.clear-data",
    "command.browser.install-update",
    "command.browser.check-updates",
    "command.other",
];
const COMMAND_COUNT: usize = 21;
/// Any command that is not built in, such as a later plugin's, is counted here, never by name.
pub(crate) const OTHER_COMMAND: usize = COMMAND_COUNT - 1;

/// The built-in command's position in `COMMAND_KEYS`.
pub(crate) const fn command_index(action: BrowserAction) -> usize {
    match action {
        BrowserAction::NewTab => 0,
        BrowserAction::CloseTab => 1,
        BrowserAction::ReopenTab => 2,
        BrowserAction::Reload => 3,
        BrowserAction::Back => 4,
        BrowserAction::Forward => 5,
        BrowserAction::EditAddress => 6,
        BrowserAction::NextTab => 7,
        BrowserAction::PreviousTab => 8,
        BrowserAction::SavePage => 9,
        BrowserAction::ShowHistory => 10,
        BrowserAction::FindInPage => 11,
        BrowserAction::ShowDownloads => 12,
        BrowserAction::Print => 13,
        BrowserAction::ZoomIn => 14,
        BrowserAction::ZoomOut => 15,
        BrowserAction::ResetZoom => 16,
        BrowserAction::ClearBrowsingData => 17,
        BrowserAction::InstallUpdateFromFile => 18,
        BrowserAction::CheckForUpdates => 19,
    }
}

/// A timing histogram's bucket edges: names are upper edges in milliseconds, plus one `gt_`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scale {
    Interaction,
    Startup,
}

const INTERACTION_EDGES: [u64; 11] = [1, 2, 5, 8, 16, 33, 50, 100, 250, 500, 1000];
const INTERACTION_NAMES: [&str; 12] = [
    "le_1", "le_2", "le_5", "le_8", "le_16", "le_33", "le_50", "le_100", "le_250", "le_500",
    "le_1000", "gt_1000",
];
const STARTUP_EDGES: [u64; 9] = [100, 250, 500, 750, 1000, 1500, 2000, 3000, 5000];
const STARTUP_NAMES: [&str; 10] = [
    "le_100", "le_250", "le_500", "le_750", "le_1000", "le_1500", "le_2000", "le_3000", "le_5000",
    "gt_5000",
];

impl Scale {
    pub const fn bucket_names(self) -> &'static [&'static str] {
        match self {
            Self::Interaction => &INTERACTION_NAMES,
            Self::Startup => &STARTUP_NAMES,
        }
    }

    const fn edges(self) -> &'static [u64] {
        match self {
            Self::Interaction => &INTERACTION_EDGES,
            Self::Startup => &STARTUP_EDGES,
        }
    }

    /// The bucket a measurement falls in: the first edge it does not exceed, else the last.
    pub fn bucket(self, elapsed: Duration) -> usize {
        let edges = self.edges();
        edges
            .iter()
            .position(|edge| elapsed <= Duration::from_millis(*edge))
            .unwrap_or(edges.len())
    }
}

/// One timing histogram.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Timing {
    StartupFirstFrame,
    StartupSessionRestored,
    PaletteOpenToPaint,
    SearchTier1,
    SearchTier2,
    TabSwitchToVisible,
    StoreSave,
    BackupSnapshot,
}

impl Timing {
    pub const ALL: [Self; 8] = [
        Self::StartupFirstFrame,
        Self::StartupSessionRestored,
        Self::PaletteOpenToPaint,
        Self::SearchTier1,
        Self::SearchTier2,
        Self::TabSwitchToVisible,
        Self::StoreSave,
        Self::BackupSnapshot,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::StartupFirstFrame => "startup.first_frame_ms",
            Self::StartupSessionRestored => "startup.session_restored_ms",
            Self::PaletteOpenToPaint => "palette.open_to_paint_ms",
            Self::SearchTier1 => "search.tier1_ms",
            Self::SearchTier2 => "search.tier2_ms",
            Self::TabSwitchToVisible => "tab.switch_to_visible_ms",
            Self::StoreSave => "store.save_ms",
            Self::BackupSnapshot => "backup.snapshot_ms",
        }
    }

    pub const fn scale(self) -> Scale {
        match self {
            Self::StartupFirstFrame | Self::StartupSessionRestored | Self::BackupSnapshot => {
                Scale::Startup
            }
            Self::PaletteOpenToPaint
            | Self::SearchTier1
            | Self::SearchTier2
            | Self::TabSwitchToVisible
            | Self::StoreSave => Scale::Interaction,
        }
    }

    /// The slot of this histogram's first bucket; the others follow it.
    pub(crate) const fn first_slot(self) -> usize {
        let mut slot = TIMING_BASE;
        let mut index = 0;
        while index < self as usize {
            slot += Self::ALL[index].scale().bucket_names().len();
            index += 1;
        }
        slot
    }
}

/// One fixed kind of failure; never a message, code or path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ErrorCategory {
    StorageOpen,
    StorageWrite,
    StorageLocationRefused,
    BackupFailed,
    RecoveryOffered,
    UncleanShutdown,
    RendererFailed,
    BrowserProcessExited,
    WebviewCreation,
    NavigationFailed,
    ImportRefused,
    PageTextCapture,
    UsageFileReset,
}

impl ErrorCategory {
    pub const ALL: [Self; 13] = [
        Self::StorageOpen,
        Self::StorageWrite,
        Self::StorageLocationRefused,
        Self::BackupFailed,
        Self::RecoveryOffered,
        Self::UncleanShutdown,
        Self::RendererFailed,
        Self::BrowserProcessExited,
        Self::WebviewCreation,
        Self::NavigationFailed,
        Self::ImportRefused,
        Self::PageTextCapture,
        Self::UsageFileReset,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::StorageOpen => "error.storage_open",
            Self::StorageWrite => "error.storage_write",
            Self::StorageLocationRefused => "error.storage_location_refused",
            Self::BackupFailed => "error.backup_failed",
            Self::RecoveryOffered => "error.recovery_offered",
            Self::UncleanShutdown => "error.unclean_shutdown",
            Self::RendererFailed => "error.renderer_failed",
            Self::BrowserProcessExited => "error.browser_process_exited",
            Self::WebviewCreation => "error.webview_creation",
            Self::NavigationFailed => "error.navigation_failed",
            Self::ImportRefused => "error.import_refused",
            Self::PageTextCapture => "error.page_text_capture",
            Self::UsageFileReset => "error.usage_file_reset",
        }
    }

    pub(crate) const fn slot(self) -> usize {
        ERROR_BASE + self as usize
    }
}

/// Where each group of counters starts in the one fixed array.
const FEATURE_BASE: usize = 0;
pub(crate) const COMMAND_BASE: usize = FEATURE_BASE + Feature::ALL.len();
const TIMING_BASE: usize = COMMAND_BASE + COMMAND_COUNT;
const ERROR_BASE: usize = TIMING_BASE + timing_slots();
/// Every counter in version 1; well under the 4 KiB the proposal allows.
pub const SLOT_COUNT: usize = ERROR_BASE + ErrorCategory::ALL.len();

const fn timing_slots() -> usize {
    let mut slots = 0;
    let mut index = 0;
    while index < Timing::ALL.len() {
        slots += Timing::ALL[index].scale().bucket_names().len();
        index += 1;
    }
    slots
}

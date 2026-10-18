//! Names what the session worker was doing when it stopped, so its reason is never lost (#9).

use super::{SessionCommand, SessionError};

impl SessionCommand {
    /// A fixed name for this command, safe to show and log: it never includes the contents.
    pub(crate) fn operation(&self) -> &'static str {
        match self {
            Self::CheckPermission { .. } => "checking a site permission",
            Self::RememberPermission { .. } => "remembering a site permission",
            Self::RememberedPermissions { .. } => "listing site permissions",
            Self::ForgetPermission { .. } => "forgetting a site permission",
            Self::ConfirmRecovery { .. } => "confirming a backup recovery",
            Self::ConfirmHistoryRepair { .. } => "confirming a history repair",
            Self::HomeEntry { .. } => "opening from Home",
            Self::NewTab { .. } => "opening a new tab",
            Self::AdoptPopup { .. } => "adopting a popup",
            Self::NavigateActive { .. } => "navigating the active tab",
            Self::CloseTab(_) => "closing a tab",
            Self::ObserveClosed(_) => "recording a page that closed itself",
            Self::CloseActive => "closing the active tab",
            Self::CycleActive { .. } => "moving to the next tab",
            Self::FocusTab(_) => "switching tabs",
            Self::OpenSaved(_) => "opening a saved page",
            Self::SaveActive => "saving the active page",
            Self::Unsave(_) => "removing a saved page",
            Self::Folder(_) => "changing saved folders",
            Self::UpdateMetadata { .. } => "updating a tab's details",
            Self::ObserveMetadata { .. } => "recording a page's title or address",
            Self::ReopenClosed => "reopening a closed tab",
            Self::ImportBookmarks { .. } => "importing bookmarks",
            Self::SaveSettings(_) => "saving settings",
            Self::EditDestinations { .. } => "editing tiles",
            Self::Query { .. } => "searching",
            Self::ObserveNavigation { .. } => "recording a finished navigation",
            Self::OpenHistory(_) => "opening a history entry",
            Self::RecentHistory { .. } => "listing history",
            Self::ClearHistory { .. } => "clearing history",
            Self::GrowSyntheticCatalogue { .. } => "growing the synthetic catalogue",
            Self::SnapshotBeforeUpdate => "taking the pre-update snapshot",
            Self::Usage(_) => "changing usage counting",
            Self::UsageReport { .. } => "building a usage report",
            Self::Shutdown => "shutting down",
        }
    }
}

/// Tags an error that ends the worker's loop with the operation that raised it.
pub(super) fn during(operation: &'static str) -> impl FnOnce(SessionError) -> SessionError {
    move |error| SessionError::During {
        operation,
        error: Box::new(error),
    }
}

/// The diagnostic line for a stopped worker: error categories and operation names only.
pub(super) fn stop_line(error: &SessionError) -> String {
    format!("The session worker stopped: {error}")
}

/// Writes the stop reason to stderr, the app's diagnostic stream, even if the window is gone.
pub(super) fn log_stop(error: &SessionError) {
    eprintln!("{}", stop_line(error));
}

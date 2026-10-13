//! Turns a confirmed import into unloaded saved rows inside one new top-level folder (P1-22).

use std::fmt;

use super::address::skip_reason;
use super::text::is_bidi_control;
use super::{BookmarkImport, ImportedBookmark, ImportedFolder, ImportedItem, SkipReason};
use crate::{AppendedSavedTree, NewSavedItem, TabError, TabMetadata, TabModel, web_address};

/// The import's folder name when the file name gives none.
pub const DEFAULT_IMPORT_FOLDER: &str = "Imported bookmarks";
/// Empty folder names are refused (2026-10-12), so unnamed exported folders get this one.
const UNTITLED_FOLDER: &str = "Untitled folder";
/// Longest import folder name kept, in characters, matching the parser's title bound.
const MAX_FOLDER_TITLE_CHARS: usize = 1_024;

/// What a confirmed import wrote, and which bookmarks the final address check refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportCommit {
    pub tree: AppendedSavedTree,
    pub skipped: Vec<RejectedBookmark>,
}

/// A parsed bookmark that navigation's address rule refused when the import was written.
#[derive(Clone, Eq, PartialEq)]
pub struct RejectedBookmark {
    /// Shown in the result so the owner knows what was left out; never logged.
    pub title: String,
    pub reason: SkipReason,
}

/// Names the import's folder after the chosen file, e.g. "Imported bookmarks" or "bookmarks_10_12".
pub fn import_folder_title(file_stem: Option<&str>) -> String {
    let cleaned: String = file_stem
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control() && !is_bidi_control(*character))
        .take(MAX_FOLDER_TITLE_CHARS)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return DEFAULT_IMPORT_FOLDER.to_owned();
    }
    trimmed.to_owned()
}

/// Adds the import as one new top-level folder of unloaded saved pages, keeping the export's
/// hierarchy and order. Toolbar and unfiled folders become ordinary folders (2026-10-12).
///
/// Every address is checked again with the shared rule; refusals are reported, never written.
/// On error the model is unchanged.
pub fn commit_bookmark_import(
    model: &mut TabModel,
    import: &BookmarkImport,
    folder_title: String,
) -> Result<ImportCommit, TabError> {
    let mut skipped = Vec::new();
    let items = convert_items(&import.items, &mut skipped);
    let tree = model.append_saved_folder(folder_title, items)?;
    Ok(ImportCommit { tree, skipped })
}

/// Converts entries in export order; parser bounds keep the recursion shallow.
fn convert_items(items: &[ImportedItem], skipped: &mut Vec<RejectedBookmark>) -> Vec<NewSavedItem> {
    items
        .iter()
        .filter_map(|item| match item {
            ImportedItem::Folder(folder) => Some(convert_folder(folder, skipped)),
            ImportedItem::Bookmark(bookmark) => convert_bookmark(bookmark, skipped),
        })
        .collect()
}

fn convert_folder(folder: &ImportedFolder, skipped: &mut Vec<RejectedBookmark>) -> NewSavedItem {
    let title = if folder.title.trim().is_empty() {
        UNTITLED_FOLDER.to_owned()
    } else {
        folder.title.clone()
    };
    NewSavedItem::Folder {
        title,
        items: convert_items(&folder.items, skipped),
    }
}

/// Re-checks the address with navigation's rule; an untitled page shows its address.
fn convert_bookmark(
    bookmark: &ImportedBookmark,
    skipped: &mut Vec<RejectedBookmark>,
) -> Option<NewSavedItem> {
    let address = match web_address(&bookmark.address) {
        Ok(address) => address,
        Err(error) => {
            skipped.push(RejectedBookmark {
                title: bookmark.title.clone(),
                reason: skip_reason(error),
            });
            return None;
        }
    };
    let title = if bookmark.title.is_empty() {
        address.clone()
    } else {
        bookmark.title.clone()
    };
    Some(NewSavedItem::Page(TabMetadata {
        title,
        address,
        favicon: None,
        badge: None,
    }))
}

impl fmt::Debug for RejectedBookmark {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RejectedBookmark")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

//! Encodes complete tab snapshots and commits their records and revision together.

use atlas_core::{
    SavedEntryId, SavedFolderId, SavedFolderSnapshot, SavedPageId, SavedPageSnapshot,
    SavedPlacement, TabId, TabKind, TabMetadata, TabModelSnapshot, TabSnapshot,
};
use rusqlite::{Connection, Row, Transaction, TransactionBehavior, params, types::ValueRef};

use crate::engine::StoredSnapshot;
use crate::error::{EncodingField, StoreError, StoreOperation, database_error, with_cleanup};
use crate::schema::{CURRENT_GENERATION, SchemaGeneration};

struct SessionState {
    revision: u64,
    active_id: Option<TabId>,
    last_tab_id: u64,
    last_saved_page_id: u64,
}

/// The saved folders, their tree and their watermark, as one generation stores them.
struct SavedTree {
    folders: Vec<SavedFolderSnapshot>,
    layout: Vec<SavedPlacement>,
    last_saved_folder_id: u64,
}

pub(crate) fn load(
    connection: &mut Connection,
    generation: SchemaGeneration,
) -> Result<StoredSnapshot, StoreError> {
    // All tables come from the same SQLite read snapshot, even while the writer commits.
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| database_error(StoreOperation::BeginRead, error))?;
    let result = read_snapshot(&transaction, generation);
    finish_transaction(transaction, result)
}

/// Reads and validates current-schema records inside a caller's transaction.
pub(crate) fn read_current(connection: &Connection) -> Result<StoredSnapshot, StoreError> {
    read_snapshot(connection, CURRENT_GENERATION)
}

pub(crate) fn save(
    connection: &mut Connection,
    expected_revision: u64,
    snapshot: &TabModelSnapshot,
) -> Result<u64, StoreError> {
    // Validate before taking the write lock, then compare the revision under that lock.
    snapshot.validate()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| database_error(StoreOperation::BeginWrite, error))?;
    let result = replace_snapshot(&transaction, expected_revision, snapshot);
    finish_transaction(transaction, result)
}

fn read_snapshot(
    connection: &Connection,
    generation: SchemaGeneration,
) -> Result<StoredSnapshot, StoreError> {
    let state = read_state(connection)?;
    let saved_pages = read_saved_pages(connection)?;
    let tree = match generation {
        SchemaGeneration::FlatSaved => flat_tree(&saved_pages),
        SchemaGeneration::Folders => read_saved_tree(connection)?,
    };
    let snapshot = TabModelSnapshot {
        tabs: read_tabs(connection)?,
        saved_pages,
        saved_folders: tree.folders,
        saved_layout: tree.layout,
        active_id: state.active_id,
        last_tab_id: state.last_tab_id,
        last_saved_page_id: state.last_saved_page_id,
        last_saved_folder_id: tree.last_saved_folder_id,
    };
    snapshot.validate()?;
    Ok(StoredSnapshot {
        revision: state.revision,
        snapshot,
    })
}

fn read_state(connection: &Connection) -> Result<SessionState, StoreError> {
    let mut statement = connection
        .prepare_cached("SELECT singleton, revision, active_tab_id, last_tab_id, last_saved_page_id FROM session_state")
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let row = rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
        .ok_or(StoreError::InvalidEncoding(EncodingField::SessionState))?;
    if integer(row, 0, EncodingField::SessionState)? != 1 {
        return Err(StoreError::InvalidEncoding(EncodingField::SessionState));
    }
    let state = SessionState {
        revision: unsigned(row, 1, EncodingField::Revision)?,
        active_id: optional_tab_id(row, 2)?,
        last_tab_id: unsigned(row, 3, EncodingField::TabWatermark)?,
        last_saved_page_id: unsigned(row, 4, EncodingField::SavedPageWatermark)?,
    };
    if rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
        .is_some()
    {
        return Err(StoreError::InvalidEncoding(EncodingField::SessionState));
    }
    Ok(state)
}

fn read_saved_pages(connection: &Connection) -> Result<Vec<SavedPageSnapshot>, StoreError> {
    let mut statement = connection
        .prepare_cached("SELECT id, saved_order, title, address, favicon, badge FROM saved_pages ORDER BY saved_order")
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut pages = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
    {
        check_order(row, 1, pages.len(), EncodingField::SavedOrder)?;
        pages.push(SavedPageSnapshot {
            id: saved_page_id(row, 0)?,
            metadata: metadata(row, 2)?,
        });
    }
    Ok(pages)
}

/// A version 1 database has no folders: every saved page sits at the top level, in order.
fn flat_tree(pages: &[SavedPageSnapshot]) -> SavedTree {
    SavedTree {
        folders: Vec::new(),
        layout: pages
            .iter()
            .map(|page| SavedPlacement {
                entry: SavedEntryId::Page(page.id),
                parent: None,
            })
            .collect(),
        last_saved_folder_id: 0,
    }
}

fn read_saved_tree(connection: &Connection) -> Result<SavedTree, StoreError> {
    Ok(SavedTree {
        folders: read_saved_folders(connection)?,
        layout: read_saved_layout(connection)?,
        last_saved_folder_id: read_folder_watermark(connection)?,
    })
}

fn read_saved_folders(connection: &Connection) -> Result<Vec<SavedFolderSnapshot>, StoreError> {
    // Unplaced folders sort first, so core validation rejects them instead of hiding them.
    let mut statement = connection
        .prepare_cached("SELECT f.id, f.title FROM saved_folders AS f LEFT JOIN saved_layout AS l ON l.folder_id = f.id ORDER BY l.entry_order")
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut folders = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
    {
        folders.push(SavedFolderSnapshot {
            id: saved_folder_id(row, 0)?,
            title: text(row, 1)?,
        });
    }
    Ok(folders)
}

fn read_saved_layout(connection: &Connection) -> Result<Vec<SavedPlacement>, StoreError> {
    let mut statement = connection
        .prepare_cached("SELECT entry_order, page_id, folder_id, parent_folder_id FROM saved_layout ORDER BY entry_order")
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut layout = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
    {
        check_order(row, 0, layout.len(), EncodingField::SavedLayout)?;
        layout.push(SavedPlacement {
            entry: layout_entry(row)?,
            parent: match value(row, 3)? {
                ValueRef::Null => None,
                _ => Some(saved_folder_id(row, 3)?),
            },
        });
    }
    Ok(layout)
}

fn layout_entry(row: &Row<'_>) -> Result<SavedEntryId, StoreError> {
    // Exactly one of the page and folder columns names the entry.
    match (value(row, 1)?, value(row, 2)?) {
        (ValueRef::Null, ValueRef::Null) | (ValueRef::Blob(_), ValueRef::Blob(_)) => {
            Err(StoreError::InvalidEncoding(EncodingField::SavedLayout))
        }
        (_, ValueRef::Null) => Ok(SavedEntryId::Page(saved_page_id(row, 1)?)),
        _ => Ok(SavedEntryId::Folder(saved_folder_id(row, 2)?)),
    }
}

fn read_folder_watermark(connection: &Connection) -> Result<u64, StoreError> {
    let mut statement = connection
        .prepare_cached("SELECT singleton, last_saved_folder_id FROM saved_folder_state")
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let row = rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
        .ok_or(StoreError::InvalidEncoding(
            EncodingField::SavedFolderWatermark,
        ))?;
    if integer(row, 0, EncodingField::SavedFolderWatermark)? != 1 {
        return Err(StoreError::InvalidEncoding(
            EncodingField::SavedFolderWatermark,
        ));
    }
    let watermark = unsigned(row, 1, EncodingField::SavedFolderWatermark)?;
    if rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
        .is_some()
    {
        return Err(StoreError::InvalidEncoding(
            EncodingField::SavedFolderWatermark,
        ));
    }
    Ok(watermark)
}

fn read_tabs(connection: &Connection) -> Result<Vec<TabSnapshot>, StoreError> {
    let mut statement = connection
        .prepare_cached("SELECT id, live_order, kind, title, address, favicon, badge, saved_page_id FROM live_tabs ORDER BY live_order")
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::Read, error))?;
    let mut tabs = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| database_error(StoreOperation::Read, error))?
    {
        check_order(row, 1, tabs.len(), EncodingField::LiveOrder)?;
        tabs.push(TabSnapshot {
            id: tab_id(row, 0, EncodingField::TabId)?,
            kind: decode_kind(integer(row, 2, EncodingField::TabKind)?)?,
            metadata: metadata(row, 3)?,
            saved_page_id: match value(row, 7)? {
                ValueRef::Null => None,
                _ => Some(saved_page_id(row, 7)?),
            },
        });
    }
    Ok(tabs)
}

fn replace_snapshot(
    connection: &Connection,
    expected_revision: u64,
    snapshot: &TabModelSnapshot,
) -> Result<u64, StoreError> {
    let previous = read_state(connection)?;
    if previous.revision != expected_revision {
        return Err(StoreError::StaleRevision {
            expected: expected_revision,
            actual: previous.revision,
        });
    }
    if snapshot.last_tab_id < previous.last_tab_id
        || snapshot.last_saved_page_id < previous.last_saved_page_id
        || snapshot.last_saved_folder_id < read_folder_watermark(connection)?
    {
        return Err(StoreError::WatermarkRegression);
    }
    let revision = previous
        .revision
        .checked_add(1)
        .ok_or(StoreError::RevisionExhausted)?;

    // Clear selection and the tree before replacing rows, then restore them with the new counters.
    connection
        .execute_batch("UPDATE session_state SET active_tab_id = NULL; DELETE FROM saved_layout; DELETE FROM live_tabs; DELETE FROM saved_pages; DELETE FROM saved_folders;")
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    write_saved_folders(connection, &snapshot.saved_folders)?;
    write_saved_pages(connection, &snapshot.saved_pages)?;
    write_saved_layout(connection, &snapshot.saved_layout)?;
    write_tabs(connection, &snapshot.tabs)?;
    write_folder_watermark(connection, snapshot.last_saved_folder_id)?;
    let active_id = snapshot.active_id.map(|id| id.get().to_be_bytes());
    let updated = connection
        .execute(
            "UPDATE session_state SET revision = ?1, active_tab_id = ?2, last_tab_id = ?3, last_saved_page_id = ?4 WHERE singleton = 1",
            params![revision.to_be_bytes(), active_id, snapshot.last_tab_id.to_be_bytes(), snapshot.last_saved_page_id.to_be_bytes()],
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    if updated != 1 {
        return Err(StoreError::InvalidEncoding(EncodingField::SessionState));
    }
    Ok(revision)
}

fn write_saved_pages(
    connection: &Connection,
    pages: &[SavedPageSnapshot],
) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare_cached("INSERT INTO saved_pages(id, saved_order, title, address, favicon, badge) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    for (position, page) in pages.iter().enumerate() {
        statement
            .execute(params![
                page.id.get().to_be_bytes(),
                order_value(position)?,
                page.metadata.title,
                page.metadata.address,
                page.metadata.favicon,
                page.metadata.badge
            ])
            .map_err(|error| database_error(StoreOperation::Write, error))?;
    }
    Ok(())
}

fn write_saved_folders(
    connection: &Connection,
    folders: &[SavedFolderSnapshot],
) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare_cached("INSERT INTO saved_folders(id, title) VALUES (?1, ?2)")
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    for folder in folders {
        statement
            .execute(params![folder.id.get().to_be_bytes(), folder.title])
            .map_err(|error| database_error(StoreOperation::Write, error))?;
    }
    Ok(())
}

fn write_saved_layout(
    connection: &Connection,
    layout: &[SavedPlacement],
) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare_cached("INSERT INTO saved_layout(entry_order, page_id, folder_id, parent_folder_id) VALUES (?1, ?2, ?3, ?4)")
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    for (position, placement) in layout.iter().enumerate() {
        let (page_id, folder_id) = match placement.entry {
            SavedEntryId::Page(id) => (Some(id.get().to_be_bytes()), None),
            SavedEntryId::Folder(id) => (None, Some(id.get().to_be_bytes())),
        };
        let parent_id = placement.parent.map(|id| id.get().to_be_bytes());
        statement
            .execute(params![
                order_value(position)?,
                page_id,
                folder_id,
                parent_id
            ])
            .map_err(|error| database_error(StoreOperation::Write, error))?;
    }
    Ok(())
}

fn write_folder_watermark(connection: &Connection, watermark: u64) -> Result<(), StoreError> {
    let updated = connection
        .execute(
            "UPDATE saved_folder_state SET last_saved_folder_id = ?1 WHERE singleton = 1",
            [watermark.to_be_bytes()],
        )
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    if updated != 1 {
        return Err(StoreError::InvalidEncoding(
            EncodingField::SavedFolderWatermark,
        ));
    }
    Ok(())
}

fn write_tabs(connection: &Connection, tabs: &[TabSnapshot]) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare_cached("INSERT INTO live_tabs(id, live_order, kind, title, address, favicon, badge, saved_page_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)")
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    for (position, tab) in tabs.iter().enumerate() {
        let saved_id = tab.saved_page_id.map(|id| id.get().to_be_bytes());
        statement
            .execute(params![
                tab.id.get().to_be_bytes(),
                order_value(position)?,
                encode_kind(tab.kind),
                tab.metadata.title,
                tab.metadata.address,
                tab.metadata.favicon,
                tab.metadata.badge,
                saved_id
            ])
            .map_err(|error| database_error(StoreOperation::Write, error))?;
    }
    Ok(())
}

fn value<'row>(row: &'row Row<'_>, column: usize) -> Result<ValueRef<'row>, StoreError> {
    row.get_ref(column)
        .map_err(|error| database_error(StoreOperation::Read, error))
}

fn unsigned(row: &Row<'_>, column: usize, field: EncodingField) -> Result<u64, StoreError> {
    let ValueRef::Blob(bytes) = value(row, column)? else {
        return Err(StoreError::InvalidEncoding(field));
    };
    let bytes = <[u8; 8]>::try_from(bytes).map_err(|_| StoreError::InvalidEncoding(field))?;
    Ok(u64::from_be_bytes(bytes))
}

fn integer(row: &Row<'_>, column: usize, field: EncodingField) -> Result<i64, StoreError> {
    match value(row, column)? {
        ValueRef::Integer(number) => Ok(number),
        _ => Err(StoreError::InvalidEncoding(field)),
    }
}

fn tab_id(row: &Row<'_>, column: usize, field: EncodingField) -> Result<TabId, StoreError> {
    TabId::new(unsigned(row, column, field)?).map_err(|_| StoreError::InvalidEncoding(field))
}

fn saved_page_id(row: &Row<'_>, column: usize) -> Result<SavedPageId, StoreError> {
    SavedPageId::new(unsigned(row, column, EncodingField::SavedPageId)?)
        .map_err(|_| StoreError::InvalidEncoding(EncodingField::SavedPageId))
}

fn saved_folder_id(row: &Row<'_>, column: usize) -> Result<SavedFolderId, StoreError> {
    SavedFolderId::new(unsigned(row, column, EncodingField::SavedFolderId)?)
        .map_err(|_| StoreError::InvalidEncoding(EncodingField::SavedFolderId))
}

fn optional_tab_id(row: &Row<'_>, column: usize) -> Result<Option<TabId>, StoreError> {
    match value(row, column)? {
        ValueRef::Null => Ok(None),
        _ => tab_id(row, column, EncodingField::ActiveTabId).map(Some),
    }
}

fn text(row: &Row<'_>, column: usize) -> Result<String, StoreError> {
    let ValueRef::Text(bytes) = value(row, column)? else {
        return Err(StoreError::InvalidEncoding(EncodingField::Metadata));
    };
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| StoreError::InvalidEncoding(EncodingField::Metadata))
}

fn optional_text(row: &Row<'_>, column: usize) -> Result<Option<String>, StoreError> {
    match value(row, column)? {
        ValueRef::Null => Ok(None),
        _ => text(row, column).map(Some),
    }
}

fn metadata(row: &Row<'_>, first: usize) -> Result<TabMetadata, StoreError> {
    Ok(TabMetadata {
        title: text(row, first)?,
        address: text(row, first + 1)?,
        favicon: optional_text(row, first + 2)?,
        badge: optional_text(row, first + 3)?,
    })
}

fn order_value(position: usize) -> Result<i64, StoreError> {
    i64::try_from(position).map_err(|_| StoreError::TooManyRecords)
}

fn check_order(
    row: &Row<'_>,
    column: usize,
    position: usize,
    field: EncodingField,
) -> Result<(), StoreError> {
    if integer(row, column, field)? != order_value(position)? {
        return Err(StoreError::InvalidEncoding(field));
    }
    Ok(())
}

fn encode_kind(kind: TabKind) -> i64 {
    match kind {
        TabKind::Web => 0,
        TabKind::PluginView => 1,
        TabKind::Note => 2,
        TabKind::File => 3,
        TabKind::Diff => 4,
        TabKind::Terminal => 5,
    }
}

fn decode_kind(kind: i64) -> Result<TabKind, StoreError> {
    match kind {
        0 => Ok(TabKind::Web),
        1 => Ok(TabKind::PluginView),
        2 => Ok(TabKind::Note),
        3 => Ok(TabKind::File),
        4 => Ok(TabKind::Diff),
        5 => Ok(TabKind::Terminal),
        _ => Err(StoreError::InvalidEncoding(EncodingField::TabKind)),
    }
}

fn finish_transaction<T>(
    transaction: Transaction<'_>,
    result: Result<T, StoreError>,
) -> Result<T, StoreError> {
    // Explicitly observe commit and rollback failures instead of relying on Transaction::drop.
    let result = result.and_then(|value| {
        transaction
            .execute_batch("COMMIT")
            .map_err(|error| database_error(StoreOperation::Commit, error))?;
        Ok(value)
    });
    match result {
        Ok(value) => Ok(value),
        Err(error) if transaction.is_autocommit() => Err(error),
        Err(error) => {
            let rollback = transaction
                .rollback()
                .map_err(|error| database_error(StoreOperation::Rollback, error));
            Err(with_cleanup(error, rollback))
        }
    }
}

//! Encodes complete tab snapshots and commits their records and revision together.

use atlas_core::{
    SavedPageId, SavedPageSnapshot, TabId, TabKind, TabMetadata, TabModelSnapshot, TabSnapshot,
};
use rusqlite::{Connection, Row, Transaction, TransactionBehavior, params, types::ValueRef};

use crate::engine::StoredSnapshot;
use crate::error::{EncodingField, StoreError, StoreOperation, database_error, with_cleanup};

struct SessionState {
    revision: u64,
    active_id: Option<TabId>,
    last_tab_id: u64,
    last_saved_page_id: u64,
}

pub(crate) fn load(connection: &mut Connection) -> Result<StoredSnapshot, StoreError> {
    // All tables come from the same SQLite read snapshot, even while the writer commits.
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| database_error(StoreOperation::BeginRead, error))?;
    let result = read_snapshot(&transaction);
    finish_transaction(transaction, result)
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

fn read_snapshot(connection: &Connection) -> Result<StoredSnapshot, StoreError> {
    let state = read_state(connection)?;
    let snapshot = TabModelSnapshot {
        tabs: read_tabs(connection)?,
        saved_pages: read_saved_pages(connection)?,
        active_id: state.active_id,
        last_tab_id: state.last_tab_id,
        last_saved_page_id: state.last_saved_page_id,
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
    {
        return Err(StoreError::WatermarkRegression);
    }
    let revision = previous
        .revision
        .checked_add(1)
        .ok_or(StoreError::RevisionExhausted)?;

    // Clear selection before replacing rows, then restore it with the new counters.
    connection
        .execute_batch("UPDATE session_state SET active_tab_id = NULL; DELETE FROM live_tabs; DELETE FROM saved_pages;")
        .map_err(|error| database_error(StoreOperation::Write, error))?;
    write_saved_pages(connection, &snapshot.saved_pages)?;
    write_tabs(connection, &snapshot.tabs)?;
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

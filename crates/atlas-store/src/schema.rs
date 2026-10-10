//! Identifies Atlas databases before enabling writes and applies transactional migrations.

use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};
use rusqlite_migration::{M, Migrations};

use crate::ShutdownState;
use crate::error::{StoreError, StoreOperation, database_error, with_cleanup};

pub(crate) const APPLICATION_ID: i64 = 0x4154_4c53;
pub(crate) const SCHEMA_VERSION: i64 = 1;

const SAVED_TABLE: &str = "CREATE TABLE saved_pages (
    id BLOB PRIMARY KEY NOT NULL CHECK(length(id) = 8 AND id != zeroblob(8)),
    saved_order INTEGER NOT NULL UNIQUE CHECK(saved_order >= 0),
    title TEXT NOT NULL,
    address TEXT NOT NULL,
    favicon TEXT,
    badge TEXT
) STRICT";

const LIVE_TABLE: &str = "CREATE TABLE live_tabs (
    id BLOB PRIMARY KEY NOT NULL CHECK(length(id) = 8 AND id != zeroblob(8)),
    live_order INTEGER NOT NULL UNIQUE CHECK(live_order >= 0),
    kind INTEGER NOT NULL CHECK(kind BETWEEN 0 AND 5),
    title TEXT NOT NULL,
    address TEXT NOT NULL,
    favicon TEXT,
    badge TEXT,
    saved_page_id BLOB UNIQUE REFERENCES saved_pages(id)
        CHECK(saved_page_id IS NULL OR
            (length(saved_page_id) = 8 AND saved_page_id != zeroblob(8) AND kind = 0))
) STRICT";

const STATE_TABLE: &str = "CREATE TABLE session_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    revision BLOB NOT NULL CHECK(length(revision) = 8),
    active_tab_id BLOB REFERENCES live_tabs(id) DEFERRABLE INITIALLY DEFERRED
        CHECK(active_tab_id IS NULL OR
            (length(active_tab_id) = 8 AND active_tab_id != zeroblob(8))),
    last_tab_id BLOB NOT NULL CHECK(length(last_tab_id) = 8),
    last_saved_page_id BLOB NOT NULL CHECK(length(last_saved_page_id) = 8)
) STRICT";

pub(crate) fn configure_connection(connection: &Connection) -> Result<(), StoreError> {
    // These connection-local settings do not modify a foreign database's schema or journal mode.
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| database_error(StoreOperation::Configure, error))?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA trusted_schema = OFF;")
        .map_err(|error| database_error(StoreOperation::Configure, error))?;
    connection.set_prepared_statement_cache_capacity(16);
    Ok(())
}

pub(crate) fn prepare_writer(
    connection: &mut Connection,
    previous_shutdown: ShutdownState,
) -> Result<(), StoreError> {
    // Refuse foreign, altered and future schemas before changing persistent settings.
    let new_database = inspect_identity(connection)?;
    if !new_database {
        validate_schema(connection)?;
        crate::records::load(connection)?;
        if previous_shutdown == ShutdownState::Unclean {
            quick_check(connection)?;
        }
    }
    let journal: String = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(|error| database_error(StoreOperation::Configure, error))?;
    if !journal.eq_ignore_ascii_case("wal") {
        return Err(StoreError::SchemaMismatch);
    }
    connection
        .execute_batch("PRAGMA synchronous = NORMAL;")
        .map_err(|error| database_error(StoreOperation::Configure, error))?;

    // The application marker, all tables and the initial state share the migration transaction.
    if new_database {
        let sql = initial_schema();
        let migrations = Migrations::new(vec![M::up(&sql).foreign_key_check()]);
        apply_migrations(connection, &migrations)?;
        crate::records::load(connection)?;
    }
    validate_schema(connection)?;
    optimize(connection)
}

fn quick_check(connection: &Connection) -> Result<(), StoreError> {
    integrity_check(connection, "PRAGMA quick_check", StoreOperation::QuickCheck)
}

pub(crate) fn validate_backup(
    connection: &mut Connection,
) -> Result<crate::StoredSnapshot, StoreError> {
    // A backup is already complete; validation never migrates it or enables a writable journal.
    configure_connection(connection)?;
    prepare_reader(connection)?;
    integrity_check(
        connection,
        "PRAGMA integrity_check",
        StoreOperation::IntegrityCheck,
    )?;
    let mut statement = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(|error| database_error(StoreOperation::IntegrityCheck, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::IntegrityCheck, error))?;
    if rows
        .next()
        .map_err(|error| database_error(StoreOperation::IntegrityCheck, error))?
        .is_some()
    {
        return Err(StoreError::IntegrityCheckFailed);
    }
    drop(rows);
    drop(statement);
    crate::records::load(connection)
}

fn integrity_check(
    connection: &Connection,
    pragma: &str,
    operation: StoreOperation,
) -> Result<(), StoreError> {
    // SQLite's diagnostic rows can contain record details; accept only one exact success row.
    let mut statement = connection
        .prepare(pragma)
        .map_err(|error| database_error(operation, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(operation, error))?;
    let row = rows
        .next()
        .map_err(|error| database_error(operation, error))?
        .ok_or(StoreError::IntegrityCheckFailed)?;
    let result = row
        .get_ref(0)
        .map_err(|error| database_error(operation, error))?;
    if result != rusqlite::types::ValueRef::Text(b"ok") {
        return Err(StoreError::IntegrityCheckFailed);
    }
    if rows
        .next()
        .map_err(|error| database_error(operation, error))?
        .is_some()
    {
        return Err(StoreError::IntegrityCheckFailed);
    }
    Ok(())
}

pub(crate) fn prepare_reader(connection: &Connection) -> Result<(), StoreError> {
    // Reader connections cannot change records even if a later query is written incorrectly.
    if inspect_identity(connection)? {
        return Err(StoreError::SchemaMismatch);
    }
    validate_schema(connection)?;
    connection
        .execute_batch("PRAGMA synchronous = NORMAL; PRAGMA query_only = ON;")
        .map_err(|error| database_error(StoreOperation::Configure, error))
}

fn inspect_identity(connection: &Connection) -> Result<bool, StoreError> {
    let application_id: i64 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|error| database_error(StoreOperation::InspectSchema, error))?;
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| database_error(StoreOperation::InspectSchema, error))?;
    let object_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| database_error(StoreOperation::InspectSchema, error))?;
    if application_id == 0 && version == 0 && object_count == 0 {
        return Ok(true);
    }
    if application_id != APPLICATION_ID {
        return Err(StoreError::ForeignDatabase);
    }
    if version > SCHEMA_VERSION {
        return Err(StoreError::FutureSchema { version });
    }
    if version != SCHEMA_VERSION {
        return Err(StoreError::SchemaMismatch);
    }
    Ok(false)
}

pub(crate) fn initial_schema() -> String {
    format!(
        "{SAVED_TABLE}; {LIVE_TABLE}; {STATE_TABLE};
         INSERT INTO session_state VALUES (1, zeroblob(8), NULL, zeroblob(8), zeroblob(8));
         PRAGMA application_id = {APPLICATION_ID};"
    )
}

pub(crate) fn apply_migrations(
    connection: &mut Connection,
    migrations: &Migrations<'_>,
) -> Result<(), StoreError> {
    // Take the write lock before migration statements, and report rollback failures explicitly.
    connection.set_transaction_behavior(TransactionBehavior::Immediate);
    let result = migrations.to_latest(connection);
    if result.is_ok() {
        return Ok(());
    }
    let error = StoreError::Database {
        operation: StoreOperation::Migrate,
        code: None,
    };
    if connection.is_autocommit() {
        return Err(error);
    }
    let rollback = connection
        .execute_batch("ROLLBACK")
        .map_err(|error| database_error(StoreOperation::Rollback, error));
    Err(with_cleanup(error, rollback))
}

fn validate_schema(connection: &Connection) -> Result<(), StoreError> {
    // Match the owned schema exactly; a copied marker alone does not identify an Atlas database.
    let expected = [
        ("live_tabs", LIVE_TABLE),
        ("saved_pages", SAVED_TABLE),
        ("session_state", STATE_TABLE),
    ];
    let mut statement = connection
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name")
        .map_err(|error| database_error(StoreOperation::InspectSchema, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::InspectSchema, error))?;
    for (name, sql) in expected {
        let row = rows
            .next()
            .map_err(|error| database_error(StoreOperation::InspectSchema, error))?
            .ok_or(StoreError::SchemaMismatch)?;
        let values = (
            row.get::<_, String>(0),
            row.get::<_, String>(1),
            row.get::<_, String>(2),
            row.get::<_, String>(3),
        );
        let (Ok(kind), Ok(actual_name), Ok(table), Ok(actual_sql)) = values else {
            return Err(StoreError::SchemaMismatch);
        };
        if kind != "table" || actual_name != name || table != name || actual_sql != sql {
            return Err(StoreError::SchemaMismatch);
        }
    }
    if rows
        .next()
        .map_err(|error| database_error(StoreOperation::InspectSchema, error))?
        .is_some()
    {
        return Err(StoreError::SchemaMismatch);
    }
    Ok(())
}

pub(crate) fn optimize(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch("PRAGMA optimize")
        .map_err(|error| database_error(StoreOperation::Optimize, error))
}

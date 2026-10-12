//! Identifies Atlas databases before enabling writes and applies transactional migrations.

use std::time::Duration;

use rusqlite::{Connection, Transaction, TransactionBehavior};
use rusqlite_migration::{HookError, HookResult, M, Migrations};

use crate::ShutdownState;
use crate::error::{StoreError, StoreOperation, database_error, with_cleanup};

pub(crate) const APPLICATION_ID: i64 = 0x4154_4c53;
pub(crate) const SCHEMA_VERSION: i64 = 5;

/// The record layout of a recognized Atlas database, by `user_version`.
/// Declared in version order, so later generations compare greater.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum SchemaGeneration {
    /// Version 1 (P1-03): saved pages form one flat, ordered list.
    FlatSaved,
    /// Version 2 (P1-21): saved pages and folders form an ordered tree.
    Folders,
    /// Version 3 (P1-28/P1-29): adds owner settings and the Quick access/Favorites tiles.
    Preferences,
    /// Version 4 (P1-20): adds browsing history beside the version 3 session and preferences.
    History,
    /// Version 5 (P1-27): records which D47 history clears this database has applied.
    Deletions,
}

pub(crate) const CURRENT_GENERATION: SchemaGeneration = SchemaGeneration::Deletions;

impl SchemaGeneration {
    /// Whether this generation (version 3 or later) stores settings and tiles.
    fn has_preferences(self) -> bool {
        self >= Self::Preferences
    }

    /// Whether this generation (version 4 or later) stores history that a clear must reach.
    pub(crate) fn has_history(self) -> bool {
        self >= Self::History
    }

    /// Whether this generation (version 5 or later) records its applied clear generation.
    pub(crate) fn has_deletion_state(self) -> bool {
        self >= Self::Deletions
    }
}

/// What a database file holds before Atlas writes to it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Identity {
    Empty,
    Atlas(SchemaGeneration),
}

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

const FOLDERS_TABLE: &str = "CREATE TABLE saved_folders (
    id BLOB PRIMARY KEY NOT NULL CHECK(length(id) = 8 AND id != zeroblob(8)),
    title TEXT NOT NULL
) STRICT";

// Every saved page and folder in visible depth-first order, with its parent folder.
const LAYOUT_TABLE: &str = "CREATE TABLE saved_layout (
    entry_order INTEGER PRIMARY KEY CHECK(entry_order >= 0),
    page_id BLOB UNIQUE REFERENCES saved_pages(id),
    folder_id BLOB UNIQUE REFERENCES saved_folders(id),
    parent_folder_id BLOB REFERENCES saved_folders(id),
    CHECK((page_id IS NULL) != (folder_id IS NULL))
) STRICT";

const FOLDER_STATE_TABLE: &str = "CREATE TABLE saved_folder_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    last_saved_folder_id BLOB NOT NULL CHECK(length(last_saved_folder_id) = 8)
) STRICT";

// One row per visited address; AUTOINCREMENT keeps cleared identities from being reused.
const HISTORY_URLS_TABLE: &str = "CREATE TABLE history_urls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    address TEXT NOT NULL UNIQUE CHECK(length(address) > 0),
    title TEXT NOT NULL,
    visit_count INTEGER NOT NULL CHECK(visit_count > 0),
    last_visited INTEGER NOT NULL CHECK(last_visited >= 0)
) STRICT";

// One row per visit, timed in Unix seconds to match D47's deletion ranges.
const HISTORY_VISITS_TABLE: &str = "CREATE TABLE history_visits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url_id INTEGER NOT NULL REFERENCES history_urls(id),
    visited_at INTEGER NOT NULL CHECK(visited_at >= 0)
) STRICT";

// The newest D47 clear applied here; a restored copy replays the external journal past it.
const DELETION_STATE_TABLE: &str = "CREATE TABLE history_deletion_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    applied_generation INTEGER NOT NULL CHECK(applied_generation >= 0)
) STRICT";

const HISTORY_VISITS_INDEX: &str =
    "CREATE INDEX history_visits_by_url ON history_visits(url_id, visited_at)";

// Version 5: range clears and retention expiry find visits by time without a full scan.
const HISTORY_TIME_INDEX: &str =
    "CREATE INDEX history_visits_by_time ON history_visits(visited_at)";

// Owner preferences as documented keys and text values; `docs/p1-28-29-home-settings.md` lists them.
const SETTINGS_TABLE: &str = "CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL CHECK(length(key) BETWEEN 1 AND 64),
    value TEXT NOT NULL CHECK(length(value) <= 256)
) STRICT";

// Quick access (surface 0) and sidebar Favorites (surface 1); a target is a web address or plugin id.
const DESTINATIONS_TABLE: &str = "CREATE TABLE quick_destinations (
    surface INTEGER NOT NULL CHECK(surface IN (0, 1)),
    position INTEGER NOT NULL CHECK(position >= 0),
    label TEXT NOT NULL,
    target_kind INTEGER NOT NULL CHECK(target_kind IN (0, 1)),
    target TEXT NOT NULL,
    PRIMARY KEY(surface, position)
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
    // Refuse foreign, altered and future schemas, and damaged records, before any change.
    let identity = inspect_identity(connection)?;
    if let Identity::Atlas(generation) = identity {
        validate_schema(connection, generation)?;
        crate::records::load(connection, generation)?;
        if generation.has_preferences() {
            crate::preferences::load(connection)?;
        }
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

    // Pending migrations, and for a new file the application marker, share one transaction.
    if identity != Identity::Atlas(CURRENT_GENERATION) {
        let scripts = MigrationScripts::new();
        apply_migrations(connection, &scripts.migrations())?;
        crate::records::load(connection, CURRENT_GENERATION)?;
    }
    validate_schema(connection, CURRENT_GENERATION)?;
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
    let generation = prepare_reader(connection)?;
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
    // An older backup stays valid; restoring it lets the next writer migrate it.
    let snapshot = crate::records::load(connection, generation)?;
    if generation.has_preferences() {
        crate::preferences::load(connection)?;
    }
    Ok(snapshot)
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

/// Identifies a staged copy's exact Atlas schema without migrating or restricting it.
pub(crate) fn staged_generation(connection: &Connection) -> Result<SchemaGeneration, StoreError> {
    let Identity::Atlas(generation) = inspect_identity(connection)? else {
        return Err(StoreError::SchemaMismatch);
    };
    validate_schema(connection, generation)?;
    Ok(generation)
}

pub(crate) fn prepare_reader(connection: &Connection) -> Result<SchemaGeneration, StoreError> {
    // Reader connections cannot change records even if a later query is written incorrectly.
    let Identity::Atlas(generation) = inspect_identity(connection)? else {
        return Err(StoreError::SchemaMismatch);
    };
    validate_schema(connection, generation)?;
    connection
        .execute_batch("PRAGMA synchronous = NORMAL; PRAGMA query_only = ON;")
        .map_err(|error| database_error(StoreOperation::Configure, error))?;
    Ok(generation)
}

fn inspect_identity(connection: &Connection) -> Result<Identity, StoreError> {
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
        return Ok(Identity::Empty);
    }
    if application_id != APPLICATION_ID {
        return Err(StoreError::ForeignDatabase);
    }
    match version {
        1 => Ok(Identity::Atlas(SchemaGeneration::FlatSaved)),
        2 => Ok(Identity::Atlas(SchemaGeneration::Folders)),
        3 => Ok(Identity::Atlas(SchemaGeneration::Preferences)),
        4 => Ok(Identity::Atlas(SchemaGeneration::History)),
        5 => Ok(Identity::Atlas(SchemaGeneration::Deletions)),
        version if version > SCHEMA_VERSION => Err(StoreError::FutureSchema { version }),
        _ => Err(StoreError::SchemaMismatch),
    }
}

/// Version 1: the flat saved-page schema created by P1-03.
pub(crate) fn initial_schema() -> String {
    format!(
        "{SAVED_TABLE}; {LIVE_TABLE}; {STATE_TABLE};
         INSERT INTO session_state VALUES (1, zeroblob(8), NULL, zeroblob(8), zeroblob(8));
         PRAGMA application_id = {APPLICATION_ID};"
    )
}

/// Version 2: adds folders and places every existing saved page at the top level, in order.
/// It only creates and fills its own tables, so it composes with unrelated later migrations.
pub(crate) fn folders_migration() -> String {
    format!(
        "{FOLDERS_TABLE}; {LAYOUT_TABLE}; {FOLDER_STATE_TABLE};
         INSERT INTO saved_folder_state VALUES (1, zeroblob(8));
         INSERT INTO saved_layout(entry_order, page_id, folder_id, parent_folder_id)
             SELECT saved_order, id, NULL, NULL FROM saved_pages;"
    )
}

/// Version 3: adds empty settings and tile tables; absent rows mean the documented defaults.
pub(crate) fn preferences_migration() -> String {
    format!("{SETTINGS_TABLE}; {DESTINATIONS_TABLE};")
}

/// Version 4: adds empty history tables. It alters no earlier table, and history starts
/// empty because nothing is imported (D10).
pub(crate) fn history_migration() -> String {
    format!("{HISTORY_URLS_TABLE}; {HISTORY_VISITS_TABLE}; {HISTORY_VISITS_INDEX};")
}

/// Version 5: records that no clear has been applied yet. The startup reconcile replays the
/// journal past this generation, so history kept from an older file is still cleared.
pub(crate) fn deletions_migration() -> String {
    format!(
        "{DELETION_STATE_TABLE}; INSERT INTO history_deletion_state VALUES (1, 0); {HISTORY_TIME_INDEX};"
    )
}

/// Owns the migration SQL that `rusqlite_migration` borrows, numbered by position.
pub(crate) struct MigrationScripts {
    initial: String,
    folders: String,
    preferences: String,
    history: String,
    deletions: String,
}

impl MigrationScripts {
    pub(crate) fn new() -> Self {
        Self {
            initial: initial_schema(),
            folders: folders_migration(),
            preferences: preferences_migration(),
            history: history_migration(),
            deletions: deletions_migration(),
        }
    }

    pub(crate) fn migrations(&self) -> Migrations<'_> {
        Migrations::new(vec![
            M::up(&self.initial).foreign_key_check(),
            M::up_with_hook(&self.folders, check_migrated_records).foreign_key_check(),
            M::up_with_hook(&self.preferences, check_migrated_preferences),
            M::up_with_hook(&self.history, check_migrated_records).foreign_key_check(),
            M::up_with_hook(&self.deletions, check_migrated_deletions),
        ])
    }
}

fn check_migrated_records(transaction: &Transaction<'_>) -> HookResult {
    // Refuse to commit a migration whose result is not a complete, valid saved tree.
    crate::records::read_current(transaction)
        .map(drop)
        .map_err(|error| HookError::Hook(error.to_string()))
}

fn check_migrated_preferences(transaction: &Transaction<'_>) -> HookResult {
    // The new tables must read back as valid, default preferences before the commit.
    crate::preferences::read(transaction)
        .map(drop)
        .map_err(|error| HookError::Hook(error.to_string()))
}

fn check_migrated_deletions(transaction: &Transaction<'_>) -> HookResult {
    // The new state must read back as the initial generation before the commit.
    match crate::history::applied_generation(transaction) {
        Ok(0) => Ok(()),
        Ok(_) => Err(HookError::Hook("unexpected deletion generation".to_owned())),
        Err(error) => Err(HookError::Hook(error.to_string())),
    }
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

/// One table or index that a generation owns, as `sqlite_schema` records it.
struct SchemaObject {
    kind: &'static str,
    name: &'static str,
    table: &'static str,
    sql: &'static str,
}

const fn table(name: &'static str, sql: &'static str) -> SchemaObject {
    SchemaObject {
        kind: "table",
        name,
        table: name,
        sql,
    }
}

const FLAT_OBJECTS: &[SchemaObject] = &[
    table("live_tabs", LIVE_TABLE),
    table("saved_pages", SAVED_TABLE),
    table("session_state", STATE_TABLE),
];

const FOLDER_OBJECTS: &[SchemaObject] = &[
    table("live_tabs", LIVE_TABLE),
    table("saved_folder_state", FOLDER_STATE_TABLE),
    table("saved_folders", FOLDERS_TABLE),
    table("saved_layout", LAYOUT_TABLE),
    table("saved_pages", SAVED_TABLE),
    table("session_state", STATE_TABLE),
];

const PREFERENCE_OBJECTS: &[SchemaObject] = &[
    table("live_tabs", LIVE_TABLE),
    table("quick_destinations", DESTINATIONS_TABLE),
    table("saved_folder_state", FOLDER_STATE_TABLE),
    table("saved_folders", FOLDERS_TABLE),
    table("saved_layout", LAYOUT_TABLE),
    table("saved_pages", SAVED_TABLE),
    table("session_state", STATE_TABLE),
    table("settings", SETTINGS_TABLE),
];

const HISTORY_OBJECTS: &[SchemaObject] = &[
    table("history_urls", HISTORY_URLS_TABLE),
    table("history_visits", HISTORY_VISITS_TABLE),
    SchemaObject {
        kind: "index",
        name: "history_visits_by_url",
        table: "history_visits",
        sql: HISTORY_VISITS_INDEX,
    },
    table("live_tabs", LIVE_TABLE),
    table("quick_destinations", DESTINATIONS_TABLE),
    table("saved_folder_state", FOLDER_STATE_TABLE),
    table("saved_folders", FOLDERS_TABLE),
    table("saved_layout", LAYOUT_TABLE),
    table("saved_pages", SAVED_TABLE),
    table("session_state", STATE_TABLE),
    table("settings", SETTINGS_TABLE),
];

/// Version 5 adds only this table, which sorts before every version 4 object.
/// Version 5 adds the applied clear generation and the visit-time index to version 4.
const DELETION_ADDED_OBJECTS: &[SchemaObject] = &[
    table("history_deletion_state", DELETION_STATE_TABLE),
    SchemaObject {
        kind: "index",
        name: "history_visits_by_time",
        table: "history_visits",
        sql: HISTORY_TIME_INDEX,
    },
];

/// Objects of each generation, sorted by name as `sqlite_schema` lists them.
fn expected_objects(generation: SchemaGeneration) -> Vec<&'static SchemaObject> {
    let (added, base): (&[SchemaObject], _) = match generation {
        SchemaGeneration::FlatSaved => (&[], FLAT_OBJECTS),
        SchemaGeneration::Folders => (&[], FOLDER_OBJECTS),
        SchemaGeneration::Preferences => (&[], PREFERENCE_OBJECTS),
        SchemaGeneration::History => (&[], HISTORY_OBJECTS),
        SchemaGeneration::Deletions => (DELETION_ADDED_OBJECTS, HISTORY_OBJECTS),
    };
    let mut objects: Vec<_> = added.iter().chain(base).collect();
    // Byte order of names, as `ORDER BY name` lists them under SQLite's binary collation.
    objects.sort_by_key(|object| object.name);
    objects
}

fn validate_schema(
    connection: &Connection,
    generation: SchemaGeneration,
) -> Result<(), StoreError> {
    // Match the owned schema exactly; a copied marker alone does not identify an Atlas database.
    let mut statement = connection
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name")
        .map_err(|error| database_error(StoreOperation::InspectSchema, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| database_error(StoreOperation::InspectSchema, error))?;
    for expected in expected_objects(generation) {
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
        let (Ok(kind), Ok(name), Ok(table), Ok(sql)) = values else {
            return Err(StoreError::SchemaMismatch);
        };
        if kind != expected.kind
            || name != expected.name
            || table != expected.table
            || sql != expected.sql
        {
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

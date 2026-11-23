//! Proves the compiled-code cache's records (#137): their version 9 migration, their bounds,
//! and that removing a plugin removes its record with it.

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use super::plugins::{install, site_zoom_database, user_version};
use super::site_zoom::VersionSixScripts;
use super::{TestDirectory, close, fixture};
use crate::{
    EncodingField, MAX_PLUGIN_COMPILED_BYTES, PluginCompiled, Store, StoreError, StoreOperation,
    schema,
};

fn compiled(plugin_id: &str) -> PluginCompiled {
    PluginCompiled {
        plugin_id: plugin_id.to_owned(),
        module_sha256: [3; 32],
        engine_key: [4; 32],
        syrinx_version: "0.1.0-cache-test".to_owned(),
        artifact_sha256: [5; 32],
        artifact_bytes: 4096,
    }
}

/// Builds the version 8 (plugins) schema with the fixture session and one synthetic install.
pub(super) fn plugins_database(directory: &TestDirectory) -> Connection {
    close(site_zoom_database(directory));
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let six = VersionSixScripts::new();
    let seven = schema::site_zoom_migration();
    let eight = schema::plugins_migration();
    let mut migrations = six.migrations();
    migrations.push(M::up(&seven));
    migrations.push(M::up(&eight).foreign_key_check());
    schema::apply_migrations(&mut connection, &Migrations::new(migrations))
        .expect("create synthetic version 8");
    assert_eq!(user_version(&connection), 8);
    connection
        .execute_batch(
            "INSERT INTO plugin_install VALUES
                 ('cache-test', '0.1.0', zeroblob(32), zeroblob(32), 'k', 1, 0, 0);",
        )
        .expect("seed a synthetic install");
    connection
}

#[test]
fn version_8_upgrades_to_9_keeping_installs_with_nothing_compiled() {
    let directory = TestDirectory::new();
    close(plugins_database(&directory));

    let store = Store::open(directory.lease()).expect("version 8 database upgrades");
    assert_eq!(store.load().expect("session kept").snapshot, fixture());
    let installs = store.plugin_installs().expect("installs kept");
    assert_eq!(installs.len(), 1);
    assert_eq!(store.plugin_compiled_entries(), Ok(Vec::new()));
    store.shutdown().expect("store closes");

    let connection = directory.connection();
    assert_eq!(user_version(&connection), schema::SCHEMA_VERSION);
    assert_eq!(schema::SCHEMA_VERSION, 10);
    close(connection);
    directory.cleanup();
}

#[test]
fn the_cache_hook_refuses_a_seeded_record_and_rolls_back_to_version_8() {
    let directory = TestDirectory::new();
    let mut connection = plugins_database(&directory);
    let seeded = format!(
        "{} INSERT INTO plugin_compiled VALUES
             ('cache-test', zeroblob(32), zeroblob(32), '0.1.0', zeroblob(32), 1);",
        schema::plugin_cache_migration()
    );
    let six = VersionSixScripts::new();
    let seven = schema::site_zoom_migration();
    let eight = schema::plugins_migration();
    let mut migrations = six.migrations();
    migrations.push(M::up(&seven));
    migrations.push(M::up(&eight));
    migrations.push(M::up_with_hook(
        &seeded,
        schema::check_migrated_plugin_cache,
    ));
    assert!(matches!(
        schema::apply_migrations(&mut connection, &Migrations::new(migrations)),
        Err(StoreError::Database {
            operation: StoreOperation::Migrate,
            ..
        })
    ));
    assert_eq!(user_version(&connection), 8);
    close(connection);

    let store = Store::open(directory.lease()).expect("intact version 8 database upgrades");
    assert_eq!(store.plugin_compiled_entries(), Ok(Vec::new()));
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn a_record_is_saved_replaced_read_and_forgotten() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    store
        .save_plugin_install(install("cache-test"))
        .expect("record a synthetic install");
    assert_eq!(store.plugin_compiled("cache-test"), Ok(None));

    store
        .save_plugin_compiled(compiled("cache-test"))
        .expect("record compiled code");
    let replacement = PluginCompiled {
        artifact_sha256: [6; 32],
        artifact_bytes: 8192,
        ..compiled("cache-test")
    };
    store
        .save_plugin_compiled(replacement.clone())
        .expect("replace the record");
    assert_eq!(
        store.plugin_compiled("cache-test"),
        Ok(Some(replacement.clone()))
    );
    assert_eq!(store.plugin_compiled_entries(), Ok(vec![replacement]));

    assert_eq!(
        store.forget_plugin_compiled("cache-test".to_owned()),
        Ok(true)
    );
    assert_eq!(
        store.forget_plugin_compiled("cache-test".to_owned()),
        Ok(false)
    );
    assert_eq!(store.plugin_compiled("cache-test"), Ok(None));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn a_plugin_that_is_not_installed_gets_no_record() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    assert!(matches!(
        store.save_plugin_compiled(compiled("absent")),
        Err(StoreError::Database {
            operation: StoreOperation::Write,
            ..
        })
    ));
    assert_eq!(store.plugin_compiled_entries(), Ok(Vec::new()));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn removing_a_plugin_removes_its_record_and_no_other() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    for plugin_id in ["cache-gone", "cache-kept"] {
        store
            .save_plugin_install(install(plugin_id))
            .expect("record a synthetic install");
        store
            .save_plugin_compiled(compiled(plugin_id))
            .expect("record compiled code");
    }
    assert_eq!(store.remove_plugin("cache-gone".to_owned()), Ok(true));
    assert_eq!(store.plugin_compiled("cache-gone"), Ok(None));
    assert_eq!(
        store.plugin_compiled_entries(),
        Ok(vec![compiled("cache-kept")])
    );
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn malformed_records_are_refused_before_they_are_written() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    store
        .save_plugin_install(install("cache-test"))
        .expect("record a synthetic install");
    let refused = Err(StoreError::InvalidEncoding(EncodingField::Plugin));
    let malformed = [
        PluginCompiled {
            plugin_id: "Not/An/Id".to_owned(),
            ..compiled("cache-test")
        },
        PluginCompiled {
            syrinx_version: String::new(),
            ..compiled("cache-test")
        },
        PluginCompiled {
            syrinx_version: "9".repeat(65),
            ..compiled("cache-test")
        },
        PluginCompiled {
            artifact_bytes: 0,
            ..compiled("cache-test")
        },
        PluginCompiled {
            artifact_bytes: MAX_PLUGIN_COMPILED_BYTES + 1,
            ..compiled("cache-test")
        },
    ];
    for record in malformed {
        assert_eq!(store.save_plugin_compiled(record), refused);
    }
    assert_eq!(store.plugin_compiled_entries(), Ok(Vec::new()));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn the_table_checks_the_same_bounds_the_store_refuses_before_writing() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    store
        .save_plugin_install(install("cache-test"))
        .expect("record a synthetic install");
    store.shutdown().expect("close synthetic store");

    let connection = directory.connection();
    let insert = |values: &str| {
        connection.execute_batch(&format!("INSERT INTO plugin_compiled VALUES ({values});"))
    };
    let largest = MAX_PLUGIN_COMPILED_BYTES;
    let long_version = "9".repeat(65);
    let refused_rows = [
        format!(
            "'cache-test', zeroblob(32), zeroblob(32), 'v', zeroblob(32), {}",
            largest + 1
        ),
        "'cache-test', zeroblob(32), zeroblob(32), 'v', zeroblob(32), 0".to_owned(),
        "'cache-test', zeroblob(31), zeroblob(32), 'v', zeroblob(32), 1".to_owned(),
        "'cache-test', zeroblob(32), zeroblob(33), 'v', zeroblob(32), 1".to_owned(),
        "'cache-test', zeroblob(32), zeroblob(32), '', zeroblob(32), 1".to_owned(),
        "'cache-test', zeroblob(32), zeroblob(32), 'v', zeroblob(31), 1".to_owned(),
        format!("'cache-test', zeroblob(32), zeroblob(32), '{long_version}', zeroblob(32), 1"),
    ];
    for row in &refused_rows {
        assert_refused_by_check(insert(row), row);
    }
    assert!(
        insert(&format!(
            "'cache-test', zeroblob(32), zeroblob(32), 'v', zeroblob(32), {largest}"
        ))
        .is_ok()
    );
    close(connection);
    directory.cleanup();
}

/// Assert that SQLite refused `row` for a CHECK constraint, not for any other reason.
fn assert_refused_by_check(result: rusqlite::Result<()>, row: &str) {
    let check_failed = matches!(
        &result,
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK
    );
    assert!(check_failed, "{row}: {result:?}");
}

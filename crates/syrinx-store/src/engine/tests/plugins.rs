//! Proves plugin installs, grants, folders and settings (#136): their version 8 migration,
//! their survival across a restart, each plugin's own namespace, and removal by cascade.

use std::path::PathBuf;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use super::site_zoom::{VersionSixScripts, permissions_database};
use super::{TestDirectory, close, fixture};
use crate::{
    EncodingField, FolderAccess, MAX_PLUGIN_SETTING_KEY_BYTES, MAX_PLUGIN_SETTING_VALUE_BYTES,
    MAX_PLUGIN_SETTINGS, PluginFolder, PluginGrant, PluginInstall, PluginState, Store, StoreError,
    StoreOperation, schema,
};

fn user_version(connection: &Connection) -> i64 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read synthetic schema version")
}

fn install(plugin_id: &str) -> PluginInstall {
    PluginInstall {
        plugin_id: plugin_id.to_owned(),
        version: "0.1.0".to_owned(),
        module_sha256: [1; 32],
        manifest_sha256: [2; 32],
        signer_key: "synthetic-test-key".to_owned(),
        state: PluginState::Enabled,
        installed_at: 100,
        updated_at: 100,
    }
}

fn grant(capability: &str) -> PluginGrant {
    PluginGrant {
        capability: capability.to_owned(),
        scope_json: "{}".to_owned(),
        granted_at: 200,
        granted_for_manifest: [2; 32],
    }
}

/// A generated folder path that names no real folder: under the test's own directory.
fn synthetic_folder(directory: &TestDirectory, name: &str) -> PathBuf {
    directory.path.join("granted").join(name)
}

/// A store with `plugin_ids` installed, each granted storage.
fn installed(directory: &TestDirectory, plugin_ids: &[&str]) -> Store {
    let store = Store::open(directory.lease()).expect("synthetic store");
    for plugin_id in plugin_ids {
        store
            .save_plugin_install(install(plugin_id))
            .expect("record a synthetic install");
        store
            .grant_plugin((*plugin_id).to_owned(), grant("storage"))
            .expect("grant synthetic storage");
    }
    store
}

/// Builds the version 7 (site zoom) schema with the fixture session.
fn site_zoom_database(directory: &TestDirectory) -> Connection {
    close(permissions_database(directory));
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let six = VersionSixScripts::new();
    let seven = schema::site_zoom_migration();
    let mut migrations = six.migrations();
    migrations.push(M::up(&seven));
    schema::apply_migrations(&mut connection, &Migrations::new(migrations))
        .expect("create synthetic version 7");
    assert_eq!(user_version(&connection), 7);
    connection
}

#[test]
fn version_7_upgrades_to_8_keeping_the_session_with_no_plugin_installed() {
    let directory = TestDirectory::new();
    close(site_zoom_database(&directory));

    let store = Store::open(directory.lease()).expect("version 7 database upgrades");
    assert_eq!(store.load().expect("session kept").snapshot, fixture());
    assert_eq!(store.plugin_installs(), Ok(Vec::new()));
    assert_eq!(
        store.plugin_grants(),
        Ok(Vec::new()),
        "nothing is granted by default"
    );
    store.shutdown().expect("store closes");

    let connection = directory.connection();
    assert_eq!(user_version(&connection), schema::SCHEMA_VERSION);
    assert_eq!(schema::SCHEMA_VERSION, 8);
    close(connection);
    directory.cleanup();
}

#[test]
fn the_plugin_hook_refuses_a_seeded_grant_and_rolls_back_to_version_7() {
    let directory = TestDirectory::new();
    let mut connection = site_zoom_database(&directory);
    let seeded = format!(
        "{} INSERT INTO plugin_install VALUES ('seeded', '0.1.0', zeroblob(32), zeroblob(32), 'k', 1, 0, 0);",
        schema::plugins_migration()
    );
    let six = VersionSixScripts::new();
    let seven = schema::site_zoom_migration();
    let mut migrations = six.migrations();
    migrations.push(M::up(&seven));
    migrations.push(M::up_with_hook(&seeded, schema::check_migrated_plugins));
    assert!(matches!(
        schema::apply_migrations(&mut connection, &Migrations::new(migrations)),
        Err(StoreError::Database {
            operation: StoreOperation::Migrate,
            ..
        })
    ));
    assert_eq!(user_version(&connection), 7);
    close(connection);

    let store = Store::open(directory.lease()).expect("intact version 7 database upgrades");
    assert_eq!(store.plugin_installs(), Ok(Vec::new()));
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn installs_grants_folders_and_settings_survive_a_restart() {
    let directory = TestDirectory::new();
    let store = installed(&directory, &["notes"]);
    let folder_grant = store
        .grant_plugin("notes".to_owned(), grant("folder"))
        .expect("grant a synthetic folder capability");
    let folder = PluginFolder {
        grant_id: folder_grant,
        path: synthetic_folder(&directory, "vault"),
        access: FolderAccess::ReadWrite,
    };
    store
        .add_plugin_folder("notes".to_owned(), folder.clone())
        .expect("record a synthetic folder");
    store
        .save_plugin_setting("notes".to_owned(), "notes.theme".to_owned(), vec![1, 2, 3])
        .expect("save a synthetic setting");
    store.shutdown().expect("close first store");

    let store = Store::open(directory.lease()).expect("reopen synthetic store");
    assert_eq!(store.plugin_installs(), Ok(vec![install("notes")]));
    let capabilities: Vec<String> = store
        .plugin_grants()
        .expect("grants")
        .into_iter()
        .map(|stored| stored.grant.capability)
        .collect();
    assert_eq!(capabilities, ["folder", "storage"]);
    assert_eq!(store.plugin_folders("notes"), Ok(vec![folder]));
    assert_eq!(
        store.plugin_setting("notes", "notes.theme"),
        Ok(Some(vec![1, 2, 3]))
    );
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn each_plugin_reads_and_writes_only_its_own_settings() {
    let directory = TestDirectory::new();
    let store = installed(&directory, &["first", "second"]);
    store
        .save_plugin_setting("first".to_owned(), "shared.key".to_owned(), b"one".to_vec())
        .expect("first saves");
    assert_eq!(store.plugin_setting("second", "shared.key"), Ok(None));
    store
        .save_plugin_setting(
            "second".to_owned(),
            "shared.key".to_owned(),
            b"two".to_vec(),
        )
        .expect("second saves");
    assert_eq!(
        store.plugin_setting("first", "shared.key"),
        Ok(Some(b"one".to_vec()))
    );
    assert_eq!(
        store.plugin_setting("second", "shared.key"),
        Ok(Some(b"two".to_vec()))
    );
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn a_plugin_that_is_not_installed_has_no_namespace_and_no_grants() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store");
    let refused = |result: Result<(), StoreError>| {
        assert!(
            matches!(
                result,
                Err(StoreError::Database {
                    operation: StoreOperation::Write,
                    ..
                })
            ),
            "{result:?}"
        );
    };
    refused(store.save_plugin_setting("absent".to_owned(), "key".to_owned(), Vec::new()));
    refused(
        store
            .grant_plugin("absent".to_owned(), grant("storage"))
            .map(drop),
    );
    assert_eq!(store.plugin_setting("absent", "key"), Ok(None));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn a_folder_cannot_hang_under_another_plugins_grant() {
    let directory = TestDirectory::new();
    let store = installed(&directory, &["first", "second"]);
    let first_grant = store
        .grant_plugin("first".to_owned(), grant("folder"))
        .expect("grant first a folder capability");
    let folder = PluginFolder {
        grant_id: first_grant,
        path: synthetic_folder(&directory, "first"),
        access: FolderAccess::Read,
    };
    assert!(matches!(
        store.add_plugin_folder("second".to_owned(), folder),
        Err(StoreError::Database {
            operation: StoreOperation::Write,
            ..
        })
    ));
    assert_eq!(store.plugin_folders("second"), Ok(Vec::new()));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn revoking_a_grant_removes_its_folders_and_a_regrant_keeps_its_id() {
    let directory = TestDirectory::new();
    let store = installed(&directory, &["notes"]);
    let folder_grant = store
        .grant_plugin("notes".to_owned(), grant("folder"))
        .expect("grant folder");
    let folder = PluginFolder {
        grant_id: folder_grant,
        path: synthetic_folder(&directory, "vault"),
        access: FolderAccess::Read,
    };
    store
        .add_plugin_folder("notes".to_owned(), folder)
        .expect("record folder");
    let mut widened = grant("folder");
    widened.scope_json = "{\"count\":1}".to_owned();
    assert_eq!(
        store.grant_plugin("notes".to_owned(), widened),
        Ok(folder_grant)
    );
    assert_eq!(store.plugin_folders("notes").map(|list| list.len()), Ok(1));

    assert_eq!(
        store.revoke_plugin_grant("notes".to_owned(), "folder".to_owned()),
        Ok(true)
    );
    assert_eq!(store.plugin_folders("notes"), Ok(Vec::new()));
    assert_eq!(
        store.revoke_plugin_grant("notes".to_owned(), "folder".to_owned()),
        Ok(false)
    );
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn removing_a_plugin_deletes_everything_keyed_to_it_and_nothing_else() {
    let directory = TestDirectory::new();
    let store = installed(&directory, &["gone", "kept"]);
    for plugin_id in ["gone", "kept"] {
        let folder_grant = store
            .grant_plugin(plugin_id.to_owned(), grant("folder"))
            .expect("grant folder");
        let folder = PluginFolder {
            grant_id: folder_grant,
            path: synthetic_folder(&directory, plugin_id),
            access: FolderAccess::Read,
        };
        store
            .add_plugin_folder(plugin_id.to_owned(), folder)
            .expect("record folder");
        store
            .save_plugin_setting(plugin_id.to_owned(), "key".to_owned(), vec![7])
            .expect("save setting");
    }

    assert_eq!(store.remove_plugin("gone".to_owned()), Ok(true));
    assert_eq!(store.remove_plugin("gone".to_owned()), Ok(false));
    store.shutdown().expect("close first store");

    let store = Store::open(directory.lease()).expect("reopen synthetic store");
    assert_eq!(store.plugin_installs(), Ok(vec![install("kept")]));
    let owners: Vec<String> = store
        .plugin_grants()
        .expect("grants")
        .into_iter()
        .map(|stored| stored.plugin_id)
        .collect();
    assert_eq!(owners, ["kept", "kept"]);
    assert_eq!(store.plugin_folders("gone"), Ok(Vec::new()));
    assert_eq!(store.plugin_folders("kept").map(|list| list.len()), Ok(1));
    assert_eq!(store.plugin_setting("gone", "key"), Ok(None));
    assert_eq!(store.plugin_setting("kept", "key"), Ok(Some(vec![7])));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn malformed_records_are_refused_before_they_are_written() {
    let directory = TestDirectory::new();
    let store = installed(&directory, &["notes"]);
    let refused = Err(StoreError::InvalidEncoding(EncodingField::Plugin));
    assert_eq!(store.save_plugin_install(install("Upper")), refused);
    assert_eq!(store.save_plugin_install(install("")), refused);
    let mut backwards = install("notes");
    backwards.updated_at = backwards.installed_at - 1;
    assert_eq!(store.save_plugin_install(backwards), refused);
    assert_eq!(
        store.grant_plugin("notes".to_owned(), grant("Not A Kind")),
        Err(StoreError::InvalidEncoding(EncodingField::Plugin))
    );
    let long_key = "k".repeat(MAX_PLUGIN_SETTING_KEY_BYTES + 1);
    assert_eq!(
        store.save_plugin_setting("notes".to_owned(), long_key, Vec::new()),
        refused
    );
    let big_value = vec![0; MAX_PLUGIN_SETTING_VALUE_BYTES + 1];
    assert_eq!(
        store.save_plugin_setting("notes".to_owned(), "key".to_owned(), big_value),
        refused
    );
    let relative = PluginFolder {
        grant_id: 1,
        path: PathBuf::from("relative/vault"),
        access: FolderAccess::Read,
    };
    assert_eq!(
        store.add_plugin_folder("notes".to_owned(), relative),
        refused
    );
    let mut not_json = grant("folder");
    not_json.scope_json = "{".to_owned();
    assert!(matches!(
        store.grant_plugin("notes".to_owned(), not_json),
        Err(StoreError::Database {
            operation: StoreOperation::Write,
            ..
        })
    ));
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn a_new_key_past_the_limit_is_refused_and_existing_keys_still_change() {
    let directory = TestDirectory::new();
    let store = installed(&directory, &["notes"]);
    for number in 0..MAX_PLUGIN_SETTINGS {
        store
            .save_plugin_setting("notes".to_owned(), format!("key.{number}"), Vec::new())
            .expect("save within the limit");
    }
    assert_eq!(
        store.save_plugin_setting("notes".to_owned(), "one.more".to_owned(), Vec::new()),
        Err(StoreError::TooManyRecords)
    );
    store
        .save_plugin_setting("notes".to_owned(), "key.0".to_owned(), vec![1])
        .expect("an existing key still changes");
    store.shutdown().expect("close synthetic store");
    directory.cleanup();
}

#[test]
fn a_damaged_row_refuses_the_whole_list_and_a_folder_never_prints_its_path() {
    let directory = TestDirectory::new();
    let store = installed(&directory, &["notes"]);
    store.shutdown().expect("close synthetic store");
    let connection = directory.connection();
    connection
        .execute_batch("UPDATE plugin_grant SET capability = 'Damaged'")
        .expect("damage a synthetic grant");
    close(connection);

    let store = Store::open(directory.lease()).expect("the database still opens");
    assert_eq!(
        store.plugin_grants(),
        Err(StoreError::InvalidEncoding(EncodingField::Plugin))
    );
    store.shutdown().expect("close synthetic store");

    let folder = PluginFolder {
        grant_id: 1,
        path: synthetic_folder(&directory, "private-name"),
        access: FolderAccess::Read,
    };
    assert!(!format!("{folder:?}").contains("private-name"));
    directory.cleanup();
}

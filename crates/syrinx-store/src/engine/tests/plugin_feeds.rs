//! Proves plugin search feed records (#141): their version 10 migration, the revision rule,
//! one transaction per batch, and removal with the plugin.

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use super::plugin_cache::plugins_database;
use super::plugins::{install, user_version};
use super::site_zoom::VersionSixScripts;
use super::{TestDirectory, close, fixture};
use crate::{
    EncodingField, FeedRowChange, FeedRowOutcome, PluginFeedCommit, PluginFeedEntry, Store,
    StoreError, schema,
};

const PLUGIN: &str = "feed-test";
const FEED: &str = "feed-test.items";

/// Builds the version 9 (compiled-code cache) schema with one synthetic install.
fn plugin_cache_database(directory: &TestDirectory) -> Connection {
    close(plugins_database(directory));
    let mut connection = directory.connection();
    schema::configure_connection(&connection).expect("configure synthetic connection");
    let six = VersionSixScripts::new();
    let seven = schema::site_zoom_migration();
    let eight = schema::plugins_migration();
    let nine = schema::plugin_cache_migration();
    let mut migrations = six.migrations();
    migrations.push(M::up(&seven));
    migrations.push(M::up(&eight).foreign_key_check());
    migrations.push(M::up(&nine).foreign_key_check());
    schema::apply_migrations(&mut connection, &Migrations::new(migrations))
        .expect("create synthetic version 9");
    assert_eq!(user_version(&connection), 9);
    connection
}

fn entry(entry_id: &str, revision: u64, title: &str) -> PluginFeedEntry {
    PluginFeedEntry {
        entry_id: entry_id.to_owned(),
        revision,
        title: title.to_owned(),
        subtitle: "A made-up subtitle".to_owned(),
        icon: "document".to_owned(),
        last_used: Some(1_900_000_000_000),
        keywords: vec!["synthetic".to_owned(), String::new()],
    }
}

fn commit(changes: Vec<FeedRowChange>, cursor: &str) -> PluginFeedCommit {
    PluginFeedCommit {
        plugin_id: PLUGIN.to_owned(),
        feed: FEED.to_owned(),
        clear_first: false,
        changes,
        next_cursor: cursor.to_owned(),
    }
}

fn upsert(entry_id: &str, revision: u64, title: &str) -> FeedRowChange {
    FeedRowChange::Upsert(entry(entry_id, revision, title))
}

/// The feed's rows as (id, revision, title).
fn rows(store: &Store) -> Vec<(String, u64, String)> {
    store
        .plugin_feed_entries()
        .expect("read the feed's rows")
        .into_iter()
        .map(|stored| {
            (
                stored.entry.entry_id,
                stored.entry.revision,
                stored.entry.title,
            )
        })
        .collect()
}

fn installed(directory: &TestDirectory) -> Store {
    let store = Store::open(directory.lease()).expect("synthetic store");
    store
        .save_plugin_install(install(PLUGIN))
        .expect("record a synthetic install");
    store
}

#[test]
fn version_9_upgrades_to_10_keeping_installs_with_no_feed_pulled() {
    let directory = TestDirectory::new();
    close(plugin_cache_database(&directory));

    let store = Store::open(directory.lease()).expect("version 9 database upgrades");
    assert_eq!(store.load().expect("session kept").snapshot, fixture());
    assert_eq!(store.plugin_installs().expect("installs kept").len(), 1);
    assert_eq!(store.plugin_feed_entries(), Ok(Vec::new()));
    assert_eq!(store.plugin_feed_cursor("cache-test", FEED), Ok(None));
    store.shutdown().expect("store closes");

    let connection = directory.connection();
    assert_eq!(user_version(&connection), schema::SCHEMA_VERSION);
    assert_eq!(schema::SCHEMA_VERSION, 10);
    close(connection);
    directory.cleanup();
}

#[test]
fn the_feed_hook_refuses_a_seeded_row_and_rolls_back_to_version_9() {
    let directory = TestDirectory::new();
    let mut connection = plugin_cache_database(&directory);
    let seeded = format!(
        "{} INSERT INTO plugin_feed_cursor VALUES ('cache-test', 'items', 'synthetic');",
        schema::plugin_feeds_migration()
    );
    let six = VersionSixScripts::new();
    let (seven, eight, nine) = (
        schema::site_zoom_migration(),
        schema::plugins_migration(),
        schema::plugin_cache_migration(),
    );
    let mut migrations = six.migrations();
    migrations.push(M::up(&seven));
    migrations.push(M::up(&eight));
    migrations.push(M::up(&nine));
    migrations.push(M::up_with_hook(
        &seeded,
        schema::check_migrated_plugin_feeds,
    ));
    assert!(schema::apply_migrations(&mut connection, &Migrations::new(migrations)).is_err());
    assert_eq!(user_version(&connection), 9);
    close(connection);
    directory.cleanup();
}

#[test]
fn a_newer_revision_wins_a_replay_is_unchanged_and_an_older_one_is_stale() {
    let directory = TestDirectory::new();
    let store = installed(&directory);
    let first = commit(vec![upsert("a", 2, "Second draft")], "c1");
    assert_eq!(
        store.commit_plugin_feed(first.clone()),
        Ok(vec![FeedRowOutcome::Applied])
    );
    // Replaying the same batch changes nothing, and the cursor stays where it was.
    assert_eq!(
        store.commit_plugin_feed(first),
        Ok(vec![FeedRowOutcome::Unchanged])
    );
    let older = commit(vec![upsert("a", 1, "First draft")], "c2");
    assert_eq!(
        store.commit_plugin_feed(older),
        Ok(vec![FeedRowOutcome::Stale])
    );
    let newer = commit(vec![upsert("a", 3, "Third draft")], "c3");
    assert_eq!(
        store.commit_plugin_feed(newer),
        Ok(vec![FeedRowOutcome::Applied])
    );
    assert_eq!(
        rows(&store),
        [("a".to_owned(), 3, "Third draft".to_owned())]
    );
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c3".to_owned()))
    );
    let stored = store.plugin_feed_entries().expect("read back");
    assert_eq!(stored[0].entry.keywords, ["synthetic", ""]);
    assert_eq!(stored[0].entry.last_used, Some(1_900_000_000_000));

    let deletes = commit(
        vec![
            FeedRowChange::Delete("a".to_owned()),
            FeedRowChange::Delete("a".to_owned()),
        ],
        "c4",
    );
    assert_eq!(
        store.commit_plugin_feed(deletes),
        Ok(vec![FeedRowOutcome::Applied, FeedRowOutcome::Absent])
    );
    assert!(rows(&store).is_empty());
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn the_largest_revision_round_trips() {
    let directory = TestDirectory::new();
    let store = installed(&directory);
    let changes = vec![
        upsert("a", u64::MAX - 1, "Near the top"),
        upsert("a", u64::MAX, "Top"),
    ];
    assert_eq!(
        store.commit_plugin_feed(commit(changes, "c1")),
        Ok(vec![FeedRowOutcome::Applied, FeedRowOutcome::Applied])
    );
    assert_eq!(rows(&store), [("a".to_owned(), u64::MAX, "Top".to_owned())]);
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn revisions_compare_unsigned_across_the_sign_bit() {
    let directory = TestDirectory::new();
    let store = installed(&directory);
    let top = commit(vec![upsert("a", u64::MAX, "Top")], "c1");
    store.commit_plugin_feed(top).expect("hold the largest");
    // Stored as -1 in SQLite, u64::MAX still beats 1.
    let low = commit(vec![upsert("a", 1, "Low")], "c2");
    assert_eq!(
        store.commit_plugin_feed(low),
        Ok(vec![FeedRowOutcome::Stale])
    );

    let held_one = commit(vec![upsert("b", 1, "One")], "c3");
    store.commit_plugin_feed(held_one).expect("hold one");
    // 2^63 is stored as i64::MIN, and still beats 1.
    let high = commit(vec![upsert("b", 1 << 63, "High")], "c4");
    assert_eq!(
        store.commit_plugin_feed(high),
        Ok(vec![FeedRowOutcome::Applied])
    );
    assert_eq!(
        rows(&store),
        [
            ("a".to_owned(), u64::MAX, "Top".to_owned()),
            ("b".to_owned(), 1 << 63, "High".to_owned())
        ]
    );
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn clearing_first_removes_only_that_feeds_rows() {
    let directory = TestDirectory::new();
    let store = installed(&directory);
    let other_feed = PluginFeedCommit {
        feed: "feed-test.other".to_owned(),
        ..commit(vec![upsert("kept", 1, "Other feed")], "o1")
    };
    store
        .commit_plugin_feed(other_feed)
        .expect("fill the other feed");
    let before = commit(vec![upsert("a", 1, "Old"), upsert("b", 1, "Old")], "c1");
    store.commit_plugin_feed(before).expect("fill the feed");
    let reset = PluginFeedCommit {
        clear_first: true,
        ..commit(vec![upsert("b", 1, "Fed again")], "c2")
    };
    assert_eq!(
        store.commit_plugin_feed(reset),
        Ok(vec![FeedRowOutcome::Applied])
    );
    assert_eq!(
        rows(&store),
        [
            ("b".to_owned(), 1, "Fed again".to_owned()),
            ("kept".to_owned(), 1, "Other feed".to_owned())
        ]
    );
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn a_refused_change_keeps_every_row_and_the_cursor_as_they_were() {
    let directory = TestDirectory::new();
    let store = installed(&directory);
    store
        .commit_plugin_feed(commit(vec![upsert("a", 1, "Kept")], "c1"))
        .expect("fill the feed");
    let mut oversized = entry("b", 1, "Refused");
    oversized.keywords = vec!["synthetic".to_owned(); 17];
    let refused = PluginFeedCommit {
        clear_first: true,
        ..commit(
            vec![
                upsert("c", 1, "Never kept"),
                FeedRowChange::Upsert(oversized),
            ],
            "c2",
        )
    };
    assert_eq!(
        store.commit_plugin_feed(refused),
        Err(StoreError::InvalidEncoding(EncodingField::Plugin))
    );
    let long_cursor = commit(vec![], &"c".repeat(1025));
    assert!(store.commit_plugin_feed(long_cursor).is_err());
    assert_eq!(rows(&store), [("a".to_owned(), 1, "Kept".to_owned())]);
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c1".to_owned()))
    );
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn a_feed_needs_an_installed_plugin_and_goes_with_it() {
    let directory = TestDirectory::new();
    let store = installed(&directory);
    let stranger = PluginFeedCommit {
        plugin_id: "not-installed".to_owned(),
        ..commit(vec![upsert("a", 1, "Refused")], "c1")
    };
    assert!(store.commit_plugin_feed(stranger).is_err());
    assert_eq!(store.plugin_feed_cursor("not-installed", FEED), Ok(None));

    store
        .commit_plugin_feed(commit(vec![upsert("a", 1, "Removed")], "c1"))
        .expect("fill the feed");
    assert_eq!(store.remove_plugin(PLUGIN.to_owned()), Ok(true));
    assert!(rows(&store).is_empty());
    assert_eq!(store.plugin_feed_cursor(PLUGIN, FEED), Ok(None));
    store.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn diagnostics_print_no_entry_text_or_cursor() {
    let printed = format!(
        "{:?}",
        PluginFeedCommit {
            clear_first: true,
            ..commit(
                vec![
                    upsert("secret-id", 1, "Secret title"),
                    FeedRowChange::Delete("secret-deleted".to_owned())
                ],
                "secret-cursor"
            )
        }
    );
    assert!(!printed.contains("secret"), "{printed}");
    assert!(printed.contains(PLUGIN));
}

#[test]
fn a_deleted_entry_keeps_its_revision_so_only_a_newer_one_comes_back() {
    let directory = TestDirectory::new();
    let store = installed(&directory);
    let written = commit(vec![upsert("a", 5, "Synthetic draft")], "c1");
    assert_eq!(
        store.commit_plugin_feed(written),
        Ok(vec![FeedRowOutcome::Applied])
    );
    let deleted = commit(vec![FeedRowChange::Delete("a".to_owned())], "c2");
    assert_eq!(
        store.commit_plugin_feed(deleted),
        Ok(vec![FeedRowOutcome::Applied])
    );
    assert!(rows(&store).is_empty(), "a tombstone is never read back");

    // The tombstone holds revision 5: the same and a lower revision stay deleted.
    let again = commit(
        vec![
            upsert("a", 5, "Synthetic same"),
            upsert("a", 4, "Synthetic lower"),
        ],
        "c3",
    );
    assert_eq!(
        store.commit_plugin_feed(again),
        Ok(vec![FeedRowOutcome::Unchanged, FeedRowOutcome::Stale])
    );
    assert!(rows(&store).is_empty());
    let newer = commit(vec![upsert("a", 6, "Synthetic newer")], "c4");
    assert_eq!(
        store.commit_plugin_feed(newer),
        Ok(vec![FeedRowOutcome::Applied])
    );
    assert_eq!(
        rows(&store),
        [("a".to_owned(), 6, "Synthetic newer".to_owned())]
    );

    // A reset forgets tombstones too, as the index's namespace reset does.
    let tombstone = commit(vec![FeedRowChange::Delete("a".to_owned())], "c5");
    assert_eq!(
        store.commit_plugin_feed(tombstone),
        Ok(vec![FeedRowOutcome::Applied])
    );
    let mut reset = commit(vec![upsert("a", 1, "Synthetic restarted")], "c6");
    reset.clear_first = true;
    assert_eq!(
        store.commit_plugin_feed(reset),
        Ok(vec![FeedRowOutcome::Applied])
    );
    assert_eq!(
        rows(&store),
        [("a".to_owned(), 1, "Synthetic restarted".to_owned())]
    );
    store.shutdown().expect("store closes");
    directory.cleanup();
}

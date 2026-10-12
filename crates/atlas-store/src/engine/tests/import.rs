//! Proves a bookmark import is written in one transaction, on disposable synthetic databases.

use atlas_core::{
    BookmarkImport, SavedEntryId, TabModel, TabModelSnapshot, commit_bookmark_import,
    parse_bookmark_html,
};

use super::{TestDirectory, close, fixture};
use crate::{Store, StoreError, StoreOperation};

/// A synthetic export: 30 pages over two nested folders, so failures can land mid-import.
fn synthetic_import() -> BookmarkImport {
    let mut text = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<DL><p>\n<DT><H3>Synthetic outer</H3>\n<DL><p>\n",
    );
    for page in 0..10 {
        text.push_str(&format!(
            "<DT><A HREF=\"https://outer-{page}.example.test/\">Outer {page}</A>\n"
        ));
    }
    text.push_str("<DT><H3>Synthetic inner</H3>\n<DL><p>\n");
    for page in 0..20 {
        text.push_str(&format!(
            "<DT><A HREF=\"https://inner-{page}.example.test/\">Inner {page}</A>\n"
        ));
    }
    text.push_str("</DL><p>\n</DL><p>\n</DL><p>\n");
    parse_bookmark_html(text.as_bytes()).expect("synthetic export parses")
}

/// The fixture state with the synthetic import committed in memory.
fn imported(previous: &TabModelSnapshot) -> TabModelSnapshot {
    let mut model = TabModel::from_snapshot(previous.clone()).expect("fixture restores");
    let commit = commit_bookmark_import(&mut model, &synthetic_import(), "Synthetic".to_owned())
        .expect("import commits in memory");
    assert_eq!(commit.tree.pages, 30);
    model.snapshot()
}

#[test]
fn an_import_is_one_revision_and_survives_restart_unloaded() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store opens");
    let previous = fixture();
    assert_eq!(store.save(0, previous.clone()), Ok(1));
    let after = imported(&previous);
    assert_eq!(store.save(1, after.clone()), Ok(2));
    store.shutdown().expect("store closes");

    let reopened = Store::open(directory.lease()).expect("synthetic store reopens");
    let stored = reopened.load().expect("imported state loads");
    assert_eq!(stored.revision, 2);
    assert_eq!(stored.snapshot, after);
    // The import added rows only; every live tab is the fixture's own.
    assert_eq!(stored.snapshot.tabs, previous.tabs);
    let folders = stored
        .snapshot
        .saved_layout
        .iter()
        .filter(|placement| matches!(placement.entry, SavedEntryId::Folder(_)))
        .count();
    assert_eq!(folders, 3);
    reopened.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn live_tab_changes_after_an_import_keep_saved_rows_and_round_trip() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("synthetic store opens");
    let previous = fixture();
    assert_eq!(store.save(0, previous.clone()), Ok(1));
    let after = imported(&previous);
    assert_eq!(store.save(1, after.clone()), Ok(2));
    // Saved rows are kept from here on; a trigger proves none is rewritten.
    let connection = directory.connection();
    connection
        .execute_batch("CREATE TRIGGER synthetic_no_rewrite BEFORE DELETE ON saved_pages BEGIN SELECT RAISE(ABORT, 'synthetic-rewrite'); END;")
        .expect("install synthetic rewrite guard");
    let mut model = TabModel::from_snapshot(after).expect("imported state restores");
    let tab = model.tabs()[0].id();
    let mut metadata = model.tabs()[0].metadata().clone();
    metadata.title = "Synthetic changed title".to_owned();
    model.update_metadata(tab, metadata).expect("title changes");
    let changed = model.snapshot();
    assert_eq!(store.save(2, changed.clone()), Ok(3));
    connection
        .execute_batch("DROP TRIGGER synthetic_no_rewrite")
        .expect("remove synthetic rewrite guard");
    close(connection);
    store.shutdown().expect("store closes");

    // A reopened store learns the committed revision from its load and keeps the rows too.
    let reopened = Store::open(directory.lease()).expect("synthetic store reopens");
    let stored = reopened.load().expect("changed state loads");
    let connection = directory.connection();
    connection
        .execute_batch("CREATE TRIGGER synthetic_no_rewrite BEFORE DELETE ON saved_pages BEGIN SELECT RAISE(ABORT, 'synthetic-rewrite'); END;")
        .expect("install synthetic rewrite guard");
    assert_eq!(stored.revision, 3);
    assert_eq!(stored.snapshot, changed);
    let mut model = TabModel::from_snapshot(changed).expect("changed state restores");
    let mut metadata = model.tabs()[0].metadata().clone();
    metadata.title = "Synthetic title after reopening".to_owned();
    model.update_metadata(tab, metadata).expect("title changes");
    let reopened_change = model.snapshot();
    assert_eq!(reopened.save(3, reopened_change.clone()), Ok(4));
    assert_eq!(
        reopened.load().expect("latest state loads").snapshot,
        reopened_change
    );
    connection
        .execute_batch("DROP TRIGGER synthetic_no_rewrite")
        .expect("remove synthetic rewrite guard");
    close(connection);
    reopened.shutdown().expect("store closes");
    directory.cleanup();
}

#[test]
fn an_interrupted_import_leaves_nothing_written() {
    // Fail part-way through the page rows, then on the last layout row of the import.
    for trigger in [
        "CREATE TRIGGER synthetic_interruption BEFORE INSERT ON saved_pages WHEN NEW.saved_order = 17 BEGIN SELECT RAISE(ABORT, 'synthetic-interruption'); END;",
        "CREATE TRIGGER synthetic_interruption BEFORE INSERT ON saved_layout WHEN NEW.entry_order = 34 BEGIN SELECT RAISE(ABORT, 'synthetic-interruption'); END;",
    ] {
        let directory = TestDirectory::new();
        let store = Store::open(directory.lease()).expect("synthetic store opens");
        let previous = fixture();
        assert_eq!(store.save(0, previous.clone()), Ok(1));
        let after = imported(&previous);
        let connection = directory.connection();
        connection
            .execute_batch(trigger)
            .expect("install synthetic interruption");

        let failure = store
            .save(1, after.clone())
            .expect_err("interrupted import fails");
        assert!(matches!(
            failure,
            StoreError::Database {
                operation: StoreOperation::Write,
                ..
            }
        ));
        let stored = store.load().expect("previous state loads");
        assert_eq!(stored.revision, 1);
        assert_eq!(stored.snapshot, previous);

        // The same import commits once the interruption is gone.
        connection
            .execute_batch("DROP TRIGGER synthetic_interruption")
            .expect("remove synthetic interruption");
        assert_eq!(store.save(1, after.clone()), Ok(2));
        assert_eq!(store.load().expect("import loads").snapshot, after);
        close(connection);
        store.shutdown().expect("store closes");
        directory.cleanup();
    }
}

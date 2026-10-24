//! Checks what survives a stop before a commit, or between a commit and its cleanup.

use tantivy::{Index, Term};

use super::super::*;
use super::fixture::{Fixture, NOW, delete, page_document, upsert};

#[test]
fn work_staged_but_never_committed_is_gone_after_reopening() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[upsert(page_document("kept", 1, NOW, "committedsigil"))],
            NOW,
        )
        .expect("stage first page");
    index.commit().expect("commit first page");
    index
        .stage(
            &[
                upsert(page_document("lost", 1, NOW, "uncommittedsigil")),
                delete(Namespace::page(), "kept", 1),
            ],
            NOW,
        )
        .expect("stage unfinished work");
    drop(index);

    let reopened = fixture.open();
    assert!(fixture.ids(&reopened, "uncommittedsigil").is_empty());
    assert_eq!(fixture.ids(&reopened, "committedsigil"), ["kept"]);
    assert!(
        fixture
            .audit(&reopened)
            .iter()
            .all(|row| !row.body.contains("uncommittedsigil"))
    );
}

#[test]
fn discarding_drops_the_staged_batch_only() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[upsert(page_document("kept", 1, NOW, "discardsigil"))],
            NOW,
        )
        .expect("stage kept page");
    index.commit().expect("commit kept page");
    index
        .stage(
            &[upsert(page_document("dropped", 1, NOW, "discardsigil"))],
            NOW,
        )
        .expect("stage dropped page");
    index.discard().expect("discard staged page");
    // The discarded revision is no longer counted as held.
    let again = index
        .stage(
            &[upsert(page_document("dropped", 1, NOW, "discardsigil"))],
            NOW,
        )
        .expect("stage dropped page again");
    assert_eq!(again, [Outcome::Staged]);
    index.discard().expect("discard again");
    index.commit().expect("commit nothing");
    assert_eq!(fixture.ids(&index, "discardsigil"), ["kept"]);
}

#[test]
fn opening_finishes_a_cleanup_a_crash_interrupted() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[
                upsert(page_document("gone", 1, NOW, "lingeringsigil")),
                upsert(page_document("kept", 1, NOW, "keptsigil")),
            ],
            NOW,
        )
        .expect("stage pages");
    index.commit().expect("commit pages");
    let key = schema::key(&Namespace::page(), "gone");
    let key_field = index.fields.key;
    drop(index);

    // A deletion committed without its merge leaves the stored text in its segment.
    commit_deletion_without_cleanup(&fixture, key_field, &key);

    let reopened = fixture.open();
    let rows = fixture.audit(&reopened);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].body.contains("keptsigil"));
    assert_eq!(reopened.cleared_generation(), 0);
}

/// Stands in for a crash between `commit` and its cleanup, using Tantivy directly.
fn commit_deletion_without_cleanup(fixture: &Fixture, key: tantivy::schema::Field, value: &str) {
    let index = Index::open_in_dir(&fixture.path).expect("open synthetic index directly");
    let mut writer: tantivy::IndexWriter = index
        .writer_with_num_threads(1, 15_000_000)
        .expect("open direct writer");
    writer.set_merge_policy(Box::new(tantivy::indexer::NoMergePolicy));
    writer.delete_term(Term::from_field_text(key, value));
    maintenance::commit(&mut writer, 0).expect("commit direct deletion");
    let segments = index.searchable_segment_metas().expect("read segments");
    assert!(
        segments
            .iter()
            .any(|segment| segment.num_deleted_docs() > 0)
    );
    writer.wait_merging_threads().expect("close direct writer");
}

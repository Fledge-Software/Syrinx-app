//! Checks what survives a stop before a commit, or between a commit and its cleanup.

use std::fs;

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

#[test]
fn an_index_whose_metadata_names_a_missing_segment_file_is_damaged() {
    // A failed empty can leave `meta.json` naming segment files it already deleted (#195);
    // calling that damage, not an I/O fault, lets the caller empty the folder again.
    for extension in segment_extensions() {
        let fixture = Fixture::new();
        commit_one_page(&fixture);
        let missing = segment_files(&fixture)
            .into_iter()
            .find(|file| {
                file.extension()
                    .is_some_and(|found| found == extension.as_str())
            })
            .expect("the synthetic segment has this file");
        fs::remove_file(&missing).expect("delete one synthetic segment file");
        let opened = FullText::open(&fixture.path).err();
        // Tantivy reads a missing positions file as empty, so the index opens and still
        // tracks the segment. A failed empty never leaves only it missing: it stops at the
        // first failure, and the segment's `.fast` file lists before it.
        let expected = (extension != "pos").then_some(FullTextError::Damaged);
        assert_eq!(opened, expected, "a missing .{extension} file");
    }
}

/// Commits one synthetic page, so the index holds exactly one segment.
fn commit_one_page(fixture: &Fixture) {
    let mut index = fixture.open();
    index
        .stage(&[upsert(page_document("kept", 1, NOW, "keptsigil"))], NOW)
        .expect("stage a page");
    index.commit().expect("commit a page");
}

/// The extensions of one committed segment's files.
fn segment_extensions() -> Vec<String> {
    let fixture = Fixture::new();
    commit_one_page(&fixture);
    segment_files(&fixture)
        .iter()
        .filter_map(|file| file.extension())
        .map(|extension| extension.to_string_lossy().into_owned())
        .collect()
}

/// The index's segment files: everything but its metadata and lock files.
fn segment_files(fixture: &Fixture) -> Vec<std::path::PathBuf> {
    fs::read_dir(&fixture.path)
        .expect("list synthetic index")
        .map(|entry| entry.expect("read synthetic entry").path())
        .filter(|file| is_segment_file(file))
        .collect()
}

/// Neither `meta.json`, `.managed.json` nor a `.tantivy-*` lock file.
fn is_segment_file(file: &std::path::Path) -> bool {
    file.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        !name.starts_with('.') && !name.ends_with(".json")
    })
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

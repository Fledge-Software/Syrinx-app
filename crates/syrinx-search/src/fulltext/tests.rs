//! Checks the adapter's durability, deletion and namespace rules on synthetic documents.

mod fixture;
mod recovery;
mod retention;
mod robustness;
mod scale;
mod searching;

use fixture::{Fixture, NOW, delete, notes, page_document, upsert};

use super::*;

#[test]
fn staged_changes_stay_invisible_until_commit() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    let outcomes = index
        .stage(&[upsert(page_document("a", 1, NOW, "harbourlight"))], NOW)
        .expect("stage synthetic page");
    assert_eq!(outcomes, [Outcome::Staged]);
    assert!(fixture.ids(&index, "harbourlight").is_empty());
    index.commit().expect("commit synthetic page");
    assert_eq!(fixture.ids(&index, "harbourlight"), ["a"]);
}

#[test]
fn replaying_a_committed_batch_changes_nothing() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    let batch = [
        upsert(page_document("a", 1, NOW, "replayword")),
        upsert(page_document("b", 1, NOW, "replayword")),
        delete(Namespace::page(), "c", 1),
    ];
    index.stage(&batch, NOW).expect("stage batch");
    index.commit().expect("commit batch");
    let before = fixture.audit(&index);

    let replayed = index.stage(&batch, NOW).expect("replay batch");
    assert_eq!(replayed, [Outcome::Unchanged; 3]);
    index.commit().expect("commit replay");
    assert_eq!(fixture.audit(&index), before);
}

#[test]
fn an_older_revision_is_refused_even_after_its_deletion() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(&[upsert(page_document("a", 3, NOW, "olderword"))], NOW)
        .expect("stage revision 3");
    index.commit().expect("commit revision 3");
    let stale = index
        .stage(&[upsert(page_document("a", 2, NOW, "olderword"))], NOW)
        .expect("stage revision 2");
    assert_eq!(stale, [Outcome::Stale]);

    index
        .stage(&[delete(Namespace::page(), "a", 3)], NOW)
        .expect("stage deletion");
    index.commit().expect("commit deletion");
    drop(index);

    // The tombstone is durable, so a replay after restart cannot bring the text back.
    let mut reopened = fixture.open();
    let replay = reopened
        .stage(&[upsert(page_document("a", 3, NOW, "olderword"))], NOW)
        .expect("replay deleted revision");
    assert_eq!(replay, [Outcome::Stale]);
    let newer = reopened
        .stage(&[upsert(page_document("a", 4, NOW, "newerword"))], NOW)
        .expect("stage newer revision");
    assert_eq!(newer, [Outcome::Staged]);
}

#[test]
fn a_change_within_one_batch_sees_the_change_before_it() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    let outcomes = index
        .stage(
            &[
                upsert(page_document("a", 1, NOW, "batchword")),
                upsert(page_document("a", 1, NOW, "batchword")),
                upsert(page_document("a", 2, NOW, "batchword")),
                delete(Namespace::page(), "a", 1),
            ],
            NOW,
        )
        .expect("stage dependent batch");
    assert_eq!(
        outcomes,
        [
            Outcome::Staged,
            Outcome::Unchanged,
            Outcome::Staged,
            Outcome::Stale
        ]
    );
    index.commit().expect("commit dependent batch");
    let rows = fixture.audit(&index);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].revision, 2);
}

#[test]
fn deleted_text_leaves_storage_not_just_results() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[
                upsert(page_document("gone", 1, NOW, "vanishingsigil")),
                upsert(page_document("kept", 1, NOW, "keptsigil")),
            ],
            NOW,
        )
        .expect("stage pages");
    index.commit().expect("commit pages");
    index
        .stage(&[delete(Namespace::page(), "gone", 1)], NOW)
        .expect("stage deletion");
    index.commit().expect("commit deletion");

    let rows = fixture.audit(&index);
    assert!(rows.iter().all(|row| !row.body.contains("vanishingsigil")));
    assert!(rows.iter().any(|row| row.body.contains("keptsigil")));
}

#[test]
fn maintenance_waits_for_staged_changes() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(&[upsert(page_document("a", 1, NOW, "waitingword"))], NOW)
        .expect("stage page");
    assert_eq!(index.expire(NOW), Err(FullTextError::StagedWorkPending));
    assert_eq!(index.reset(&notes()), Err(FullTextError::StagedWorkPending));
    assert_eq!(
        index.apply_clears(&[]),
        Err(FullTextError::StagedWorkPending)
    );
    index.discard().expect("discard staged page");
    assert_eq!(index.expire(NOW), Ok(0));
}

#[test]
fn invalid_ids_and_feed_names_are_refused_before_anything_is_staged() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    let refused = index.stage(
        &[
            upsert(page_document("a", 1, NOW, "invalidword")),
            upsert(page_document("", 1, NOW, "invalidword")),
        ],
        NOW,
    );
    assert_eq!(refused, Err(FullTextError::InvalidDocument));
    index.commit().expect("commit nothing");
    assert!(fixture.audit(&index).is_empty());
    for name in ["", "Notes", "notes/vault", "feed:notes", &"n".repeat(65)] {
        assert_eq!(Namespace::feed(name), Err(FullTextError::InvalidNamespace));
    }
}

#[test]
fn a_debug_print_holds_no_document_text() {
    let document = page_document("private-id.example.test", 1, NOW, "secretsigil");
    let printed = format!("{:?}", upsert(document));
    assert!(!printed.contains("secretsigil"));
    assert!(!printed.contains("private-id"));
}

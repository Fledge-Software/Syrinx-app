//! Checks that expiry, resets and journal clears each touch only the namespaces they name.

use super::super::*;
use super::fixture::{DAY, Fixture, NOW, delete, document, notes, page_document, upsert};

const EIGHT_DAYS_AGO: i64 = NOW - 8 * DAY;

#[test]
fn page_expiry_never_deletes_a_note() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[
                upsert(page_document(
                    "old",
                    1,
                    EIGHT_DAYS_AGO + DAY,
                    "expiringsigil",
                )),
                upsert(page_document("fresh", 1, NOW, "expiringsigil")),
                upsert(document(notes(), "old", 1, EIGHT_DAYS_AGO, "expiringsigil")),
                delete(Namespace::page(), "tomb", 1),
            ],
            EIGHT_DAYS_AGO + DAY,
        )
        .expect("stage mixed sources");
    index.commit().expect("commit mixed sources");

    // Page text captured eight days before `NOW` (one day after the stage time) expires.
    let removed = index.expire(NOW + DAY).expect("expire page text");
    assert_eq!(removed, 1);
    let mut found = fixture.ids(&index, "expiringsigil");
    found.sort();
    assert_eq!(found, ["fresh", "old"]);
    assert_eq!(
        fixture.ids_in(
            &index,
            "expiringsigil",
            NamespaceFilter::Only(vec![notes()])
        ),
        ["old"]
    );
    let rows = fixture.audit(&index);
    assert_eq!(
        rows.len(),
        3,
        "the fresh page, the note and the tombstone remain"
    );
}

#[test]
fn expiry_removes_old_page_tombstones_and_their_text() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[upsert(page_document("a", 1, EIGHT_DAYS_AGO, "tombsigil"))],
            EIGHT_DAYS_AGO,
        )
        .expect("stage old page");
    index.commit().expect("commit old page");
    index
        .stage(
            &[Change::Delete(Deletion {
                namespace: Namespace::page(),
                id: "a".to_owned(),
                revision: 1,
                deleted_at: EIGHT_DAYS_AGO,
            })],
            EIGHT_DAYS_AGO,
        )
        .expect("stage old deletion");
    index.commit().expect("commit old deletion");
    assert_eq!(fixture.audit(&index).len(), 1);

    index.expire(NOW).expect("expire");
    assert!(fixture.audit(&index).is_empty());
    // Once its tombstone is gone, the old revision is still refused as expired text.
    let replay = index
        .stage(
            &[upsert(page_document("a", 1, EIGHT_DAYS_AGO, "tombsigil"))],
            NOW,
        )
        .expect("replay expired page");
    assert_eq!(replay, [Outcome::Expired]);
}

#[test]
fn a_note_never_extends_page_retention() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[
                upsert(page_document(
                    "shared-id",
                    1,
                    EIGHT_DAYS_AGO + DAY,
                    "sharedsigil",
                )),
                upsert(document(notes(), "shared-id", 9, NOW, "sharedsigil")),
            ],
            EIGHT_DAYS_AGO + DAY,
        )
        .expect("stage the same id in two namespaces");
    index.commit().expect("commit both");
    index.expire(NOW + DAY).expect("expire page text");

    let hits = fixture.ids_in(
        &index,
        "sharedsigil",
        NamespaceFilter::Only(vec![Namespace::page()]),
    );
    assert!(hits.is_empty(), "the newer note kept the page's text alive");
    assert_eq!(fixture.ids(&index, "sharedsigil"), ["shared-id"]);
}

#[test]
fn resetting_one_namespace_leaves_the_others() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    let other = Namespace::feed("other-feed").expect("valid synthetic feed");
    index
        .stage(
            &[
                upsert(page_document("p", 1, NOW, "resetsigil")),
                upsert(document(notes(), "n", 1, NOW, "resetsigil")),
                upsert(document(other.clone(), "o", 1, NOW, "resetsigil")),
                delete(notes(), "gone", 1),
            ],
            NOW,
        )
        .expect("stage three sources");
    index.commit().expect("commit three sources");
    index.reset(&notes()).expect("reset notes");

    let mut found = fixture.ids(&index, "resetsigil");
    found.sort();
    assert_eq!(found, ["o", "p"]);
    assert!(
        fixture
            .audit(&index)
            .iter()
            .all(|row| !row.key.starts_with("feed:notes"))
    );
}

#[test]
fn journal_clears_reach_only_page_text_in_their_range() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[
                upsert(page_document("before", 1, NOW - 300, "clearsigil")),
                upsert(page_document("inside", 1, NOW - 100, "clearsigil")),
                upsert(page_document("end", 1, NOW, "clearsigil")),
                upsert(document(notes(), "inside", 1, NOW - 100, "clearsigil")),
            ],
            NOW,
        )
        .expect("stage pages and a note");
    index.commit().expect("commit pages and a note");

    let clear = ClearRequest {
        generation: 1,
        scope: ClearScope::Pages {
            start: NOW - 200,
            end: NOW,
        },
    };
    assert_eq!(index.apply_clears(&[clear]), Ok(1));
    let mut found = fixture.ids_in(
        &index,
        "clearsigil",
        NamespaceFilter::Only(vec![Namespace::page()]),
    );
    found.sort();
    assert_eq!(found, ["before", "end"], "the range is half-open");
    assert_eq!(
        fixture.ids_in(&index, "clearsigil", NamespaceFilter::Only(vec![notes()])),
        ["inside"]
    );
    assert_eq!(index.cleared_generation(), 1);
}

#[test]
fn replaying_the_journal_applies_each_clear_once_across_restarts() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    let first = ClearRequest {
        generation: 1,
        scope: ClearScope::Pages {
            start: 0,
            end: NOW - 1_000,
        },
    };
    assert_eq!(index.apply_clears(&[first]), Ok(1));
    index
        .stage(
            &[upsert(page_document(
                "later",
                1,
                NOW - 2_000,
                "journalsigil",
            ))],
            NOW,
        )
        .expect("stage a page dated inside the applied range");
    index.commit().expect("commit page");
    drop(index);

    // The applied generation is durable, so the same journal does not clear new text again.
    let mut reopened = fixture.open();
    assert_eq!(reopened.cleared_generation(), 1);
    let all_time = ClearRequest {
        generation: 2,
        scope: ClearScope::Pages {
            start: 0,
            end: i64::MAX,
        },
    };
    assert_eq!(reopened.apply_clears(&[first]), Ok(0));
    assert_eq!(fixture.ids(&reopened, "journalsigil"), ["later"]);
    assert_eq!(reopened.apply_clears(&[first, all_time]), Ok(1));
    assert!(fixture.ids(&reopened, "journalsigil").is_empty());
    assert_eq!(reopened.apply_clears(&[first, all_time]), Ok(0));
    assert_eq!(reopened.cleared_generation(), 2);
}

#[test]
fn a_gap_in_the_journal_is_refused_and_applies_nothing() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(&[upsert(page_document("a", 1, NOW, "gapsigil"))], NOW)
        .expect("stage page");
    index.commit().expect("commit page");
    let skipped = ClearRequest {
        generation: 2,
        scope: ClearScope::Pages {
            start: 0,
            end: i64::MAX,
        },
    };
    assert_eq!(
        index.apply_clears(&[skipped]),
        Err(FullTextError::ClearOutOfOrder)
    );
    assert_eq!(fixture.ids(&index, "gapsigil"), ["a"]);
    assert_eq!(index.cleared_generation(), 0);
}

#[test]
fn a_clear_of_something_else_advances_the_generation_and_keeps_page_text() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[upsert(page_document("kept", 1, NOW - 100, "zoomsigil"))],
            NOW,
        )
        .expect("stage page");
    index.commit().expect("commit page");

    // A site-zoom clear shares the journal's numbering but names no page text.
    let zoom = ClearRequest {
        generation: 1,
        scope: ClearScope::Elsewhere,
    };
    let history = ClearRequest {
        generation: 2,
        scope: ClearScope::Pages {
            start: 0,
            end: NOW - 1_000,
        },
    };
    assert_eq!(index.apply_clears(&[zoom, history]), Ok(2));
    assert_eq!(fixture.ids(&index, "zoomsigil"), ["kept"]);
    assert_eq!(index.cleared_generation(), 2);
}

#[test]
fn restarting_clears_removes_page_text_only_and_counts_from_zero() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[
                upsert(page_document("page", 1, NOW - 100, "restartsigil")),
                delete(Namespace::page(), "gone", 1),
                upsert(document(notes(), "note", 1, NOW - 100, "restartsigil")),
            ],
            NOW,
        )
        .expect("stage a page, a tombstone and a note");
    index.commit().expect("commit them");
    let ahead = ClearRequest {
        generation: 1,
        scope: ClearScope::Elsewhere,
    };
    assert_eq!(index.apply_clears(&[ahead]), Ok(1));

    // A journal recreated behind the index cannot say what it cleared, so page text goes.
    index.restart_clears().expect("restart the clear count");
    assert_eq!(fixture.ids(&index, "restartsigil"), ["note"]);
    assert!(
        fixture
            .audit(&index)
            .iter()
            .all(|row| !row.key.starts_with("page"))
    );
    assert_eq!(index.cleared_generation(), 0);
    drop(index);

    // Both the removal and the new count are durable.
    let reopened = fixture.open();
    assert_eq!(reopened.cleared_generation(), 0);
    assert_eq!(fixture.ids(&reopened, "restartsigil"), ["note"]);
}

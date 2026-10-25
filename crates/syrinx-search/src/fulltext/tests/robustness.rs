//! Checks the adapter stays usable after a blocked cleanup, a discard or an oversized limit.

use std::{fs, os::windows::fs::OpenOptionsExt};

use super::super::*;
use super::fixture::{Fixture, NOW, delete, page_document, upsert};

/// Windows' `FILE_SHARE_READ`: other readers may open the file, but nobody may delete it.
const SHARE_READ_ONLY: u32 = 0x1;

#[test]
fn a_blocked_cleanup_is_reported_and_retried() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(
            &[
                upsert(page_document("gone", 1, NOW, "blockedsigil")),
                upsert(page_document("kept", 1, NOW, "keptsigil")),
            ],
            NOW,
        )
        .expect("stage pages");
    index.commit().expect("commit pages");

    // Stand in for a scanner holding the segment's stored text open without delete sharing.
    let store = fs::read_dir(&fixture.path)
        .expect("list the synthetic index")
        .map(|entry| entry.expect("read an index entry").path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "store")
        })
        .expect("the committed segment's stored text");
    let holder = fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_ONLY)
        .open(&store)
        .expect("hold the stored text open");

    index
        .stage(&[delete(Namespace::page(), "gone", 1)], NOW)
        .expect("stage deletion");
    assert_eq!(index.commit(), Err(FullTextError::CleanupPending));
    assert!(index.cleanup_pending());
    // The deletion is durable and the index still answers.
    assert!(fixture.ids(&index, "blockedsigil").is_empty());
    assert_eq!(fixture.ids(&index, "keptsigil"), ["kept"]);

    drop(holder);
    index.retry_cleanup().expect("retry the cleanup");
    assert!(!index.cleanup_pending());
    assert!(!store.exists(), "the old segment's text is still on disk");
    let rows = fixture.audit(&index);
    assert!(rows.iter().all(|row| !row.body.contains("blockedsigil")));
}

#[test]
fn discarding_keeps_merges_under_the_adapters_control() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(&[upsert(page_document("a", 1, NOW, "policysigil"))], NOW)
        .expect("stage page");
    index.discard().expect("discard page");
    // Tantivy's rollback builds a new writer, which would otherwise merge in the background.
    let policy = format!("{:?}", index.writer.get_merge_policy());
    assert!(
        policy.contains("NoMergePolicy"),
        "policy after discard: {policy}"
    );
}

#[test]
fn an_oversized_limit_is_capped_instead_of_allocating() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    index
        .stage(&[upsert(page_document("a", 1, NOW, "limitsigil"))], NOW)
        .expect("stage page");
    index.commit().expect("commit page");
    let request = SearchRequest {
        text: "limitsigil".to_owned(),
        prefix_last: false,
        namespaces: NamespaceFilter::All,
        limit: usize::MAX,
    };
    assert_eq!(index.search(&request).expect("search").len(), 1);
}

#[test]
fn a_handle_without_its_writer_refuses_every_operation() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    // Stands in for a failed rollback, which released the folder's lock.
    index.broken = true;
    let refused = Err(FullTextError::NeedsReopen);
    assert_eq!(index.stage(&[], NOW), refused.map(|()| Vec::new()));
    assert_eq!(index.commit(), refused);
    assert_eq!(index.discard(), refused);
    assert_eq!(index.retry_cleanup(), refused);
    assert_eq!(index.expire(NOW), refused.map(|()| 0));
    assert_eq!(index.reset(&Namespace::page()), refused);
    assert_eq!(index.apply_clears(&[]), refused.map(|()| 0));
    let request = SearchRequest {
        text: "anything".to_owned(),
        prefix_last: false,
        namespaces: NamespaceFilter::All,
        limit: 1,
    };
    assert_eq!(index.search(&request), refused.map(|()| Vec::new()));
}

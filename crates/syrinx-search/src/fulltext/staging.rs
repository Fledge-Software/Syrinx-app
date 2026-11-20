//! Decides whether a staged change may replace what the index already holds.

use super::schema::{Held, RowKind};
use super::types::{Change, Outcome, Retention};

/// Applies the revision rules: newer wins, the same revision is a no-op, older is refused.
///
/// A tombstone counts as the newest revision it deleted, so replaying an older batch cannot
/// bring deleted text back. Page text already past D40's retention is refused outright.
pub(super) fn decide(change: &Change, held: Option<Held>, now: i64) -> Outcome {
    match change {
        Change::Upsert(document) => {
            if is_past_retention(change, now) {
                return Outcome::Expired;
            }
            decide_upsert(document.revision, held)
        }
        Change::Delete(deletion) => decide_delete(deletion.revision, held),
    }
}

fn is_past_retention(change: &Change, now: i64) -> bool {
    match change.namespace().retention() {
        Retention::ExpiresAfter(seconds) => change.dated() <= now.saturating_sub(seconds),
        Retention::Kept => false,
    }
}

fn decide_upsert(revision: u64, held: Option<Held>) -> Outcome {
    let Some(held) = held else {
        return Outcome::Staged;
    };
    match held.kind {
        RowKind::Document if held.revision == revision => Outcome::Unchanged,
        RowKind::Document if held.revision > revision => Outcome::Stale,
        RowKind::Tombstone if revision <= held.revision => Outcome::Stale,
        _ => Outcome::Staged,
    }
}

fn decide_delete(revision: u64, held: Option<Held>) -> Outcome {
    let Some(held) = held else {
        return Outcome::Staged;
    };
    match held.kind {
        RowKind::Document if revision < held.revision => Outcome::Stale,
        RowKind::Tombstone if revision <= held.revision => Outcome::Unchanged,
        _ => Outcome::Staged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fulltext::types::{Deletion, FullTextDocument, Namespace, PAGE_RETENTION_SECONDS};

    const NOW: i64 = 1_900_000_000;

    fn upsert(namespace: Namespace, revision: u64, captured_at: i64) -> Change {
        Change::Upsert(FullTextDocument {
            namespace,
            id: "doc-1".to_owned(),
            revision,
            captured_at,
            title: "Synthetic".to_owned(),
            body: "synthetic body".to_owned(),
        })
    }

    fn delete(revision: u64) -> Change {
        Change::Delete(Deletion {
            namespace: Namespace::page(),
            id: "doc-1".to_owned(),
            revision,
            deleted_at: NOW,
        })
    }

    fn held(kind: RowKind, revision: u64) -> Option<Held> {
        Some(Held { kind, revision })
    }

    #[test]
    fn a_newer_revision_replaces_and_an_older_one_is_stale() {
        let change = upsert(Namespace::page(), 5, NOW);
        assert_eq!(decide(&change, None, NOW), Outcome::Staged);
        assert_eq!(
            decide(&change, held(RowKind::Document, 4), NOW),
            Outcome::Staged
        );
        assert_eq!(
            decide(&change, held(RowKind::Document, 5), NOW),
            Outcome::Unchanged
        );
        assert_eq!(
            decide(&change, held(RowKind::Document, 6), NOW),
            Outcome::Stale
        );
    }

    #[test]
    fn a_tombstone_refuses_every_revision_it_deleted() {
        let tombstone = held(RowKind::Tombstone, 5);
        assert_eq!(
            decide(&upsert(Namespace::page(), 5, NOW), tombstone, NOW),
            Outcome::Stale
        );
        assert_eq!(
            decide(&upsert(Namespace::page(), 4, NOW), tombstone, NOW),
            Outcome::Stale
        );
        assert_eq!(
            decide(&upsert(Namespace::page(), 6, NOW), tombstone, NOW),
            Outcome::Staged
        );
    }

    #[test]
    fn deleting_is_idempotent_and_never_removes_a_newer_revision() {
        assert_eq!(decide(&delete(5), None, NOW), Outcome::Staged);
        assert_eq!(
            decide(&delete(5), held(RowKind::Document, 5), NOW),
            Outcome::Staged
        );
        assert_eq!(
            decide(&delete(5), held(RowKind::Document, 6), NOW),
            Outcome::Stale
        );
        assert_eq!(
            decide(&delete(5), held(RowKind::Tombstone, 5), NOW),
            Outcome::Unchanged
        );
        assert_eq!(
            decide(&delete(6), held(RowKind::Tombstone, 5), NOW),
            Outcome::Staged
        );
    }

    #[test]
    fn only_page_text_past_its_seven_days_is_refused() {
        let cutoff = NOW - PAGE_RETENTION_SECONDS;
        assert_eq!(
            decide(&upsert(Namespace::page(), 1, cutoff), None, NOW),
            Outcome::Expired
        );
        assert_eq!(
            decide(&upsert(Namespace::page(), 1, cutoff + 1), None, NOW),
            Outcome::Staged
        );
        let notes = Namespace::feed("notes").expect("valid synthetic feed");
        assert_eq!(decide(&upsert(notes, 1, 0), None, NOW), Outcome::Staged);
    }
}

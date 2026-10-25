//! Runs expiry, replay and prefix snippets over a bake-off-sized synthetic corpus (#52).

use super::super::*;
use super::fixture::{DAY, Fixture, NOW, document, notes};

const PAGES: u64 = 100_000;
const NOTES: u64 = 1_000;
const BATCH: u64 = 10_000;
const WORDS_PER_BODY: usize = 32;

/// A page `id % 10` days old, its body drawn from 16,384 invented words plus its own sigil.
fn synthetic_page(id: u64) -> FullTextDocument {
    let mut state = 0x5359_5249_4e58 ^ id;
    let mut body = String::new();
    for _ in 0..WORDS_PER_BODY {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        body.push_str(&format!("lexeme{:04x} ", state % 16_384));
    }
    body.push_str(&format!("sigil{id}"));
    FullTextDocument {
        namespace: Namespace::page(),
        id: format!("page-{id}"),
        revision: 1,
        captured_at: NOW - (id % 10) as i64 * DAY,
        title: format!("Fictional observatory folio {}", id % 997),
        body,
    }
}

fn is_expired(id: u64) -> bool {
    id % 10 >= 7
}

/// The corpus is captured three days before `NOW`, when even its oldest page was fresh.
const CAPTURE_TIME: i64 = NOW - 3 * DAY;

fn stage_and_commit(index: &mut FullText, changes: &[Change], now: i64) -> Vec<Outcome> {
    let outcomes = index.stage(changes, now).expect("stage synthetic batch");
    index.commit().expect("commit synthetic batch");
    outcomes
}

#[test]
fn a_hundred_thousand_pages_expire_and_replay_exactly() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    for start in (0..PAGES).step_by(BATCH as usize) {
        let batch: Vec<_> = (start..start + BATCH)
            .map(|id| Change::Upsert(synthetic_page(id)))
            .collect();
        let outcomes = stage_and_commit(&mut index, &batch, CAPTURE_TIME);
        assert!(outcomes.iter().all(|outcome| *outcome == Outcome::Staged));
    }
    let note_changes: Vec<_> = (0..NOTES)
        .map(|id| Change::Upsert(document(notes(), &format!("note-{id}"), 1, 0, "sigil7")))
        .collect();
    stage_and_commit(&mut index, &note_changes, CAPTURE_TIME);

    // Pages seven or more days old leave; notes, however old, stay.
    let removed = index.expire(NOW).expect("expire synthetic pages");
    assert_eq!(removed as u64, PAGES * 3 / 10);
    let rows = fixture.audit(&index);
    assert_eq!(rows.len() as u64, PAGES * 7 / 10 + NOTES);
    assert!(rows.iter().all(|row| {
        let Some(id) = row.key.strip_prefix("page\u{1f}page-") else {
            return true;
        };
        !is_expired(id.parse().expect("synthetic page id"))
    }));
    assert!(fixture.ids(&index, "sigil9").is_empty());
    assert_eq!(fixture.ids(&index, "sigil5"), ["page-5"]);
    assert_eq!(
        fixture
            .ids_in(&index, "sigil7", NamespaceFilter::Only(vec![notes()]))
            .len(),
        100,
        "the default limit of the fixture's query"
    );

    // Replaying the whole corpus changes nothing and resurrects nothing.
    let replay: Vec<_> = (0..PAGES)
        .map(|id| Change::Upsert(synthetic_page(id)))
        .collect();
    let outcomes = stage_and_commit(&mut index, &replay, NOW);
    for (id, outcome) in (0..PAGES).zip(outcomes) {
        let expected = if is_expired(id) {
            Outcome::Expired
        } else {
            Outcome::Unchanged
        };
        assert_eq!(outcome, expected);
    }
    assert_eq!(fixture.audit(&index), rows);

    // Prefix matches over many expansions keep their snippets.
    let request = SearchRequest {
        text: "lexeme00".to_owned(),
        prefix_last: true,
        namespaces: NamespaceFilter::Only(vec![Namespace::page()]),
        limit: 50,
    };
    let hits = index.search(&request).expect("prefix search");
    assert_eq!(hits.len(), 50);
    assert!(hits.iter().all(|hit| !hit.highlights.is_empty()));
}
